/**
 * DomainBrowserPanel.tsx
 *
 * Left pane of ReferenceManagerPage. Renders the domain → set hierarchy
 * as an accessible tree with client-side search, protected-domain badges,
 * context menus, and keyboard navigation.
 *
 * Phase 2 – Sub-phase 03 – Sprint S4 (GAP REF-02).
 */

import {
  ChevronDown,
  ChevronRight,
  FolderClosed,
  FolderOpen,
  Lock,
  MoreVertical,
  Plus,
  Search,
} from "lucide-react";
import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  DOMAIN_TREE_BADGE_SLOT,
  DOMAIN_TREE_BADGE_TEXT,
  DOMAIN_TREE_CHEVRON_SLOT,
  DOMAIN_TREE_ICON_SLOT,
  DOMAIN_TREE_LABEL_CLASS,
  DOMAIN_TREE_LEADING_ICON,
  DOMAIN_TREE_ROW_CLASS,
  DOMAIN_TREE_ROW_FOCUS_CLASS,
  DOMAIN_TREE_ROW_IDLE_CLASS,
  DOMAIN_TREE_ROW_SELECTED_CLASS,
  DOMAIN_TREE_SET_INDENT,
  DOMAIN_TREE_TRAIL_CLASS,
} from "@/components/lookups/domain-browser-ui";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import {
  governanceCategoryLabelKey,
  governanceCategoryShortLabelKey,
  showCategoryLockIcon,
  visibleSetsForDomain,
} from "@/lib/reference-governance-ui";
import { cn } from "@/lib/utils";
import {
  INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  WORK_ORDER_PRIORITIES_DOMAIN_ID,
  WORK_ORDER_STATUSES_DOMAIN_ID,
  WORK_ORDER_TYPES_DOMAIN_ID,
  useReferenceManagerStore,
} from "@/stores/reference-manager-store";
import type { ReferenceDomain, ReferenceSet } from "@shared/ipc-types";

// ── Status badge variant mapping ──────────────────────────────────────────────

function statusBadgeVariant(status: string) {
  switch (status) {
    case "published":
      return "default" as const;
    case "draft":
      return "secondary" as const;
    case "validated":
      return "outline" as const;
    default:
      return "secondary" as const;
  }
}

const STATUS_LABEL_KEY = {
  draft: "browser.status.draft",
  validated: "browser.status.validated",
  published: "browser.status.published",
  superseded: "browser.status.superseded",
} as const;

type SetStatus = keyof typeof STATUS_LABEL_KEY;

function statusLabelKey(status: string) {
  return STATUS_LABEL_KEY[status as SetStatus] ?? "browser.status.draft";
}

// ── Reusable tree row shell ───────────────────────────────────────────────────

interface DomainTreeRowProps {
  selected: boolean;
  focused: boolean;
  onActivate: () => void;
  children: ReactNode;
  className?: string;
  /** Omit on nested domain chrome where the outer wrapper already owns treeitem. */
  role?: "treeitem";
}

/**
 * One layout for every tree row: leading slots + flexible label + trailing rail.
 * Badges/actions must be shrink-0 siblings so the label keeps max width.
 */
function DomainTreeRow({
  selected,
  focused,
  onActivate,
  children,
  className,
  role,
}: DomainTreeRowProps) {
  return (
    <div
      role={role}
      className={cn(
        DOMAIN_TREE_ROW_CLASS,
        selected ? DOMAIN_TREE_ROW_SELECTED_CLASS : DOMAIN_TREE_ROW_IDLE_CLASS,
        focused ? DOMAIN_TREE_ROW_FOCUS_CLASS : null,
        className,
      )}
      onClick={onActivate}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onActivate();
        }
      }}
    >
      {children}
    </div>
  );
}

// ── Domain Browser Panel ──────────────────────────────────────────────────────

interface DomainBrowserPanelProps {
  onCreateDraftSet?: (domainId: number) => void;
}

