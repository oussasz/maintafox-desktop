/**
 * OrganizationTreePanel.tsx
 *
 * Accessible treegrid for the org designer workspace. Renders the flattened
 * snapshot as depth-indented rows with capability badges, child counts,
 * and keyboard navigation. Search/filters use SmartFilterBar (client-side).
 */

import type { TFunction } from "i18next";
import { Plus } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const INDENT_PX = 20;

function capabilityBadges(node: OrgDesignerNodeRow, t: TFunction<"org">) {
  const caps: { key: string; label: string }[] = [];
  if (node.can_host_assets) caps.push({ key: "asset", label: t("capabilities.asset") });
  if (node.can_own_work) caps.push({ key: "work", label: t("capabilities.work") });
  if (node.can_carry_cost_center) caps.push({ key: "cost", label: t("capabilities.cost") });
  if (node.can_aggregate_kpis) caps.push({ key: "kpi", label: t("capabilities.kpi") });
  if (node.can_receive_permits) caps.push({ key: "permit", label: t("capabilities.permit") });
  return caps;
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

  const filteredNodes = useMemo(() => {
    if (!snapshot) return [];
    let nodes = snapshot.nodes;

    if (filterText.trim()) {
      const lower = filterText.toLowerCase();
      nodes = nodes.filter(
        (n) =>
          n.code.toLowerCase().includes(lower) ||
          n.name.toLowerCase().includes(lower) ||
          n.node_type_label.toLowerCase().includes(lower),
      );
    }

    if (statusFilter) {
      nodes = nodes.filter((n) => n.status === statusFilter);
    }

    if (typeFilter) {
      nodes = nodes.filter((n) => n.node_type_code === typeFilter);
    }

    return nodes;
  }, [snapshot, filterText, statusFilter, typeFilter]);

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

  const handleRowClick = (nodeId: number) => {
    setSelectedNodeId(nodeId === selectedNodeId ? null : nodeId);
  };

  const handleKeyDown = (e: React.KeyboardEvent, nodeId: number) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleRowClick(nodeId);
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
          resultCount={filteredNodes.length}
          onReset={onReset}
          className="border-0"
        />
        {canAddDraftNodes && !readOnly && (onAddRoot || onAddChild) && (
          <PermissionGate permission={P.ORG_MANAGE}>
            <div className="px-4 pb-2 flex flex-wrap gap-1.5">
              {onAddRoot && (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  className="h-7 gap-1 text-xs"
                  onClick={onAddRoot}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("designer.addRootNode")}
                </Button>
              )}
              {onAddChild && (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  className="h-7 gap-1 text-xs"
                  onClick={onAddChild}
                  disabled={selectedNodeId == null}
                  title={
                    selectedNodeId == null ? t("designer.addChildNodeNeedSelection") : undefined
                  }
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("designer.addChildNode")}
                </Button>
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
        role="treegrid"
        aria-label={t("designer.treeLabel")}
      >
        {filteredNodes.length === 0 ? (
          <div className="flex items-center justify-center p-8 text-text-muted text-sm">
            {filterText.trim() || statusFilter || typeFilter
              ? t("designer.noResults")
              : t("designer.emptyTree")}
          </div>
        ) : (
          filteredNodes.map((node) => {
            const isSelected = node.node_id === selectedNodeId;
            const isInactive = node.status !== "active";
            const caps = capabilityBadges(node, t);

            return (
              <div
                key={node.node_id}
                role="row"
                tabIndex={0}
                aria-selected={isSelected}
                onClick={() => handleRowClick(node.node_id)}
                onKeyDown={(e) => handleKeyDown(e, node.node_id)}
                className={cn(
                  "flex items-center gap-2 px-3 py-2 cursor-pointer border-b border-surface-border transition-colors",
                  "hover:bg-surface-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset",
                  isSelected && "bg-primary/10 border-l-2 border-l-primary",
                  isInactive && "opacity-60",
                )}
                style={{ paddingLeft: `${node.depth * INDENT_PX + 12}px` }}
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs text-text-muted">{node.code}</span>
                    <span className="text-sm font-medium text-text-primary truncate">
                      {node.name}
                    </span>
                    {isInactive && (
                      <Badge variant="outline" className="text-[10px] px-1.5">
                        {node.status}
                      </Badge>
                    )}
                  </div>
                  <div className="flex items-center gap-1.5 mt-0.5">
                    <span className="text-xs text-text-muted">{node.node_type_label}</span>
                    {caps.map((c) => (
                      <Badge key={c.key} variant="secondary" className="text-[10px] px-1.5 py-0">
                        {c.label}
                      </Badge>
                    ))}
                  </div>
                </div>

                <div className="flex items-center gap-3 shrink-0 text-xs text-text-muted">
                  {node.child_count > 0 && (
                    <span title={t("designer.childCount")}>
                      {node.child_count} {t("designer.children")}
                    </span>
                  )}
                  {node.active_responsibility_count > 0 && (
                    <span title={t("designer.responsibilityCount")}>
                      {node.active_responsibility_count} {t("designer.resp")}
                    </span>
                  )}
                  {node.active_binding_count > 0 && (
                    <span title={t("designer.bindingCount")}>
                      {node.active_binding_count} {t("designer.bindings")}
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
