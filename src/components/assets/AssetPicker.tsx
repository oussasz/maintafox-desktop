/**
 * AssetPicker — Design System primitive for asset identity selection.
 *
 * Used by DI, WO, and later PM / Inspection / LOTO / Inventory / Reliability.
 * Modules pass value/onChange only — no inline search gates or custom lists.
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { searchAssets, suggestPickerAssets } from "@/services/asset-search-service";
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
      <mark className="bg-amber-200/80 text-inherit rounded-sm px-0.5 dark:bg-amber-500/40">
        {match}
      </mark>
      {after}
    </>
  );
}

function findExactCodeMatch(items: AssetSearchResult[], query: string): AssetSearchResult | null {
  const folded = foldSearchText(query.trim());
  if (!folded) return null;
  const matches = items.filter((a) => foldSearchText(a.asset_code) === folded);
  return matches.length === 1 ? (matches[0] ?? null) : null;
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
}: AssetPickerProps) {
  const { t } = useTranslation("common");
  const listId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestGen = useRef(0);

  const [query, setQuery] = useState("");
  const [items, setItems] = useState<AssetSearchResult[]>([]);
  const [mode, setMode] = useState<"recent" | "frequent" | "search" | null>(null);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [scanMode, setScanMode] = useState(false);

  const loadSuggestions = useCallback(async (q: string) => {
    const gen = ++requestGen.current;
    setLoading(true);
    try {
      const trimmed = q.trim();
      if (!trimmed) {
        const res = await suggestPickerAssets({
          limit: 15,
          include_decommissioned: false,
        });
        if (gen !== requestGen.current) return;
        setItems(res.items);
        setMode(res.mode === "frequent" ? "frequent" : "recent");
        setActiveIndex(0);
        return;
      }
      const results = await searchAssets({
        query: trimmed,
        limit: 20,
        include_decommissioned: false,
      });
      if (gen !== requestGen.current) return;
      setItems(results);
      setMode("search");
      setActiveIndex(0);
    } catch {
      if (gen !== requestGen.current) return;
      setItems([]);
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
        const trimmed = query.trim();
        if (trimmed && tryExactSelect(trimmed, items)) {
          return;
        }
        // Unique exact among async results: fetch once more if list not loaded
        if (trimmed && items.length === 0) {
          void (async () => {
            try {
              const results = await searchAssets({
                query: trimmed,
                limit: 20,
                include_decommissioned: false,
              });
              if (tryExactSelect(trimmed, results)) return;
              setItems(results);
              setMode("search");
              setOpen(true);
              setActiveIndex(0);
            } catch {
              /* keep quiet; user can retry */
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
    [activeIndex, items, open, openAndLoad, query, selectAsset, tryExactSelect],
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

  const groupLabel =
    mode === "recent"
      ? t("assetPicker.recent")
      : mode === "frequent"
        ? t("assetPicker.frequent")
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
              className="h-6 w-6 p-0 shrink-0"
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

  return (
    <div className={cn("relative space-y-1", className)} ref={rootRef}>
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted pointer-events-none" />
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
            className="pl-9 pr-9"
            placeholder={scanMode ? t("assetPicker.scanHint") : t("assetPicker.searchPlaceholder")}
            value={query}
            disabled={disabled}
            autoFocus={autoFocus}
            onChange={(e) => handleQueryChange(e.target.value)}
            onFocus={() => openAndLoad()}
            onKeyDown={handleKeyDown}
          />
          {loading && (
            <Loader2 className="absolute right-2.5 top-2.5 h-4 w-4 animate-spin text-text-muted" />
          )}
        </div>
        <Button
          type="button"
          variant="outline"
          size="icon"
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
          {items.length === 0 && !loading && (
            <p className="px-3 py-3 text-sm text-text-muted">
              {query.trim() ? t("assetPicker.empty") : t("assetPicker.emptySuggestions")}
            </p>
          )}
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
        </div>
      )}

      {error && <p className="text-xs text-status-danger">{error}</p>}
    </div>
  );
}
