/**
 * SmartFilterBar — shared search + quick-filter toolbar for list modules.
 *
 * Controlled by the page: no IPC, no stores. Debounces search (300ms default);
 * filter selects apply immediately. Active filters render as removable chips.
 *
 * URL sync is intentionally omitted — pages may add adapters later without
 * changing this component’s public API.
 */

import { RotateCcw, Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfInput, mfLayout } from "@/design-system/tokens";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { cn } from "@/lib/utils";

import type { SmartFilterBarProps, SmartFilterDef, SmartFilterOption } from "./smart-filter-types";

export type { SmartFilterBarProps, SmartFilterDef, SmartFilterOption } from "./smart-filter-types";

const DEFAULT_ALL = "__all__";

function optionLabel(options: SmartFilterOption[], value: string): string {
  return options.find((o) => o.value === value)?.label ?? value;
}

function isFilterActive(def: SmartFilterDef): boolean {
  if (def.kind === "select") {
    const all = def.allValue ?? DEFAULT_ALL;
    return def.value != null && def.value !== "" && def.value !== all;
  }
  return def.value.length > 0;
}

export function SmartFilterBar({
  searchPlaceholder,
  searchValue,
  onSearchChange,
  onSearchInputChange,
  filters,
  resultCount,
  resultCountLabel,
  onReset,
  debounceMs = 300,
  className,
}: SmartFilterBarProps) {
  const { t } = useTranslation("common");
  const [draftSearch, setDraftSearch] = useState(searchValue);
  const debouncedSearch = useDebouncedValue(draftSearch, debounceMs);
  const lastEmittedSearch = useRef(searchValue);

  // Sync external resets / parent-driven value into the draft input.
  // Only mark as emitted when the parent value actually differs from draft
  // (e.g. reset). Matching values from onSearchInputChange must not update
  // lastEmittedSearch, or the debounce would never call onSearchChange.
  useEffect(() => {
    setDraftSearch((current) => {
      if (current === searchValue) return current;
      lastEmittedSearch.current = searchValue;
      return searchValue;
    });
  }, [searchValue]);

  // Emit settled search to the parent once per debounce window.
  useEffect(() => {
    if (debouncedSearch === lastEmittedSearch.current) return;
    lastEmittedSearch.current = debouncedSearch;
    onSearchChange(debouncedSearch);
  }, [debouncedSearch, onSearchChange]);

  const handleSearchInput = (value: string) => {
    setDraftSearch(value);
    onSearchInputChange?.(value);
  };

  const clearSearch = () => {
    setDraftSearch("");
    lastEmittedSearch.current = "";
    onSearchInputChange?.("");
    onSearchChange("");
  };

  const activeFilters = useMemo(() => filters.filter(isFilterActive), [filters]);
  const searchActive = draftSearch.trim().length > 0;
  const activeCount = activeFilters.length + (searchActive ? 1 : 0);

  const clearFilter = (def: SmartFilterDef) => {
    if (def.kind === "select") {
      def.onChange(null);
    } else {
      def.onChange([]);
    }
  };

  const removeMultiValue = (def: Extract<SmartFilterDef, { kind: "multi-select" }>, value: string) => {
    def.onChange(def.value.filter((v) => v !== value));
  };

  return (
    <div className={cn(mfLayout.moduleFilterBar, "flex-col items-stretch gap-2", className)}>
      {/* Search row */}
      <div className="flex flex-wrap items-center gap-2 w-full">
        <div className="relative min-w-[200px] flex-1 max-w-xl">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-text-muted pointer-events-none" />
          <Input
            value={draftSearch}
            onChange={(e) => handleSearchInput(e.target.value)}
            placeholder={searchPlaceholder}
            className={mfInput.filterSearch}
            aria-label={searchPlaceholder}
          />
          {searchActive && (
            <button
              type="button"
              className="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-primary"
              onClick={clearSearch}
              aria-label={t("smartFilter.clearSearch")}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        <div className="flex items-center gap-2 ml-auto shrink-0">
          {resultCount != null && (
            <span className="text-xs text-text-muted whitespace-nowrap">
              {resultCountLabel ?? t("smartFilter.resultCount", { count: resultCount })}
            </span>
          )}
          {activeCount > 0 && (
            <Badge variant="secondary" className="text-[10px] h-5 px-1.5">
              {t("smartFilter.activeCount", { count: activeCount })}
            </Badge>
          )}
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-8 gap-1.5 text-xs"
            onClick={onReset}
            disabled={activeCount === 0 && !searchActive}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            {t("smartFilter.reset")}
          </Button>
        </div>
      </div>

      {/* Quick filters */}
      {filters.length > 0 && (
        <div className="flex flex-wrap items-end gap-2 w-full">
          {filters.map((def) => {
            if (def.kind === "select") {
              const all = def.allValue ?? DEFAULT_ALL;
              const selectValue = def.value ?? all;
              return (
                <div key={def.id} className="space-y-1 min-w-[140px]">
                  <span className="text-[10px] uppercase tracking-wide text-text-muted">
                    {def.label}
                  </span>
                  <Select
                    value={selectValue}
                    onValueChange={(v) => def.onChange(v === all ? null : v)}
                  >
                    <SelectTrigger className={cn(mfInput.filterSelect, "w-full min-w-[140px]")}>
                      <SelectValue placeholder={def.allLabel ?? t("smartFilter.all")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={all}>{def.allLabel ?? t("smartFilter.all")}</SelectItem>
                      {def.options.map((o) => (
                        <SelectItem key={o.value} value={o.value}>
                          {o.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              );
            }

            // multi-select: pick to add; chips remove
            const remaining = def.options.filter((o) => !def.value.includes(o.value));
            return (
              <div key={def.id} className="space-y-1 min-w-[160px]">
                <span className="text-[10px] uppercase tracking-wide text-text-muted">
                  {def.label}
                </span>
                <Select
                  key={def.value.join("|")}
                  onValueChange={(v) => {
                    if (!v || def.value.includes(v)) return;
                    def.onChange([...def.value, v]);
                  }}
                >
                  <SelectTrigger className={cn(mfInput.filterSelect, "w-full min-w-[160px]")}>
                    <SelectValue placeholder={t("smartFilter.addFilter")} />
                  </SelectTrigger>
                  <SelectContent>
                    {remaining.length === 0 ? (
                      <SelectItem value="__none__" disabled>
                        {t("smartFilter.noMoreOptions")}
                      </SelectItem>
                    ) : (
                      remaining.map((o) => (
                        <SelectItem key={o.value} value={o.value}>
                          {o.label}
                        </SelectItem>
                      ))
                    )}
                  </SelectContent>
                </Select>
              </div>
            );
          })}
        </div>
      )}

      {/* Active chips */}
      {activeCount > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 w-full">
          {searchActive && (
            <Badge variant="outline" className="gap-1 text-xs font-normal h-6 pl-2 pr-1">
              <span className="max-w-[180px] truncate">
                {t("smartFilter.searchChip", { query: draftSearch.trim() })}
              </span>
              <button
                type="button"
                className="rounded-sm p-0.5 hover:bg-surface-2"
                onClick={clearSearch}
                aria-label={t("smartFilter.clearSearch")}
              >
                <X className="h-3 w-3" />
              </button>
            </Badge>
          )}
          {activeFilters.map((def) => {
            if (def.kind === "select" && def.value) {
              return (
                <Badge
                  key={def.id}
                  variant="outline"
                  className="gap-1 text-xs font-normal h-6 pl-2 pr-1"
                >
                  <span className="text-text-muted">{def.label}:</span>
                  <span>{optionLabel(def.options, def.value)}</span>
                  <button
                    type="button"
                    className="rounded-sm p-0.5 hover:bg-surface-2"
                    onClick={() => clearFilter(def)}
                    aria-label={t("smartFilter.removeFilter", { label: def.label })}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </Badge>
              );
            }
            if (def.kind === "multi-select") {
              return def.value.map((v) => (
                <Badge
                  key={`${def.id}-${v}`}
                  variant="outline"
                  className="gap-1 text-xs font-normal h-6 pl-2 pr-1"
                >
                  <span className="text-text-muted">{def.label}:</span>
                  <span>{optionLabel(def.options, v)}</span>
                  <button
                    type="button"
                    className="rounded-sm p-0.5 hover:bg-surface-2"
                    onClick={() => removeMultiValue(def, v)}
                    aria-label={t("smartFilter.removeFilter", { label: def.label })}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </Badge>
              ));
            }
            return null;
          })}
        </div>
      )}
    </div>
  );
}
