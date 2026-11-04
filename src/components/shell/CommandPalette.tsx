import { Activity, Search } from "lucide-react";
import { useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { canSeeNavItem, type NavItem } from "@/components/layout/Sidebar";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui";
import { useModuleCapabilities } from "@/hooks/use-module-capabilities";
import { usePermissions } from "@/hooks/use-permissions";
import { isModuleCapabilityAllowed } from "@/lib/module-capability";
import { cn } from "@/lib/utils";
import { defaultNavItems } from "@/navigation/nav-registry";
import { P } from "@shared/rbac/permissions.generated";

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * The reliability methods are searchable by their own names (Weibull, AMDEC, RCM, Ishikawa); each
 * opens the reliability home page, whose first card explains and starts each analysis.
 */
const RAMS_METHOD_ITEMS: NavItem[] = (
  [
    ["rams-weibull", "nav.ramsWeibull"],
    ["rams-fmeca", "nav.ramsFmeca"],
    ["rams-rcm", "nav.ramsRcm"],
    ["rams-ishikawa", "nav.ramsIshikawa"],
  ] as const
).map(([key, labelKey]) => ({
  key,
  labelKey,
  path: "/reliability",
  icon: <Activity className="h-4 w-4" />,
  requiredPermission: P.RAM_VIEW,
}));

/** Navigable items — excludes group headers. */
const navigableItems = [...defaultNavItems.filter((i) => !i.isGroupHeader), ...RAMS_METHOD_ITEMS];

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const { t } = useTranslation("shell");
  const navigate = useNavigate();
  const { can } = usePermissions();
  const { capabilityMap } = useModuleCapabilities();

  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Same filter as the Sidebar: the user's rights and the modules the licence includes. A module
  // outside the licence is not offered here either (it used to open and refuse every list).
  const permittedItems = navigableItems.filter(
    (item) =>
      canSeeNavItem(item, can) && isModuleCapabilityAllowed(capabilityMap, item.requiredPermission),
  );

  // Search filter — match against translated label
  const filtered = query.trim()
    ? permittedItems.filter((item) => {
        const label = t(item.labelKey as never) as string;
        return label.toLowerCase().includes(query.toLowerCase());
      })
    : permittedItems;

  // Reset state when dialog opens/closes
  useEffect(() => {
    if (open) {
      setQuery("");
      setSelectedIndex(0);
      // Focus input on next tick after dialog animation
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  // Clamp selectedIndex when filtered list changes
  useEffect(() => {
    setSelectedIndex((prev) => Math.min(prev, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const selected = listRef.current.querySelector("[data-selected='true']");
    selected?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const selectItem = useCallback(
    (item: NavItem) => {
      onOpenChange(false);
      navigate(item.path);
    },
    [navigate, onOpenChange],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setSelectedIndex((i) => (i + 1) % filtered.length);
          break;
        case "ArrowUp":
          e.preventDefault();
          setSelectedIndex((i) => (i - 1 + filtered.length) % filtered.length);
          break;
        case "Enter":
          e.preventDefault();
          if (filtered[selectedIndex]) {
            selectItem(filtered[selectedIndex]);
          }
          break;
        case "Escape":
          e.preventDefault();
          onOpenChange(false);
          break;
      }
    },
    [filtered, selectedIndex, selectItem, onOpenChange],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="top-[20%] translate-y-0 p-0 gap-0 max-w-lg"
        confirmDiscard={false}
        onKeyDown={handleKeyDown}
      >
        {/* Accessible but visually hidden title */}
        <DialogTitle className="sr-only">{t("commandPalette.title")}</DialogTitle>

        {/* Search input */}
        <div className="flex items-center gap-2 border-b border-surface-border px-3">
          <Search className="h-4 w-4 text-text-muted shrink-0" aria-hidden />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("commandPalette.placeholder")}
            aria-label={t("commandPalette.placeholder")}
            role="combobox"
            aria-expanded="true"
            aria-controls="command-palette-list"
            aria-activedescendant={
              filtered[selectedIndex] ? `command-palette-${filtered[selectedIndex].key}` : undefined
            }
            className="flex-1 bg-transparent py-3 text-sm text-text-primary
                       placeholder:text-text-muted outline-none"
            autoComplete="off"
            spellCheck={false}
          />
          <kbd
            className="hidden sm:inline-flex h-5 items-center rounded border
                       border-surface-border bg-surface-2 px-1.5 text-2xs
                       text-text-muted font-mono"
          >
            Esc
          </kbd>
        </div>

        {/* Results list */}
        <div
          ref={listRef}
          id="command-palette-list"
          className="max-h-72 overflow-y-auto py-1"
          role="listbox"
          aria-label={t("commandPalette.title")}
        >
          {filtered.length === 0 ? (
            <div className="px-3 py-6 text-center text-sm text-text-muted">
              {t("commandPalette.noResults", { query })}
            </div>
          ) : (
            filtered.map((item, i) => (
              <button
                key={item.key}
                id={`command-palette-${item.key}`}
                type="button"
                tabIndex={-1}
                role="option"
                aria-selected={i === selectedIndex}
                data-selected={i === selectedIndex}
                onClick={() => selectItem(item)}
                className={cn(
                  "flex min-h-10 w-full items-center gap-3 px-3 py-2 text-sm text-start",
                  "transition-colors duration-fast",
                  i === selectedIndex
                    ? "bg-primary-bg text-primary"
                    : "text-text-secondary hover:bg-surface-2",
                )}
              >
                <span className="h-4 w-4 shrink-0" aria-hidden>
                  {item.icon}
                </span>
                <span className="truncate">{t(item.labelKey as never)}</span>
              </button>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
