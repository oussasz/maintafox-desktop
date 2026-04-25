/**
 * Searchable parent equipment selector — browse by code / name with debounced server search.
 */

import { Check, ChevronsUpDown, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { getAssetById, listAssetChildren, listAssets } from "@/services/asset-service";
import type { Asset } from "@shared/ipc-types";

interface EquipmentParentPickerProps {
  id?: string;
  value: number | null;
  onChange: (parentId: number | null) => void;
  /** Exclude self and descendants from results */
  excludeIds?: Set<number>;
  /** Optional tree root used to suggest existing hierarchy nodes first. */
  rootAssetId?: number | null;
  disabled?: boolean;
}

export function EquipmentParentPicker({
  id,
  value,
  onChange,
  excludeIds,
  rootAssetId,
  disabled,
}: EquipmentParentPickerProps) {
  const { t } = useTranslation("equipment");
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Asset[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedAsset, setSelectedAsset] = useState<Asset | null>(null);
  const [suggestedParents, setSuggestedParents] = useState<Asset[]>([]);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>();
  const rootRef = useRef<HTMLDivElement>(null);

  const exclude = useMemo(() => excludeIds ?? new Set<number>(), [excludeIds]);

  useEffect(() => {
    if (value == null) {
      setSelectedAsset(null);
      return;
    }
    let cancelled = false;
    void getAssetById(value)
      .then((a) => {
        if (!cancelled) setSelectedAsset(a);
      })
      .catch(() => {
        if (!cancelled) setSelectedAsset(null);
      });
    return () => {
      cancelled = true;
    };
  }, [value]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  useEffect(() => {
    if (!open || rootAssetId == null) {
      setSuggestedParents([]);
      return;
    }
    let cancelled = false;
    void Promise.all([
      getAssetById(rootAssetId),
      listAssetChildren(rootAssetId)
        .then((rows) => Promise.all(rows.map((r) => getAssetById(r.child_asset_id))))
        .catch(() => [] as Asset[]),
    ])
      .then(([root, children]) => {
        if (cancelled) return;
        const all = [root, ...children].filter((a) => !exclude.has(a.id));
        const dedup = Array.from(new Map(all.map((a) => [a.id, a])).values());
        setSuggestedParents(dedup);
      })
      .catch(() => {
        if (!cancelled) setSuggestedParents([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, rootAssetId, exclude]);

  const runSearch = useCallback(
    (q: string) => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        const trimmed = q.trim();
        if (trimmed.length < 2) {
          setResults([]);
          setLoading(false);
          return;
        }
        setLoading(true);
        void listAssets(null, null, trimmed)
          .then((rows) => {
            setResults(rows.filter((a) => !exclude.has(a.id)));
          })
          .catch(() => setResults([]))
          .finally(() => setLoading(false));
      }, 280);
    },
    [exclude],
  );

  const handlePick = (a: Asset) => {
    setSelectedAsset(a);
    onChange(a.id);
    setOpen(false);
    setQuery("");
    setResults([]);
  };

  const handleClear = () => {
    setSelectedAsset(null);
    onChange(null);
    setQuery("");
    setResults([]);
  };

  return (
    <div ref={rootRef} className="relative w-full space-y-2">
      <div className="flex gap-2">
        <Button
          id={id}
          type="button"
          variant="outline"
          disabled={disabled}
          className={cn(
            "h-auto min-h-9 flex-1 justify-between px-3 py-2 font-normal text-left",
            !selectedAsset && !value && "text-text-muted",
          )}
          onClick={() => !disabled && setOpen((o) => !o)}
        >
          <span className="truncate">
            {selectedAsset
              ? `${selectedAsset.asset_code} — ${selectedAsset.asset_name}`
              : t("createForm.parentPlaceholder")}
          </span>
          <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
        </Button>
        {(value != null || selectedAsset) && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="shrink-0"
            disabled={disabled}
            onClick={handleClear}
            aria-label={t("createForm.clearParent")}
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {open && (
        <div className="absolute left-0 right-0 top-full z-50 mt-1 rounded-md border border-surface-border bg-surface-1 p-2 shadow-lg">
          <Input
            autoFocus
            value={query}
            onChange={(e) => {
              const v = e.target.value;
              setQuery(v);
              runSearch(v);
            }}
            placeholder={t("createForm.searchParent")}
            className="mb-2 h-8 text-sm"
            onKeyDown={(e) => {
              if (e.key === "Escape") setOpen(false);
            }}
          />
          <ul className="max-h-52 overflow-auto rounded-sm border border-surface-border/80 text-sm">
            {!loading && query.trim().length < 2 && suggestedParents.length > 0 && (
              <>
                <li className="border-b border-surface-border/80 px-3 py-1 text-[11px] font-medium text-text-muted">
                  {t("createForm.parentTreeSuggestions")}
                </li>
                {suggestedParents.map((a) => (
                  <li key={`suggested-${a.id}`}>
                    <button
                      type="button"
                      className={cn(
                        "flex w-full items-start gap-2 px-2 py-2 text-left hover:bg-muted",
                        a.id === value && "bg-muted/80",
                      )}
                      onClick={() => handlePick(a)}
                    >
                      <Check
                        className={cn(
                          "mt-0.5 h-4 w-4 shrink-0",
                          a.id === value ? "opacity-100" : "opacity-0",
                        )}
                      />
                      <span>
                        <span className="font-mono text-xs">{a.asset_code}</span>
                        <span className="block text-xs text-text-muted">{a.asset_name}</span>
                      </span>
                    </button>
                  </li>
                ))}
              </>
            )}
            {loading && (
              <li className="px-3 py-2 text-xs text-text-muted">
                {t("createForm.parentSearching")}
              </li>
            )}
            {!loading && query.trim().length < 2 && (
              <li className="px-3 py-2 text-xs text-text-muted">
                {t("createForm.parentMinChars")}
              </li>
            )}
            {!loading &&
              query.trim().length >= 2 &&
              results.map((a) => (
                <li key={a.id}>
                  <button
                    type="button"
                    className={cn(
                      "flex w-full items-start gap-2 px-2 py-2 text-left hover:bg-muted",
                      a.id === value && "bg-muted/80",
                    )}
                    onClick={() => handlePick(a)}
                  >
                    <Check
                      className={cn(
                        "mt-0.5 h-4 w-4 shrink-0",
                        a.id === value ? "opacity-100" : "opacity-0",
                      )}
                    />
                    <span>
                      <span className="font-mono text-xs">{a.asset_code}</span>
                      <span className="block text-xs text-text-muted">{a.asset_name}</span>
                    </span>
                  </button>
                </li>
              ))}
            {!loading && query.trim().length >= 2 && results.length === 0 && (
              <li className="px-3 py-2 text-xs text-text-muted">
                {t("createForm.parentNoResults")}
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
