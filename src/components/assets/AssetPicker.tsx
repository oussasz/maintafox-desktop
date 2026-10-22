/**
 * AssetPicker — Design System primitive for asset identity selection.
 *
 * Used by DI, WO, and later PM / Inspection / LOTO / Inventory / Reliability.
 * Modules pass value/onChange only — no inline search gates or custom lists.
 *
 * A failed search is shown as a failure with a retry, never as "no asset". When more assets match
 * than are listed, the list says so. A scanned identification label (`@/lib/asset-qr`) selects the
 * asset by its code; a label of the former local-id format is reported as to reprint.
 */

import { Loader2, ScanLine, Search, X } from "lucide-react";
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { CriticalityReviewMark } from "@/components/assets/CriticalityReviewMark";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { describeAppError } from "@/lib/app-error";
import { parseAssetScan } from "@/lib/asset-qr";
import { useCanReadList } from "@/lib/list-access";
import { cn } from "@/lib/utils";
import { searchAssetsPage, suggestPickerAssets } from "@/services/asset-search-service";
import type { AssetSearchResult } from "@shared/ipc-types";

// ── Accent / case fold (mirrors Rust fold_search_text) ────────────────────────

export function foldSearchText(input: string): string {
  const map: Record<string, string> = {
    à: "a",
    á: "a",
    â: "a",
    ä: "a",
    ã: "a",
    å: "a",
    è: "e",
    é: "e",
    ê: "e",
    ë: "e",
    ì: "i",
    í: "i",
    î: "i",
    ï: "i",
    ò: "o",
    ó: "o",
    ô: "o",
    ö: "o",
    õ: "o",
    ù: "u",
    ú: "u",
    û: "u",
    ü: "u",
    ý: "y",
    ÿ: "y",
    ç: "c",
    ñ: "n",
    œ: "oe",
    æ: "ae",
  };
  return input
    .toLowerCase()
    .split("")
    .map((ch) => map[ch] ?? ch)
    .join("");
}

/** Highlight matching spans in original text using folded query. */
export function highlightMatch(text: string, query: string): ReactNode {
  const q = foldSearchText(query.trim());
  if (!q) return text;

  const folded = foldSearchText(text);
  const idx = folded.indexOf(q);
  if (idx < 0) return text;

  // Map folded indices back to original character indices (ligatures expand length).
  let origStart = 0;
  let foldedPos = 0;
  const chars = [...text];
  while (foldedPos < idx && origStart < chars.length) {
    const ch = chars[origStart] ?? "";
    const foldedCh = foldSearchText(ch);
    foldedPos += foldedCh.length;
    origStart += 1;
  }
  let origEnd = origStart;
  let consumed = 0;
  while (consumed < q.length && origEnd < chars.length) {
    const ch = chars[origEnd] ?? "";
    const foldedCh = foldSearchText(ch);
    consumed += foldedCh.length;
    origEnd += 1;
  }

  const before = text.slice(0, origStart);
  const match = text.slice(origStart, origEnd);
  const after = text.slice(origEnd);
  return (
    <>
      {before}
      <mark className="rounded-sm bg-tone-attention-bg px-0.5 text-inherit">{match}</mark>
      {after}
    </>
  );
}

/** The asset's criticality, with "à revoir" when it only comes from the old list. */
function PickerCriticality({ asset }: { asset: AssetSearchResult }) {
  if (!asset.criticality_code && !asset.criticality_to_review) return null;
  return (
    <>
      {asset.criticality_code ? (
        <CriticalityBadge criticality={asset.criticality_code} compact />
      ) : null}
      {asset.criticality_to_review ? <CriticalityReviewMark /> : null}
    </>
  );
}

function findExactCodeMatch(items: AssetSearchResult[], query: string): AssetSearchResult | null {
  const folded = foldSearchText(query.trim());
  if (!folded) return null;
  const matches = items.filter((a) => foldSearchText(a.asset_code) === folded);
  return matches.length === 1 ? (matches[0] ?? null) : null;
}

/** Search page size of the picker (the list states when more assets match). */
const SEARCH_LIMIT = 20;

/**
 * Up to this many pickable assets the picker lists the whole register when the field is empty
 * (scrollable); above it, a few suggestions and a hint to type. The server caps it at 200.
 */
export const ASSET_PICKER_FULL_LIST_MAX = 50;

