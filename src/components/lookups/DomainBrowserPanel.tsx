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
  Pencil,
  Plus,
  Search,
} from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
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

// ── Domain Browser Panel ──────────────────────────────────────────────────────

interface DomainBrowserPanelProps {
  onCreateDraftSet?: (domainId: number) => void;
  onRenameDomain?: (domain: ReferenceDomain) => void;
}

export function DomainBrowserPanel({ onCreateDraftSet, onRenameDomain }: DomainBrowserPanelProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();

  const domains = useReferenceManagerStore((s) => s.domains);
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
      // Match domain name
      if (d.name.toLowerCase().includes(lower)) return true;
      if (d.code.toLowerCase().includes(lower)) return true;
      // Match any set name in this domain
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
        const sets = setsMap[domain.id] ?? [];
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
            // Move focus up to parent domain
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

  // ── Render helpers ──────────────────────────────────────────────────────

  const isProtected = (d: ReferenceDomain) => d.governance_level === "protected_analytical";

  return (
    <aside className="flex h-full flex-col w-[300px] shrink-0 border-r border-surface-border">
      {/* Search */}
      <div className="p-3 border-b border-surface-border">
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
        className="flex-1 overflow-y-auto py-1"
      >
        {filteredDomains.length === 0 && (
          <div className="p-4 text-center text-sm text-text-muted">
            {searchFilter.trim() ? t("browser.noSearchResults") : t("browser.noDomains")}
          </div>
        )}

        {filteredDomains.map((domain) => {
          const isExpanded = expandedDomainIds.includes(domain.id);
          const isSelected = selectedDomainId === domain.id && selectedSetId === null;
          const sets = setsMap[domain.id] ?? [];
          const isLoadingSets = setsLoading[domain.id] ?? false;
          const flatIdx = flatItems.findIndex(
            (fi) => fi.type === "domain" && fi.domain.id === domain.id,
          );

          return (
            <div key={domain.id} role="treeitem" aria-expanded={isExpanded}>
              {/* Domain row */}
              <div
                className={`group flex items-center gap-1.5 px-3 py-1.5 cursor-pointer text-sm hover:bg-surface-1 ${
                  isSelected ? "bg-surface-2 text-text-primary font-medium" : "text-text-secondary"
                } ${focusedIndex === flatIdx ? "ring-1 ring-inset ring-primary/40" : ""}`}
                onClick={() => selectDomain(domain.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    selectDomain(domain.id);
                  }
                }}
              >
                {/* Expand chevron */}
                <button
                  type="button"
                  className="shrink-0 p-0.5 hover:text-text-primary"
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

                {/* Folder icon */}
                {isExpanded ? (
                  <FolderOpen className="h-4 w-4 shrink-0 text-text-muted" />
                ) : (
                  <FolderClosed className="h-4 w-4 shrink-0 text-text-muted" />
                )}

                {/* Domain name */}
                <span className="truncate flex-1">{domain.name}</span>

                {/* Protected badge */}
                {isProtected(domain) && (
                  <Lock
                    className="h-3.5 w-3.5 shrink-0 text-status-warning"
                    aria-label={t("browser.protectedDomain")}
                  />
                )}

                {/* Context menu */}
                {can("ref.manage") && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus:opacity-100"
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
                      <DropdownMenuItem onClick={() => onRenameDomain?.(domain)}>
                        <Pencil className="mr-2 h-3.5 w-3.5" />
                        {t("browser.renameDomain")}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>

              {/* Child sets */}
              {isExpanded && (
                <fieldset className="contents">
                  {isLoadingSets && (
                    <div className="flex items-center gap-2 pl-10 pr-3 py-1.5 text-xs text-text-muted">
                      <div className="h-3 w-3 animate-spin rounded-full border border-surface-3 border-t-primary" />
                      {t("browser.loadingSets")}
                    </div>
                  )}

                  {!isLoadingSets && sets.length === 0 && (
                    <div className="pl-10 pr-3 py-1.5 text-xs text-text-muted italic">
                      {t("browser.noSets")}
                    </div>
                  )}

                  {sets.map((refSet) => {
                    const isSetSelected = selectedSetId === refSet.id;
                    const setFlatIdx = flatItems.findIndex(
                      (fi) => fi.type === "set" && fi.set.id === refSet.id,
                    );

                    return (
                      <div
                        key={refSet.id}
                        role="treeitem"
                        className={`flex items-center gap-2 pl-10 pr-3 py-1.5 cursor-pointer text-sm hover:bg-surface-1 ${
                          isSetSelected
                            ? "bg-surface-2 text-text-primary font-medium"
                            : "text-text-secondary"
                        } ${focusedIndex === setFlatIdx ? "ring-1 ring-inset ring-primary/40" : ""}`}
                        onClick={() => selectSet(refSet.id, domain.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            selectSet(refSet.id, domain.id);
                          }
                        }}
                      >
                        <span className="truncate flex-1">v{refSet.version_no}</span>
                        <Badge
                          variant={statusBadgeVariant(refSet.status)}
                          className="text-[10px] px-1.5 py-0"
                        >
                          {t(statusLabelKey(refSet.status))}
                        </Badge>
                      </div>
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