export function DomainBrowserPanel({ onCreateDraftSet }: DomainBrowserPanelProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();

  const domains = useReferenceManagerStore((s) => s.domains);
  const domainCapabilities = useReferenceManagerStore((s) => s.domainCapabilities);
  const setsMap = useReferenceManagerStore((s) => s.setsMap);
  const setsLoading = useReferenceManagerStore((s) => s.setsLoading);
  const selectedDomainId = useReferenceManagerStore((s) => s.selectedDomainId);
  const selectedSetId = useReferenceManagerStore((s) => s.selectedSetId);
  const expandedDomainIds = useReferenceManagerStore((s) => s.expandedDomainIds);
  const searchFilter = useReferenceManagerStore((s) => s.searchFilter);
  const setSearchFilter = useReferenceManagerStore((s) => s.setSearchFilter);
  const selectDomain = useReferenceManagerStore((s) => s.selectDomain);
  const selectSet = useReferenceManagerStore((s) => s.selectSet);
  const toggleDomainExpanded = useReferenceManagerStore((s) => s.toggleDomainExpanded);

  const treeRef = useRef<HTMLDivElement>(null);
  const [focusedIndex, setFocusedIndex] = useState(-1);

  // ── Client-side filtering ───────────────────────────────────────────────

  const filteredDomains = useMemo(() => {
    if (!searchFilter.trim()) return domains;
    const lower = searchFilter.toLowerCase();
    return domains.filter((d) => {
      if (d.name.toLowerCase().includes(lower)) return true;
      if (d.code.toLowerCase().includes(lower)) return true;
      const sets = setsMap[d.id];
      if (
        sets?.some(
          (s) => `v${s.version_no}`.includes(lower) || s.status.toLowerCase().includes(lower),
        )
      ) {
        return true;
      }
      return false;
    });
  }, [domains, setsMap, searchFilter]);

  // ── Flat list for keyboard nav ──────────────────────────────────────────

  const flatItems = useMemo(() => {
    const items: Array<
      | { type: "domain"; domain: ReferenceDomain }
      | { type: "set"; set: ReferenceSet; domainId: number }
    > = [];
    for (const domain of filteredDomains) {
      items.push({ type: "domain", domain });
      if (expandedDomainIds.includes(domain.id)) {
        const sets = visibleSetsForDomain(domain.governance_category, setsMap[domain.id] ?? []);
        for (const refSet of sets) {
          items.push({ type: "set", set: refSet, domainId: domain.id });
        }
      }
    }
    return items;
  }, [filteredDomains, expandedDomainIds, setsMap]);

  // ── Keyboard navigation ─────────────────────────────────────────────────

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const len = flatItems.length;
      if (len === 0) return;

      let next = focusedIndex;

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          next = Math.min(focusedIndex + 1, len - 1);
          break;
        case "ArrowUp":
          e.preventDefault();
          next = Math.max(focusedIndex - 1, 0);
          break;
        case "ArrowRight": {
          e.preventDefault();
          const item = flatItems[focusedIndex];
          if (item?.type === "domain" && !expandedDomainIds.includes(item.domain.id)) {
            toggleDomainExpanded(item.domain.id);
          }
          return;
        }
        case "ArrowLeft": {
          e.preventDefault();
          const item = flatItems[focusedIndex];
          if (item?.type === "domain" && expandedDomainIds.includes(item.domain.id)) {
            toggleDomainExpanded(item.domain.id);
          } else if (item?.type === "set") {
            const parentIdx = flatItems.findIndex(
              (fi) => fi.type === "domain" && fi.domain.id === item.domainId,
            );
            if (parentIdx >= 0) next = parentIdx;
          }
          break;
        }
        case "Enter": {
          e.preventDefault();
          const item = flatItems[focusedIndex];
          if (item?.type === "domain") {
            selectDomain(item.domain.id);
          } else if (item?.type === "set") {
            selectSet(item.set.id, item.domainId);
          }
          return;
        }
        default:
          return;
      }

      setFocusedIndex(next);
    },
    [flatItems, focusedIndex, expandedDomainIds, toggleDomainExpanded, selectDomain, selectSet],
  );

  return (
    <aside className="flex h-full w-[min(100%,20rem)] min-w-[16rem] shrink-0 flex-col border-r border-surface-border">
      {/* Search */}
      <div className="border-b border-surface-border p-3">
        <div className="relative">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            value={searchFilter}
            onChange={(e) => setSearchFilter(e.target.value)}
            placeholder={t("browser.searchPlaceholder")}
            className="h-9 pl-9 text-sm"
          />
        </div>
      </div>

      {/* Tree */}
      <div
        ref={treeRef}
        role="tree"
        aria-label={t("browser.treeLabel")}
        onKeyDown={handleKeyDown}
        className="flex-1 overflow-y-auto overflow-x-hidden py-1"
      >
        {filteredDomains.length === 0 && (
          <div className="p-4 text-center text-sm text-text-muted">
            {searchFilter.trim() ? t("browser.noSearchResults") : t("browser.noDomains")}
          </div>
        )}

        {filteredDomains.map((domain) => {
          const isInventoryFamilyDomain = domain.id === INVENTORY_ARTICLE_FAMILY_DOMAIN_ID;
          const isInventoryTaxDomain = domain.id === INVENTORY_TAX_CATEGORY_DOMAIN_ID;
          const isWorkOrderTypesDomain = domain.id === WORK_ORDER_TYPES_DOMAIN_ID;
          const isWorkOrderPrioritiesDomain = domain.id === WORK_ORDER_PRIORITIES_DOMAIN_ID;
          const isWorkOrderStatusesDomain = domain.id === WORK_ORDER_STATUSES_DOMAIN_ID;
          const isSynthetic =
            isInventoryFamilyDomain ||
            isInventoryTaxDomain ||
            isWorkOrderTypesDomain ||
            isWorkOrderPrioritiesDomain ||
            isWorkOrderStatusesDomain;
          const isExpanded = expandedDomainIds.includes(domain.id);
          const isSelected = selectedDomainId === domain.id && selectedSetId === null;
          const category = domain.governance_category;
          const sets = visibleSetsForDomain(category, setsMap[domain.id] ?? []);
          const isLoadingSets = setsLoading[domain.id] ?? false;
          const flatIdx = flatItems.findIndex(
            (fi) => fi.type === "domain" && fi.domain.id === domain.id,
          );
          const showLock = showCategoryLockIcon(category);
          const domainCaps = domainCapabilities[domain.id];
          const canCreateDraft =
            can("ref.manage") && !isSynthetic && (domainCaps?.can_create_draft_set ?? false);
          const fullCategoryLabel = String(t(governanceCategoryLabelKey(category)));
          const shortCategoryLabel = String(t(governanceCategoryShortLabelKey(category)));

          return (
            <div key={domain.id} role="treeitem" aria-expanded={isExpanded}>
              <DomainTreeRow
                selected={isSelected}
                focused={focusedIndex === flatIdx}
                onActivate={() => selectDomain(domain.id)}
              >
                {/* 1. Expand */}
                <button
                  type="button"
                  className={DOMAIN_TREE_CHEVRON_SLOT}
                  onClick={(e) => {
                    e.stopPropagation();
                    toggleDomainExpanded(domain.id);
                  }}
                  aria-label={isExpanded ? t("browser.collapse") : t("browser.expand")}
                >
                  {isExpanded ? (
                    <ChevronDown className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5" />
                  )}
                </button>

                {/* 2. Folder */}
                {isExpanded ? (
                  <FolderOpen className={DOMAIN_TREE_LEADING_ICON} aria-hidden />
                ) : (
                  <FolderClosed className={DOMAIN_TREE_LEADING_ICON} aria-hidden />
                )}

                {/* 3. Label — flexible, truncates last */}
                <span className={DOMAIN_TREE_LABEL_CLASS} title={domain.name}>
                  {domain.name}
                </span>

                {/* 4. Governance badge — fixed slot, secondary */}
                {!isSynthetic ? (
                  <span
                    className={DOMAIN_TREE_BADGE_SLOT}
                    title={fullCategoryLabel}
                    aria-label={fullCategoryLabel}
                  >
                    <span className={DOMAIN_TREE_BADGE_TEXT}>{shortCategoryLabel}</span>
                  </span>
                ) : (
                  /* Reserve badge width so synthetic rows align with governed ones */
                  <span className="w-[5.25rem] shrink-0" aria-hidden />
                )}

                {/* 5–6. Lock + overflow — far-right rail */}
                <div className={DOMAIN_TREE_TRAIL_CLASS}>
                  <div className={DOMAIN_TREE_ICON_SLOT} aria-hidden={!showLock}>
                    {showLock ? (
                      <Lock
                        className="h-3.5 w-3.5 text-status-warning"
                        aria-label={t("browser.systemCatalogDomain")}
                      />
                    ) : null}
                  </div>
                  <div className={DOMAIN_TREE_ICON_SLOT}>
                    {canCreateDraft ? (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <MoreVertical className="h-3.5 w-3.5" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-44">
                          <DropdownMenuItem onClick={() => onCreateDraftSet?.(domain.id)}>
                            <Plus className="mr-2 h-3.5 w-3.5" />
                            {t("browser.newSet")}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    ) : null}
                  </div>
                </div>
              </DomainTreeRow>

              {isExpanded && (
                <fieldset className="contents">
                  {isLoadingSets && (
                    <div
                      className={cn(
                        DOMAIN_TREE_SET_INDENT,
                        "flex items-center gap-2 py-1.5 text-xs text-text-muted",
                      )}
                    >
                      <div className="h-3 w-3 animate-spin rounded-full border border-surface-3 border-t-primary" />
                      {t("browser.loadingSets")}
                    </div>
                  )}

                  {!isLoadingSets && sets.length === 0 && (
                    <div
                      className={cn(
                        DOMAIN_TREE_SET_INDENT,
                        "py-1.5 text-xs italic text-text-muted",
                      )}
                    >
                      {t("browser.noSets")}
                    </div>
                  )}

                  {sets.map((refSet) => {
                    const isSetSelected = selectedSetId === refSet.id;
                    const setFlatIdx = flatItems.findIndex(
                      (fi) => fi.type === "set" && fi.set.id === refSet.id,
                    );
                    const setLabel =
                      category === "operational_dictionary"
                        ? String(t("browser.workingCatalog"))
                        : `v${refSet.version_no}`;

                    return (
                      <DomainTreeRow
                        key={refSet.id}
                        role="treeitem"
                        selected={isSetSelected}
                        focused={focusedIndex === setFlatIdx}
                        onActivate={() => selectSet(refSet.id, domain.id)}
                        className={DOMAIN_TREE_SET_INDENT}
                      >
                        <span className={DOMAIN_TREE_LABEL_CLASS} title={setLabel}>
                          {setLabel}
                        </span>
                        {category !== "operational_dictionary" ? (
                          <Badge
                            variant={statusBadgeVariant(refSet.status)}
                            className="h-5 w-[4.5rem] shrink-0 justify-center truncate px-1 text-[10px] font-normal"
                            title={String(t(statusLabelKey(refSet.status)))}
                          >
                            {t(statusLabelKey(refSet.status))}
                          </Badge>
                        ) : null}
                      </DomainTreeRow>
                    );
                  })}
                </fieldset>
              )}
            </div>
          );
        })}
      </div>
    </aside>
  );
}
