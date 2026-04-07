/**
 * NodeInspectorPanel.tsx
 *
 * Right-panel inspector for the selected org node. Shows details,
 * responsibilities, external bindings, and preview action triggers
 * organized in tabs.
 */

import { ArrowRightLeft, Info, Link2, ShieldAlert, Users } from "lucide-react";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgNodeStore } from "@/stores/org-node-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";

function DetailRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <span className="text-xs text-text-muted w-32 shrink-0">{label}</span>
      <span className="text-sm text-text-primary">{value ?? "—"}</span>
    </div>
  );
}

export function NodeInspectorPanel() {
  const { t } = useTranslation("org");
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const openPreview = useOrgDesignerStore((s) => s.openPreview);

  const responsibilities = useOrgNodeStore((s) => s.responsibilities);
  const bindings = useOrgNodeStore((s) => s.bindings);
  const selectNode = useOrgNodeStore((s) => s.selectNode);

  const selectedRow: OrgDesignerNodeRow | null = useMemo(() => {
    if (!snapshot || selectedNodeId === null) return null;
    return snapshot.nodes.find((n) => n.node_id === selectedNodeId) ?? null;
  }, [snapshot, selectedNodeId]);

  useEffect(() => {
    void selectNode(selectedNodeId);
  }, [selectedNodeId, selectNode]);

  if (!selectedRow) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-text-muted text-sm">
        <div className="text-center space-y-2">
          <Info className="h-8 w-8 mx-auto opacity-40" />
          <p>{t("designer.inspector.noSelection")}</p>
        </div>
      </div>
    );
  }

  const capabilities: string[] = [];
  if (selectedRow.can_host_assets) capabilities.push(t("capabilities.asset"));
  if (selectedRow.can_own_work) capabilities.push(t("capabilities.work"));
  if (selectedRow.can_carry_cost_center) capabilities.push(t("capabilities.cost"));
  if (selectedRow.can_aggregate_kpis) capabilities.push(t("capabilities.kpi"));
  if (selectedRow.can_receive_permits) capabilities.push(t("capabilities.permit"));

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="p-4 border-b border-surface-border">
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs text-text-muted">{selectedRow.code}</span>
          <Badge
            variant={selectedRow.status === "active" ? "default" : "outline"}
            className="text-[10px]"
          >
            {selectedRow.status}
          </Badge>
        </div>
        <h3 className="text-lg font-semibold text-text-primary mt-1">{selectedRow.name}</h3>
        <p className="text-xs text-text-muted mt-0.5">{selectedRow.node_type_label}</p>
      </div>

      {/* Tabs */}
      <Tabs defaultValue="details" className="flex-1 flex flex-col">
        <TabsList className="mx-4 mt-2">
          <TabsTrigger value="details">{t("designer.inspector.details")}</TabsTrigger>
          <TabsTrigger value="responsibilities">
            {t("designer.inspector.responsibilities")}
          </TabsTrigger>
          <TabsTrigger value="bindings">{t("designer.inspector.bindings")}</TabsTrigger>
          <TabsTrigger value="actions">{t("designer.inspector.actions")}</TabsTrigger>
        </TabsList>

        {/* Details tab */}
        <TabsContent value="details" className="flex-1 overflow-y-auto p-4 space-y-2">
          <DetailRow label={t("designer.inspector.nodeId")} value={selectedRow.node_id} />
          <DetailRow label={t("designer.inspector.type")} value={selectedRow.node_type_label} />
          <DetailRow label={t("designer.inspector.typeCode")} value={selectedRow.node_type_code} />
          <DetailRow label={t("designer.inspector.depth")} value={selectedRow.depth} />
          <DetailRow
            label={t("designer.inspector.ancestorPath")}
            value={<span className="font-mono text-xs break-all">{selectedRow.ancestor_path}</span>}
          />
          <DetailRow label={t("designer.inspector.childCount")} value={selectedRow.child_count} />
          <DetailRow label={t("designer.inspector.rowVersion")} value={selectedRow.row_version} />
          <Separator className="my-2" />
          <div className="space-y-1">
            <span className="text-xs text-text-muted font-medium">
              {t("designer.inspector.capabilitiesLabel")}
            </span>
            <div className="flex flex-wrap gap-1.5">
              {capabilities.length > 0 ? (
                capabilities.map((c) => (
                  <Badge key={c} variant="secondary" className="text-[10px]">
                    {c}
                  </Badge>
                ))
              ) : (
                <span className="text-xs text-text-muted">{t("designer.inspector.noCaps")}</span>
              )}
            </div>
          </div>
        </TabsContent>

        {/* Responsibilities tab */}
        <TabsContent value="responsibilities" className="flex-1 overflow-y-auto p-4">
          {responsibilities.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-8 text-text-muted text-sm">
              <Users className="h-6 w-6 mb-2 opacity-40" />
              <p>{t("designer.inspector.noResponsibilities")}</p>
            </div>
          ) : (
            <div className="space-y-2">
              {responsibilities.map((r) => (
                <div
                  key={r.id}
                  className={cn(
                    "rounded-lg border border-surface-border p-3 text-sm",
                    r.valid_to && "opacity-60",
                  )}
                >
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-text-primary">{r.responsibility_type}</span>
                    {r.valid_to && (
                      <Badge variant="outline" className="text-[10px]">
                        {t("designer.inspector.ended")}
                      </Badge>
                    )}
                  </div>
                  <div className="text-xs text-text-muted mt-1">
                    {r.person_id && `${t("designer.inspector.person")}: #${r.person_id}`}
                    {r.team_id && `${t("designer.inspector.team")}: #${r.team_id}`}
                  </div>
                </div>
              ))}
            </div>
          )}
        </TabsContent>

        {/* External Bindings tab */}
        <TabsContent value="bindings" className="flex-1 overflow-y-auto p-4">
          {bindings.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-8 text-text-muted text-sm">
              <Link2 className="h-6 w-6 mb-2 opacity-40" />
              <p>{t("designer.inspector.noBindings")}</p>
            </div>
          ) : (
            <div className="space-y-2">
              {bindings.map((b) => (
                <div key={b.id} className="rounded-lg border border-surface-border p-3 text-sm">
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-text-primary">{b.binding_type}</span>
                    {b.is_primary && (
                      <Badge variant="default" className="text-[10px]">
                        {t("designer.inspector.primary")}
                      </Badge>
                    )}
                  </div>
                  <div className="text-xs text-text-muted mt-1">
                    {b.external_system} → {b.external_id}
                  </div>
                </div>
              ))}
            </div>
          )}
        </TabsContent>

        {/* Preview Actions tab */}
        <TabsContent value="actions" className="flex-1 overflow-y-auto p-4">
          <div className="space-y-3">
            <p className="text-xs text-text-muted">{t("designer.inspector.actionsDescription")}</p>

            <Button
              variant="outline"
              className="w-full justify-start gap-2"
              onClick={() => openPreview({ action: "MoveNode", node_id: selectedRow.node_id })}
            >
              <ArrowRightLeft className="h-4 w-4" />
              {t("designer.actions.moveNode")}
            </Button>

            <Button
              variant="outline"
              className="w-full justify-start gap-2 text-status-danger hover:text-status-danger"
              onClick={() =>
                openPreview({ action: "DeactivateNode", node_id: selectedRow.node_id })
              }
            >
              <ShieldAlert className="h-4 w-4" />
              {t("designer.actions.deactivateNode")}
            </Button>

            <Button
              variant="outline"
              className="w-full justify-start gap-2"
              onClick={() =>
                openPreview({
                  action: "ReassignResponsibility",
                  node_id: selectedRow.node_id,
                })
              }
            >
              <Users className="h-4 w-4" />
              {t("designer.actions.reassignResponsibility")}
            </Button>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}
