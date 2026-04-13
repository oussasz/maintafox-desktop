/**
 * OrganizationTreePanel.tsx
 *
 * Accessible treegrid for the org designer workspace. Renders the flattened
 * snapshot as depth-indented rows with capability badges, child counts,
 * and keyboard navigation.
 */

import type { TFunction } from "i18next";
import { Search } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";

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

export function OrganizationTreePanel() {
  const { t } = useTranslation("org");
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const filterText = useOrgDesignerStore((s) => s.filterText);
  const statusFilter = useOrgDesignerStore((s) => s.statusFilter);
  const typeFilter = useOrgDesignerStore((s) => s.typeFilter);
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const setFilterText = useOrgDesignerStore((s) => s.setFilterText);
  const setSelectedNodeId = useOrgDesignerStore((s) => s.setSelectedNodeId);

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
    <div className="flex h-full flex-col">
      {/* Search bar */}
      <div className="relative p-3 border-b border-surface-border">
        <Search className="absolute left-5 top-1/2 -translate-y-1/2 h-4 w-4 text-text-muted" />
        <Input
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          placeholder={t("designer.searchPlaceholder")}
          className="pl-8"
        />
      </div>

      {/* Tree rows */}
      <div className="flex-1 overflow-y-auto" role="treegrid" aria-label={t("designer.treeLabel")}>
        {filteredNodes.length === 0 ? (
          <div className="flex items-center justify-center p-8 text-text-muted text-sm">
            {filterText.trim() ? t("designer.noResults") : t("designer.emptyTree")}
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
                {/* Node identity */}
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

                {/* Counters */}
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
