/**
 * Module header controls — the right-hand toolbar of every module page (OT/DI pattern):
 *   [primary create action] [view switcher] [filter toggle] [refresh]
 *
 * Icon-only controls always carry an accessible name (aria-label + tooltip) and a 44 px target.
 */
import type { LucideIcon } from "lucide-react";
import { Filter, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface ViewOption<V extends string> {
  value: V;
  label: string;
  icon: LucideIcon;
  /** Hide the option (e.g. permission-gated views). */
  hidden?: boolean;
}

export interface ViewToggleProps<V extends string> {
  value: V;
  options: ViewOption<V>[];
  onChange: (value: V) => void;
}

/** Segmented view switcher (list / board / calendar / dashboard …). */
export function ViewToggle<V extends string>({ value, options, onChange }: ViewToggleProps<V>) {
  const { t } = useTranslation("common");
  const visible = options.filter((o) => !o.hidden);
  if (visible.length < 2) return null;
  return (
    <div role="radiogroup" aria-label={t("ds.view.label")} className={mfLayout.viewToggleGroup}>
      {visible.map(({ value: v, label, icon: Icon }) => {
        const active = v === value;
        return (
          <button
            key={v}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={label}
            title={label}
            onClick={() => onChange(v)}
            className={cn(
              mfLayout.viewToggleButton,
              active
                ? "bg-primary text-primary-foreground shadow-sm"
                : "text-text-secondary hover:bg-surface-3 hover:text-text-primary",
            )}
          >
            <Icon className="h-4 w-4" aria-hidden />
          </button>
        );
      })}
    </div>
  );
}

export interface FilterToggleButtonProps {
  open: boolean;
  onToggle: () => void;
  /** Number of active filters, shown as a counter when the bar is hidden. */
  activeCount?: number;
}

export function FilterToggleButton({ open, onToggle, activeCount = 0 }: FilterToggleButtonProps) {
  const { t } = useTranslation("common");
  const label = open ? t("ds.toolbar.hideFilters") : t("ds.toolbar.showFilters");
  return (
    <Button
      type="button"
      variant={open ? "secondary" : "outline"}
      className={cn(mfLayout.toolbarIconButton, "relative")}
      onClick={onToggle}
      aria-pressed={open}
      aria-label={label}
      title={label}
    >
      <Filter className="h-4 w-4" aria-hidden />
      {!open && activeCount > 0 ? (
        <span className="absolute -right-1 -top-1 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-semibold text-accent-foreground">
          {activeCount}
        </span>
      ) : null}
    </Button>
  );
}

export interface RefreshButtonProps {
  onRefresh: () => void;
  loading?: boolean;
}

export function RefreshButton({ onRefresh, loading = false }: RefreshButtonProps) {
  const { t } = useTranslation("common");
  const label = loading ? t("ds.toolbar.refreshing") : t("ds.toolbar.refresh");
  return (
    <Button
      type="button"
      variant="outline"
      className={mfLayout.toolbarIconButton}
      onClick={onRefresh}
      disabled={loading}
      aria-label={label}
      title={label}
    >
      <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} aria-hidden />
    </Button>
  );
}