/** Suggestions listed for an empty field when the register is larger than the full-list limit. */
const SUGGEST_LIMIT = 15;

/** Text to search for: a scanned label's asset code, else what was typed. */
function searchTextOf(q: string): string | null {
  const scan = parseAssetScan(q);
  if (scan.kind === "legacy_label") return null;
  return scan.kind === "code" ? scan.code : scan.text;
}

// ── Props ─────────────────────────────────────────────────────────────────────

export interface AssetPickerProps {
  value: AssetSearchResult | null;
  onChange: (asset: AssetSearchResult | null) => void;
  disabled?: boolean;
  error?: string | null | undefined;
  autoFocus?: boolean;
  id?: string;
  className?: string;
  /** `sm` matches compact dialogs (`h-8`) so the picker lines up with their other fields. */
  size?: "sm" | "md";
}

// ── Component ─────────────────────────────────────────────────────────────────

export function AssetPicker({
  value,
  onChange,
  disabled = false,
  error,
  autoFocus = false,
  id,
  className,
  size = "md",
}: AssetPickerProps) {
  const compact = size === "sm";
  const { t } = useTranslation("common");
  const canReadAssets = useCanReadList("assets");
  const listId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestGen = useRef(0);

  const [query, setQuery] = useState("");
  const [items, setItems] = useState<AssetSearchResult[]>([]);
  const [mode, setMode] = useState<"recent" | "frequent" | "register" | "search" | null>(null);
  /** Suggestions of an empty field: the whole register is listed / the register is empty. */
  const [listsAll, setListsAll] = useState(false);
  const [registerEmpty, setRegisterEmpty] = useState(false);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [scanMode, setScanMode] = useState(false);
  const [searchError, setSearchError] = useState<unknown>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [legacyLabel, setLegacyLabel] = useState(false);

  const loadSuggestions = useCallback(async (q: string) => {
    const gen = ++requestGen.current;
    setSearchError(null);
    setLegacyLabel(false);
    const text = searchTextOf(q);
    if (text == null) {
      // A label of the former id format: never resolved to a possibly wrong asset.
      setItems([]);
      setTotal(null);
      setListsAll(false);
      setRegisterEmpty(false);
      setMode("search");
      setLegacyLabel(true);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const trimmed = text.trim();
      if (!trimmed) {
        const res = await suggestPickerAssets({
          limit: SUGGEST_LIMIT,
          include_decommissioned: false,
          full_list_max: ASSET_PICKER_FULL_LIST_MAX,
        });
        if (gen !== requestGen.current) return;
        setItems(res.items);
        setTotal(res.total);
        setListsAll(res.complete);
        setRegisterEmpty(res.total === 0);
        setMode(
          res.mode === "frequent" ? "frequent" : res.mode === "register" ? "register" : "recent",
        );
        setActiveIndex(0);
        return;
      }
      setListsAll(false);
      setRegisterEmpty(false);
      const page = await searchAssetsPage({
        query: trimmed,
        limit: SEARCH_LIMIT,
        offset: 0,
        include_decommissioned: false,
      });
      if (gen !== requestGen.current) return;
      setItems(page.items);
      setTotal(page.total);
      setMode("search");
      setActiveIndex(0);
    } catch (err) {
      if (gen !== requestGen.current) return;
      // Shown as a failure with a retry: an empty list would read "no asset found".
      setItems([]);
      setTotal(null);
      setSearchError(err);
      setMode("search");
    } finally {
      if (gen === requestGen.current) setLoading(false);
    }
  }, []);

  const openAndLoad = useCallback(() => {
    if (disabled) return;
    setOpen(true);
    void loadSuggestions(query);
  }, [disabled, loadSuggestions, query]);

  const selectAsset = useCallback(
    (asset: AssetSearchResult) => {
      onChange(asset);
      setQuery("");
      setItems([]);
      setOpen(false);
      setScanMode(false);
      setMode(null);
    },
    [onChange],
  );

  const clearSelection = useCallback(() => {
    onChange(null);
    setQuery("");
    setItems([]);
    setOpen(false);
    setScanMode(false);
  }, [onChange]);

  const handleQueryChange = useCallback(
    (next: string) => {
      setQuery(next);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        void loadSuggestions(next);
        setOpen(true);
      }, 250);
    },
    [loadSuggestions],
  );

  const tryExactSelect = useCallback(
    (q: string, list: AssetSearchResult[]): boolean => {
      const exact = findExactCodeMatch(list, q);
      if (exact) {
        selectAsset(exact);
        return true;
      }
      return false;
    },
    [selectAsset],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
        setScanMode(false);
        return;
      }

      if (e.key === "ArrowDown") {
        e.preventDefault();
        if (!open) {
          openAndLoad();
          return;
        }
        setActiveIndex((i) => Math.min(i + 1, Math.max(items.length - 1, 0)));
        return;
      }

      if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((i) => Math.max(i - 1, 0));
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        const text = searchTextOf(query);
        if (text == null) {
          // Former label format: say so instead of searching.
          void loadSuggestions(query);
          setOpen(true);
          return;
        }
        const trimmed = text.trim();
        if (trimmed && tryExactSelect(trimmed, items)) {
          return;
        }
        // Unique exact among async results (a scan ends with Enter): fetch once if not loaded.
        if (trimmed && items.length === 0) {
          void (async () => {
            const gen = ++requestGen.current;
            setLoading(true);
            setSearchError(null);
            try {
              const page = await searchAssetsPage({
                query: trimmed,
                limit: SEARCH_LIMIT,
                offset: 0,
                include_decommissioned: false,
              });
              if (gen !== requestGen.current) return;
              if (tryExactSelect(trimmed, page.items)) return;
              setItems(page.items);
              setTotal(page.total);
              setMode("search");
              setOpen(true);
              setActiveIndex(0);
            } catch (err) {
              if (gen !== requestGen.current) return;
              setItems([]);
              setTotal(null);
              setSearchError(err);
              setMode("search");
              setOpen(true);
            } finally {
              if (gen === requestGen.current) setLoading(false);
            }
          })();
          return;
        }
        if (open && items[activeIndex]) {
          const chosen = items[activeIndex];
          if (chosen) selectAsset(chosen);
        }
      }
    },
    [activeIndex, items, loadSuggestions, open, openAndLoad, query, selectAsset, tryExactSelect],
  );

  useEffect(() => {
    function onDocMouseDown(ev: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(ev.target as Node)) {
        setOpen(false);
        setScanMode(false);
      }
    }
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, []);

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  // Scroll active option into view
  useEffect(() => {
    if (!open) return;
    const el = rootRef.current?.querySelector<HTMLElement>(
      `[data-asset-picker-index="${activeIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  const groupLabel = listsAll
    ? t("assetPicker.all")
    : mode === "recent"
      ? t("assetPicker.recent")
      : mode === "frequent"
        ? t("assetPicker.frequent")
        : mode === "register"
          ? t("assetPicker.register")
          : mode === "search"
            ? t("assetPicker.results")
            : null;

  if (value) {
    return (
      <div className={cn("space-y-1", className)} ref={rootRef}>
        <div className="flex items-start gap-2 rounded-md border border-surface-border bg-surface-1 px-3 py-2">
          <div className="min-w-0 flex-1 space-y-0.5">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="font-mono text-sm font-semibold">{value.asset_code}</span>
              <AssetStatusBadge code={value.status_code} />
              <PickerCriticality asset={value} />
              {value.family_name && (
                <Badge variant="outline" className="text-[10px]">
                  {value.family_name}
                </Badge>
              )}
            </div>
            <p className="text-sm truncate">{value.asset_name}</p>
            {(value.org_path || value.org_node_name) && (
              <p className="text-xs text-text-muted truncate">
                {value.org_path || value.org_node_name}
              </p>
            )}
          </div>
          {!disabled && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-9 w-9 p-0 shrink-0"
              onClick={clearSelection}
              aria-label={t("assetPicker.clear")}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
        {error && <p className="text-xs text-status-danger">{error}</p>}
      </div>
    );
  }

  // Without the equipment view right the search is not offered (the server would refuse it).
  if (!canReadAssets) {
    return (
      <div className={cn("space-y-1", className)}>
        <Input
          id={id}
          disabled
          placeholder={t("assetPicker.searchPlaceholder")}
          className={compact ? "h-8 text-sm" : undefined}
        />
        <ListAccessNote list="assets" />
      </div>
    );
  }

  return (
    <div className={cn("relative space-y-1", className)} ref={rootRef}>
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search
            className={cn(
              "absolute left-2.5 h-4 w-4 text-text-muted pointer-events-none",
              compact ? "top-2" : "top-2.5",
            )}
          />
          <Input
            ref={inputRef}
            id={id}
            role="combobox"
            aria-expanded={open}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={
              open && items[activeIndex] ? `${listId}-opt-${activeIndex}` : undefined
            }
            className={cn("pl-9 pr-9", compact && "h-8 text-sm")}
            placeholder={scanMode ? t("assetPicker.scanHint") : t("assetPicker.searchPlaceholder")}
            value={query}
            disabled={disabled}
            autoFocus={autoFocus}
            onChange={(e) => handleQueryChange(e.target.value)}
            onFocus={() => openAndLoad()}
            onKeyDown={handleKeyDown}
          />
          {loading && (
            <Loader2
              className={cn(
                "absolute right-2.5 h-4 w-4 animate-spin text-text-muted",
                compact ? "top-2" : "top-2.5",
              )}
            />
          )}
        </div>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className={compact ? "h-8 w-8" : undefined}
          disabled={disabled}
          title={t("assetPicker.scan")}
          aria-label={t("assetPicker.scan")}
          onClick={() => {
            setScanMode(true);
            setOpen(false);
            inputRef.current?.focus();
          }}
        >
          <ScanLine className="h-4 w-4" />
        </Button>
      </div>

      {open && (
        <div
          id={listId}
          role="listbox"
          className="absolute z-50 left-0 right-0 mt-1 rounded-md border border-surface-border bg-surface-0 shadow-lg max-h-72 overflow-y-auto"
        >
          {groupLabel && items.length > 0 && (
            <div className="px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-muted border-b border-surface-border">
              {groupLabel}
            </div>
          )}
          {searchError != null && !loading ? (
            <div role="alert" className="flex flex-wrap items-center gap-2 px-3 py-3 text-sm">
              <span className="text-tone-danger-fg">
                {t("assetPicker.searchFailed")} {describeAppError(searchError).title}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className={compact ? "h-8" : "h-9"}
                onMouseDown={(ev) => ev.preventDefault()}
                onClick={() => void loadSuggestions(query)}
              >
                {t("assetPicker.retry")}
              </Button>
            </div>
          ) : legacyLabel ? (
            <p role="alert" className="px-3 py-3 text-sm text-tone-attention-fg">
              {t("assetPicker.legacyLabel")}
            </p>
          ) : items.length === 0 && !loading ? (
            <p className="px-3 py-3 text-sm text-text-muted">
              {query.trim()
                ? t("assetPicker.empty")
                : registerEmpty
                  ? t("assetPicker.emptyRegister")
                  : t("assetPicker.emptySuggestions")}
            </p>
          ) : null}
          {items.map((asset, index) => (
            <button
              key={asset.id}
              type="button"
              id={`${listId}-opt-${index}`}
              role="option"
              aria-selected={index === activeIndex}
              data-asset-picker-index={index}
              className={cn(
                "flex w-full flex-col items-start gap-0.5 px-3 py-2 text-left text-sm transition-colors",
                index === activeIndex ? "bg-surface-2" : "hover:bg-surface-1",
              )}
              onMouseEnter={() => setActiveIndex(index)}
              onMouseDown={(ev) => {
                ev.preventDefault();
                selectAsset(asset);
              }}
            >
              <div className="flex w-full items-center gap-2 min-w-0">
                <span className="font-mono text-xs font-semibold shrink-0">
                  {highlightMatch(asset.asset_code, query)}
                </span>
                <AssetStatusBadge code={asset.status_code} />
                <PickerCriticality asset={asset} />
                {asset.family_name && (
                  <Badge variant="outline" className="text-[10px] ml-auto shrink-0">
                    {asset.family_name}
                  </Badge>
                )}
              </div>
              <span className="truncate w-full">{highlightMatch(asset.asset_name, query)}</span>
              {(asset.org_path || asset.org_node_name) && (
                <span className="text-xs text-text-muted truncate w-full">
                  {asset.org_path || asset.org_node_name}
                </span>
              )}
            </button>
          ))}
          {total != null && total > items.length ? (
            <p className="border-t border-surface-border px-3 py-2 text-xs text-text-muted">
              {mode === "search"
                ? t("assetPicker.more", { shown: items.length, total })
                : t("assetPicker.typeToSearch", { total })}
            </p>
          ) : null}
        </div>
      )}

      {error && <p className="text-xs text-status-danger">{error}</p>}
    </div>
  );
}
