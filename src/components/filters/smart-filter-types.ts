/**
 * SmartFilterBar filter definition types.
 *
 * Pages own values and side effects (stores / IPC). This component is presentational
 * plus debounce/chip UX. URL sync is intentionally not handled here — adapters can
 * sync later without changing this API.
 */

export type SmartFilterOption = {
  value: string;
  label: string;
};

export type SmartFilterSelectDef = {
  id: string;
  kind: "select";
  label: string;
  options: SmartFilterOption[];
  /** Selected value, or null / allValue for “all”. */
  value: string | null;
  onChange: (value: string | null) => void;
  /** Sentinel for “all” in the Select (default `__all__`). */
  allValue?: string;
  allLabel?: string;
};

export type SmartFilterMultiSelectDef = {
  id: string;
  kind: "multi-select";
  label: string;
  options: SmartFilterOption[];
  value: string[];
  onChange: (value: string[]) => void;
};

export type SmartFilterDef = SmartFilterSelectDef | SmartFilterMultiSelectDef;

export type SmartFilterBarProps = {
  searchPlaceholder: string;
  /** Immediate controlled search text shown in the input. */
  searchValue: string;
  /**
   * Called with the settled search string after debounce (default 300ms).
   * Also called immediately when the search chip is cleared or reset runs via parent.
   */
  onSearchChange: (value: string) => void;
  /**
   * Optional: when provided, parent is notified of every keystroke so the input
   * stays controlled. If omitted, the bar keeps an internal draft synced from searchValue.
   */
  onSearchInputChange?: (value: string) => void;
  filters: SmartFilterDef[];
  resultCount?: number | null;
  /** Pre-translated count label, e.g. "12 results". */
  resultCountLabel?: string;
  onReset: () => void;
  debounceMs?: number;
  className?: string;
};
