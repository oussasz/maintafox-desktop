/**
 * OrganizationTreePanel.tsx
 *
 * Accessible tree for the org designer workspace (WAI-ARIA tree pattern). Renders the
 * snapshot as depth-indented units with capability badges and counts.
 *
 * Keyboard: one Tab stop (roving focus); ↑/↓ move, Home/End jump, → expands or enters the
 * first sub-unit, ← collapses or goes to the parent, Enter/Space selects.
 * Search/filters use SmartFilterBar (client-side); while filtering, matches are listed flat.
 */

import type { TFunction } from "i18next";
import { ChevronDown, ChevronRight, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { orgNodeStatusLabel } from "@/components/org/org-display";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import { ORG_NODE_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Horizontal indent per tree level, in px. */
const INDENT_PX = 20;
/** Left padding of a top-level row, in px. */
const ROW_BASE_PADDING_PX = 12;

function capabilityBadges(node: OrgDesignerNodeRow, t: TFunction<"org">) {
  const caps: { key: string; label: string; title: string }[] = [];
  if (node.can_host_assets)
    caps.push({
      key: "asset",
      label: t("capabilities.asset"),
      title: t("capabilities.long.asset"),
    });
  if (node.can_own_work)
    caps.push({ key: "work", label: t("capabilities.work"), title: t("capabilities.long.work") });
  if (node.can_carry_cost_center)
    caps.push({ key: "cost", label: t("capabilities.cost"), title: t("capabilities.long.cost") });
  if (node.can_aggregate_kpis)
    caps.push({ key: "kpi", label: t("capabilities.kpi"), title: t("capabilities.long.kpi") });
  if (node.can_receive_permits)
    caps.push({
      key: "permit",
      label: t("capabilities.permit"),
      title: t("capabilities.long.permit"),
    });
  return caps;
}

/** Ancestor ids of a row from its materialized path ("/1/2/5/" → [1, 2]). */
function ancestorIds(node: OrgDesignerNodeRow): number[] {
  return node.ancestor_path
    .split("/")
    .filter(Boolean)
    .map(Number)
    .filter((id) => Number.isFinite(id) && id !== node.node_id);
}

interface OrganizationTreePanelProps {
  /** When true, the tree is for browsing only (no structural editing in the inspector). */
  readOnly?: boolean;
  /** When true, user is in draft design mode and may add nodes to the draft tree. */
  canAddDraftNodes?: boolean;
  onAddRoot?: () => void;
  onAddChild?: () => void;
  /** In draft design mode, explain why add-node is unavailable when there is no active (published) model. */
  showNoActiveModelHint?: boolean;
}

export function OrganizationTreePanel({
  readOnly = false,
  canAddDraftNodes = false,
  onAddRoot,
  onAddChild,
  showNoActiveModelHint = false,
}: OrganizationTreePanelProps) {
  const { t } = useTranslation("org");
  const { formatNumber } = useFormatters();
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const filterText = useOrgDesignerStore((s) => s.filterText);
  const statusFilter = useOrgDesignerStore((s) => s.statusFilter);
  const typeFilter = useOrgDesignerStore((s) => s.typeFilter);
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const setFilterText = useOrgDesignerStore((s) => s.setFilterText);
  const setStatusFilter = useOrgDesignerStore((s) => s.setStatusFilter);
  const setTypeFilter = useOrgDesignerStore((s) => s.setTypeFilter);
  const setSelectedNodeId = useOrgDesignerStore((s) => s.setSelectedNodeId);

  const [searchInput, setSearchInput] = useState(filterText);
  const [collapsed, setCollapsed] = useState<Set<number>>(() => new Set());
  const [focusedId, setFocusedId] = useState<number | null>(null);
  const rowRefs = useRef(new Map<number, HTMLDivElement>());

  const nodeTypeOptions = useMemo(() => {
    if (!snapshot) return [];
    const seen = new Map<string, string>();
    for (const node of snapshot.nodes) {
      if (!seen.has(node.node_type_code)) {
        seen.set(node.node_type_code, node.node_type_label);
      }
    }
    return Array.from(seen.entries()).map(([value, label]) => ({ value, label }));
  }, [snapshot]);

  const isFiltering = Boolean(filterText.trim() || statusFilter || typeFilter);

  const visibleNodes = useMemo(() => {
    if (!snapshot) return [];
    let nodes = snapshot.nodes;

    if (isFiltering) {
      if (filterText.trim()) {
        const lower = filterText.toLowerCase();
        nodes = nodes.filter(
          (n) =>
            n.code.toLowerCase().includes(lower) ||
            n.name.toLowerCase().includes(lower) ||
            n.node_type_label.toLowerCase().includes(lower),
        );
      }
      if (statusFilter) nodes = nodes.filter((n) => n.status === statusFilter);
      if (typeFilter) nodes = nodes.filter((n) => n.node_type_code === typeFilter);
      return nodes;
    }

    if (collapsed.size === 0) return nodes;
    return nodes.filter((n) => !ancestorIds(n).some((id) => collapsed.has(id)));
  }, [snapshot, filterText, statusFilter, typeFilter, isFiltering, collapsed]);

  const onSearchChange = useCallback(
    (value: string) => {
      setFilterText(value);
    },
    [setFilterText],
  );

  const onReset = useCallback(() => {
    setSearchInput("");
    setFilterText("");
    setStatusFilter(null);
    setTypeFilter(null);
  }, [setFilterText, setStatusFilter, setTypeFilter]);

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "status",
        kind: "select",
        label: t("designer.statusFilter"),
        options: [
          { value: "active", label: t("designer.statusActive") },
          { value: "inactive", label: t("designer.statusInactive") },
          { value: "draft", label: t("designer.statusDraft") },
        ],
        value: statusFilter,
        onChange: setStatusFilter,
        allLabel: t("designer.allStatuses"),
      },
      {
        id: "type",
        kind: "select",
        label: t("designer.typeFilter"),
        options: nodeTypeOptions,
        value: typeFilter,
        onChange: setTypeFilter,
        allLabel: t("designer.allTypes"),
      },
    ],
    [t, statusFilter, typeFilter, nodeTypeOptions, setStatusFilter, setTypeFilter],
  );

  // Keep the roving focus on a visible row.
  const tabStopId =
    focusedId != null && visibleNodes.some((n) => n.node_id === focusedId)
      ? focusedId
      : selectedNodeId != null && visibleNodes.some((n) => n.node_id === selectedNodeId)
        ? selectedNodeId
        : (visibleNodes[0]?.node_id ?? null);

  const focusRow = useCallback((nodeId: number) => {
    setFocusedId(nodeId);
    rowRefs.current.get(nodeId)?.focus();
  }, []);

  // A collapsed branch hides the selected unit: keep focus reachable.
  useEffect(() => {
    if (focusedId != null && !visibleNodes.some((n) => n.node_id === focusedId)) {
      setFocusedId(null);
    }
  }, [visibleNodes, focusedId]);

  const toggleCollapsed = useCallback((nodeId: number) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  const handleRowClick = (nodeId: number) => {
    setFocusedId(nodeId);
    setSelectedNodeId(nodeId === selectedNodeId ? null : nodeId);
  };

  const handleKeyDown = (e: React.KeyboardEvent, node: OrgDesignerNodeRow) => {
    const index = visibleNodes.findIndex((n) => n.node_id === node.node_id);
    const hasChildren = node.child_count > 0 && !isFiltering;
    const isCollapsed = collapsed.has(node.node_id);
    switch (e.key) {
      case "ArrowDown": {
        e.preventDefault();
        const next = visibleNodes[index + 1];
        if (next) focusRow(next.node_id);
        break;
      }
      case "ArrowUp": {
        e.preventDefault();
        const prev = visibleNodes[index - 1];
        if (prev) focusRow(prev.node_id);
        break;
      }
      case "Home": {
        e.preventDefault();
        const first = visibleNodes[0];
        if (first) focusRow(first.node_id);
        break;
      }
      case "End": {
        e.preventDefault();
        const last = visibleNodes[visibleNodes.length - 1];
        if (last) focusRow(last.node_id);
        break;
      }
      case "ArrowRight": {
        e.preventDefault();
        if (!hasChildren) break;
        if (isCollapsed) {
          toggleCollapsed(node.node_id);
        } else {
          const firstChild = visibleNodes[index + 1];
          if (firstChild && firstChild.parent_id === node.node_id) focusRow(firstChild.node_id);
        }
        break;
      }
      case "ArrowLeft": {
        e.preventDefault();
        if (hasChildren && !isCollapsed) {
          toggleCollapsed(node.node_id);
        } else if (
          node.parent_id != null &&
          visibleNodes.some((n) => n.node_id === node.parent_id)
        ) {
          focusRow(node.parent_id);
        }
        break;
      }
      case "Enter":
      case " ": {
        e.preventDefault();
        setFocusedId(node.node_id);
        setSelectedNodeId(node.node_id);
        break;
      }
      default:
        break;
    }
  };

  return (
    <div
      className="flex h-full flex-col"
      data-readonly={readOnly ? "true" : undefined}
      aria-readonly={readOnly || undefined}
    >
      <div className="border-b border-surface-border shrink-0">
        <SmartFilterBar
          searchPlaceholder={t("designer.searchPlaceholder")}
          searchValue={searchInput}
          onSearchInputChange={setSearchInput}
          onSearchChange={onSearchChange}
          filters={filterDefs}
          resultCount={visibleNodes.length}
          onReset={onReset}
          className="border-0"
        />
        {canAddDraftNodes && !readOnly && (onAddRoot || onAddChild) && (
          <PermissionGate permission={P.ORG_MANAGE}>
            <div className="px-4 pb-2 flex flex-wrap items-center gap-1.5">
              {onAddRoot && (
                <Button
                  type="button"
                  variant="secondary"
                  className="h-9 gap-1 text-xs"
                  onClick={onAddRoot}
                >
                  <Plus className="h-3.5 w-3.5" aria-hidden />
                  {t("designer.addRootNode")}
                </Button>
              )}
              {onAddChild && (
                <Button
                  type="button"
                  variant="secondary"
                  className="h-9 gap-1 text-xs"
                  onClick={onAddChild}
                  disabled={selectedNodeId == null}
                >
                  <Plus className="h-3.5 w-3.5" aria-hidden />
                  {t("designer.addChildNode")}
                </Button>
              )}
              {onAddChild && selectedNodeId == null && (
                <span className="text-xs text-text-muted">
                  {t("designer.addChildNodeNeedSelection")}
                </span>
              )}
            </div>
          </PermissionGate>
        )}
        {showNoActiveModelHint && !readOnly && (
          <p className="text-xs text-text-muted mx-4 mb-2 border-t border-surface-border/60 pt-2">
            {t("designer.noActiveModelForNodes")}
          </p>
        )}
      </div>

      <div
        className="min-h-0 flex-1 overflow-y-auto"
        role="tree"
        aria-label={t("designer.treeLabel")}
      >
        {visibleNodes.length === 0 ? (
          <div className="flex items-center justify-center p-8 text-text-muted text-sm">
            {isFiltering ? t("designer.noResults") : t("designer.emptyTree")}
          </div>
        ) : (
          visibleNodes.map((node) => {
            const isSelected = node.node_id === selectedNodeId;
            const isInactive = node.status !== "active";
            const caps = capabilityBadges(node, t);
            const hasChildren = node.child_count > 0 && !isFiltering;
            const isCollapsed = collapsed.has(node.node_id);

            return (
              <div
                key={node.node_id}
                ref={(el) => {
                  if (el) rowRefs.current.set(node.node_id, el);
                  else rowRefs.current.delete(node.node_id);
                }}
                role="treeitem"
                tabIndex={node.node_id === tabStopId ? 0 : -1}
                aria-selected={isSelected}
                aria-level={node.depth + 1}
                aria-expanded={hasChildren ? !isCollapsed : undefined}
                onClick={() => handleRowClick(node.node_id)}
                onKeyDown={(e) => handleKeyDown(e, node)}
                onFocus={() => setFocusedId(node.node_id)}
                className={cn(
                  "flex min-h-11 items-center gap-2 px-3 py-2 cursor-pointer border-b border-surface-border transition-colors",
                  "hover:bg-surface-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset",
                  isSelected && "bg-primary/10 border-l-2 border-l-primary",
                )}
                style={{
                  paddingLeft: `${(isFiltering ? 0 : node.depth) * INDENT_PX + ROW_BASE_PADDING_PX}px`,
                }}
              >
                {hasChildren ? (
                  <button
                    type="button"
                    tabIndex={-1}
                    className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded text-text-muted hover:bg-surface-2"
                    aria-label={
                      isCollapsed
                        ? t("designer.expandUnit", { name: node.name })
                        : t("designer.collapseUnit", { name: node.name })
                    }
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleCollapsed(node.node_id);
                    }}
                  >
                    {isCollapsed ? (
                      <ChevronRight className="h-4 w-4" aria-hidden />
                    ) : (
                      <ChevronDown className="h-4 w-4" aria-hidden />
                    )}
                  </button>
                ) : (
                  <span className="h-8 w-8 shrink-0" aria-hidden />
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs text-text-muted">{node.code}</span>
                    <span
                      className={cn(
                        "text-sm font-medium truncate",
                        isInactive ? "text-text-secondary" : "text-text-primary",
                      )}
                    >
                      {node.name}
                    </span>
                    {isInactive && (
                      <StatusChip
                        label={orgNodeStatusLabel(t, node.status)}
                        tone={toneOf(ORG_NODE_STATUS_TONE, node.status)}
                      />
                    )}
                  </div>
                  <div className="flex flex-wrap items-center gap-1.5 mt-0.5">
                    <span className="text-xs text-text-muted">{node.node_type_label}</span>
                    {caps.map((c) => (
                      <Badge
                        key={c.key}
                        variant="secondary"
                        className="text-[10px] px-1.5 py-0"
                        title={c.title}
                      >
                        {c.label}
                      </Badge>
                    ))}
                  </div>
                </div>

                <div className="flex items-center gap-3 shrink-0 text-xs text-text-muted">
                  {node.child_count > 0 && (
                    <span title={t("designer.childCount")}>
                      {formatNumber(node.child_count)} {t("designer.children")}
                    </span>
                  )}
                  {node.active_responsibility_count > 0 && (
                    <span title={t("designer.responsibilityCount")}>
                      {formatNumber(node.active_responsibility_count)} {t("designer.resp")}
                    </span>
                  )}
                  {node.active_binding_count > 0 && (
                    <span title={t("designer.bindingCount")}>
                      {formatNumber(node.active_binding_count)} {t("designer.bindings")}
                    </span>
                  )}
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
