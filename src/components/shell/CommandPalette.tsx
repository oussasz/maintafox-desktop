import { Activity, ClipboardList, Cog, Search, Wrench } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { type NavItem, canSeeNavItem } from "@/components/layout/Sidebar";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui";
import { useModuleCapabilities } from "@/hooks/use-module-capabilities";
import { usePermissions } from "@/hooks/use-permissions";
import { invoke } from "@/lib/ipc-invoke";
import { isModuleCapabilityAllowed } from "@/lib/module-capability";
import { cn } from "@/lib/utils";
import { defaultNavItems } from "@/navigation/nav-registry";
import type { Asset, DiListPage, WoListPage } from "@shared/ipc-types";
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

/** What a line of the palette opens: a module, or one record found by its number or name. */
interface PaletteEntry {
  key: string;
  label: string;
  /** The kind of record ("Ordre de travail"), for found records only. */
  kind?: string;
  path: string;
  icon: React.ReactNode;
}

/** Records are searched from this many typed characters, after this pause. */
const RECORD_SEARCH_MIN_CHARS = 2;
const RECORD_SEARCH_DELAY_MS = 250;
const RECORDS_PER_KIND = 5;

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const { t } = useTranslation("shell");
  const navigate = useNavigate();
  const { can } = usePermissions();
  const { capabilityMap } = useModuleCapabilities();

  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Records (work orders, requests, equipment) found by number or name, only those the user may
  // see: each search is offered only with its view right; the backend stays the authority.
  const [found, setFound] = useState<{ query: string; items: PaletteEntry[] }>({
    query: "",
    items: [],
  });
  const canWo = can(P.OT_VIEW);
  const canDi = can(P.DI_VIEW);
  const canAsset = can(P.EQ_VIEW);
  useEffect(() => {
    const text = query.trim();
    if (!open || text.length < RECORD_SEARCH_MIN_CHARS) {
      // Same empty list when already empty: no new state, no extra render.
      setFound((prev) => (prev.items.length === 0 ? prev : { query: "", items: [] }));
      return;
    }
    let stale = false;
    const timer = window.setTimeout(() => {
      // A search the user did not ask for by name: a failure or a refusal shows nothing (no
      // global error toast while typing).
      const attempt = async (run: () => Promise<PaletteEntry[]>): Promise<PaletteEntry[]> => {
        try {
          return await run();
        } catch {
          return [];
        }
      };
      void Promise.all([
        canWo
          ? attempt(async () =>
              (
                await invoke<WoListPage>(
                  "list_wo",
                  { filter: { search: text, limit: RECORDS_PER_KIND, offset: 0 } },
                  { notifyOnError: false },
                )
              ).items.map(
                (wo): PaletteEntry => ({
                  key: `wo-${wo.id}`,
                  label: `${wo.code} · ${wo.title}`,
                  kind: t("commandPalette.kind.workOrder"),
                  path: `/work-orders?openWo=${wo.id}`,
                  icon: <Wrench className="h-4 w-4" />,
                }),
              ),
            )
          : Promise.resolve([]),
        canDi
          ? attempt(async () =>
              (
                await invoke<DiListPage>(
                  "list_di",
                  { filter: { search: text, limit: RECORDS_PER_KIND, offset: 0 } },
                  { notifyOnError: false },
                )
              ).items.map(
                (di): PaletteEntry => ({
                  key: `di-${di.id}`,
                  label: `${di.code} · ${di.title}`,
                  kind: t("commandPalette.kind.request"),
                  path: `/requests?openDi=${di.id}`,
                  icon: <ClipboardList className="h-4 w-4" />,
                }),
              ),
            )
          : Promise.resolve([]),
        canAsset
          ? attempt(async () =>
              (
                await invoke<Asset[]>(
                  "list_assets",
                  {
                    statusFilter: null,
                    orgNodeFilter: null,
                    query: text,
                    limit: RECORDS_PER_KIND,
                  },
                  { notifyOnError: false },
                )
              ).map(
                (asset): PaletteEntry => ({
                  key: `asset-${asset.id}`,
                  label: `${asset.asset_code} · ${asset.asset_name}`,
                  kind: t("commandPalette.kind.equipment"),
                  path: `/equipment?assetId=${asset.id}`,
                  icon: <Cog className="h-4 w-4" />,
                }),
              ),
            )
          : Promise.resolve([]),
      ]).then((groups) => {
        if (!stale) setFound({ query: text, items: groups.flat() });
      });
    }, RECORD_SEARCH_DELAY_MS);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [query, open, canWo, canDi, canAsset, t]);

  // Same filter as the Sidebar: the user's rights and the modules the licence includes. A module
  // outside the licence is not offered here either (it used to open and refuse every list).
  // The modules are matched against their translated label.
  const entries = useMemo<PaletteEntry[]>(() => {
    const needle = query.trim().toLowerCase();
    const modules = navigableItems
      .filter(
        (item) =>
          canSeeNavItem(item, can) &&
          isModuleCapabilityAllowed(capabilityMap, item.requiredPermission),
      )
      .map((item) => ({
        key: item.key,
        label: t(item.labelKey as never) as string,
        path: item.path,
        icon: item.icon,
      }))
      .filter((item) => !needle || item.label.toLowerCase().includes(needle));
    // Records found for an earlier text are not offered for the current one.
    const records = found.query === query.trim() ? found.items : [];
    return [...modules, ...records];
  }, [can, capabilityMap, query, found, t]);

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
    setSelectedIndex((prev) => Math.min(prev, Math.max(0, entries.length - 1)));
  }, [entries.length]);

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const selected = listRef.current.querySelector("[data-selected='true']");
    selected?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const selectItem = useCallback(
    (item: PaletteEntry) => {
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
          setSelectedIndex((i) => (i + 1) % Math.max(1, entries.length));
          break;
        case "ArrowUp":
          e.preventDefault();
          setSelectedIndex((i) => (i - 1 + entries.length) % Math.max(1, entries.length));
          break;
        case "Enter":
          e.preventDefault();
          if (entries[selectedIndex]) {
            selectItem(entries[selectedIndex]);
          }
          break;
        case "Escape":
          e.preventDefault();
          onOpenChange(false);
          break;
      }
    },
    [entries, selectedIndex, selectItem, onOpenChange],
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
              entries[selectedIndex] ? `command-palette-${entries[selectedIndex].key}` : undefined
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
          {entries.length === 0 ? (
            <div className="px-3 py-6 text-center text-sm text-text-muted">
              {t("commandPalette.noResults", { query })}
            </div>
          ) : (
            entries.map((item, i) => (
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
                <span className="truncate">{item.label}</span>
                {item.kind ? (
                  <span className="ms-auto shrink-0 text-xs text-text-muted">{item.kind}</span>
                ) : null}
              </button>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
