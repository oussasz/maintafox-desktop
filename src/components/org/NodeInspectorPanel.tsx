/**
 * NodeInspectorPanel.tsx
 *
 * Right-panel inspector for the selected org node. Shows details,
 * responsibilities, external bindings, and preview action triggers
 * organized in tabs.
 */

import { ArrowRightLeft, Info, Link2, Package, ShieldAlert, Users, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import {
  assignEquipmentToNode,
  listOrgNodeEquipment,
  searchUnassignedEquipment,
  unassignEquipmentFromNode,
  updateOrgNodeMetadata,
  VersionConflictError,
} from "@/services/org-node-service";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgNodeStore } from "@/stores/org-node-store";
import { toErrorMessage } from "@/utils/errors";
import type { OrgDesignerNodeRow, OrgNodeEquipmentRow } from "@shared/ipc-types";

function DetailRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <span className="text-xs text-text-muted w-32 shrink-0">{label}</span>
      <span className="text-sm text-text-primary">{value ?? "—"}</span>
    </div>
  );
}

interface NodeInspectorPanelProps {
  readOnly?: boolean;
}

export function NodeInspectorPanel({ readOnly = false }: NodeInspectorPanelProps) {
  const { t } = useTranslation("org");
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const openPreview = useOrgDesignerStore((s) => s.openPreview);

  const responsibilities = useOrgNodeStore((s) => s.responsibilities);
  const bindings = useOrgNodeStore((s) => s.bindings);
  const selectNode = useOrgNodeStore((s) => s.selectNode);
  const selectedNode = useOrgNodeStore((s) => s.selectedNode);
  const nodeContextLoading = useOrgNodeStore((s) => s.loading);
  const refreshSelectedNodeContext = useOrgNodeStore((s) => s.refreshSelectedNodeContext);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  const selectedRow: OrgDesignerNodeRow | null = useMemo(() => {
    if (!snapshot || selectedNodeId === null) return null;
    return snapshot.nodes.find((n) => n.node_id === selectedNodeId) ?? null;
  }, [snapshot, selectedNodeId]);

  useEffect(() => {
    void selectNode(selectedNodeId);
  }, [selectedNodeId, selectNode]);

  // Equipment state (GAP ORG-02)
  const [equipment, setEquipment] = useState<OrgNodeEquipmentRow[]>([]);
  const [equipSearch, setEquipSearch] = useState("");
  const [equipResults, setEquipResults] = useState<OrgNodeEquipmentRow[]>([]);
  const [equipSearching, setEquipSearching] = useState(false);

  const [metaName, setMetaName] = useState("");
  const [metaDescription, setMetaDescription] = useState("");
  const [metaCostCenter, setMetaCostCenter] = useState("");
  const [metaExternalRef, setMetaExternalRef] = useState("");
  const [metaErpRef, setMetaErpRef] = useState("");
  const [metaNotes, setMetaNotes] = useState("");
  const [metaSaving, setMetaSaving] = useState(false);
  const [metaError, setMetaError] = useState<string | null>(null);

  const hasAssetCapability = selectedRow?.can_host_assets ?? false;

  const loadEquipment = useCallback(async () => {
    if (!selectedNodeId || !hasAssetCapability) return;
    try {
      const items = await listOrgNodeEquipment(selectedNodeId);
      setEquipment(items);
    } catch {
      /* ignore */
    }
  }, [selectedNodeId, hasAssetCapability]);

  useEffect(() => {
    void loadEquipment();
  }, [loadEquipment]);

  const handleEquipSearch = useCallback(async (query: string) => {
    setEquipSearch(query);
    if (query.length < 2) {
      setEquipResults([]);
      return;
    }
    setEquipSearching(true);
    try {
      const results = await searchUnassignedEquipment(query, 10);
      setEquipResults(results);
    } finally {
      setEquipSearching(false);
    }
  }, []);

  const handleAssign = useCallback(
    async (equipmentId: number) => {
      if (!selectedNodeId) return;
      await assignEquipmentToNode({ equipment_id: equipmentId, node_id: selectedNodeId });
      setEquipSearch("");
      setEquipResults([]);
      await loadEquipment();
    },
    [selectedNodeId, loadEquipment],
  );

  const handleUnassign = useCallback(
    async (equipmentId: number) => {
      await unassignEquipmentFromNode(equipmentId);
      await loadEquipment();
    },
    [loadEquipment],
  );

  useEffect(() => {
    if (selectedNodeId == null || !selectedNode) return;
    if (selectedNode.id !== selectedNodeId) return;
    setMetaName(selectedNode.name);
    setMetaDescription(selectedNode.description ?? "");
    setMetaCostCenter(selectedNode.cost_center_code ?? "");
    setMetaExternalRef(selectedNode.external_reference ?? "");
    setMetaErpRef(selectedNode.erp_reference ?? "");
    setMetaNotes(selectedNode.notes ?? "");
    setMetaError(null);
  }, [selectedNodeId, selectedNode]);

  const isMetaDirty = useMemo(() => {
    if (!selectedNode) return false;
    const costOk =
      !selectedRow || !selectedRow.can_carry_cost_center
        ? true
        : (metaCostCenter || "") === (selectedNode.cost_center_code ?? "");
    return (
      metaName !== selectedNode.name ||
      (metaDescription || "") !== (selectedNode.description ?? "") ||
      !costOk ||
      (metaExternalRef || "") !== (selectedNode.external_reference ?? "") ||
      (metaErpRef || "") !== (selectedNode.erp_reference ?? "") ||
      (metaNotes || "") !== (selectedNode.notes ?? "")
    );
  }, [
    selectedNode,
    selectedRow,
    metaName,
    metaDescription,
    metaCostCenter,
    metaExternalRef,
    metaErpRef,
    metaNotes,
  ]);

  const handleMetadataSave = useCallback(async () => {
    if (!selectedNode || !selectedRow) return;
    setMetaSaving(true);
    setMetaError(null);
    try {
      const base = {
        node_id: selectedNode.id,
        name: metaName.trim(),
        description: metaDescription.trim() || null,
        external_reference: metaExternalRef.trim() || null,
        erp_reference: metaErpRef.trim() || null,
        notes: metaNotes.trim() || null,
        expected_row_version: selectedNode.row_version,
      };
      await updateOrgNodeMetadata(
        selectedRow.can_carry_cost_center
          ? { ...base, cost_center_code: metaCostCenter.trim() || null }
          : base,
      );
      await loadSnapshot();
      await refreshSelectedNodeContext();
    } catch (e) {
      if (e instanceof VersionConflictError) {
        setMetaError(t("designer.metadata.versionConflict"));
        void loadSnapshot();
        void refreshSelectedNodeContext();
      } else {
        setMetaError(toErrorMessage(e));
      }
    } finally {
      setMetaSaving(false);
    }
  }, [
    selectedNode,
    selectedRow,
    metaName,
    metaDescription,
    metaCostCenter,
    metaExternalRef,
    metaErpRef,
    metaNotes,
    loadSnapshot,
    refreshSelectedNodeContext,
    t,
  ]);

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
    <div className="flex min-h-0 flex-1 flex-col" data-readonly={readOnly ? "true" : undefined}>
      {readOnly && (
        <div className="px-4 pt-3 pb-0 text-xs text-text-muted border-b border-surface-border/60">
          {t("designer.readOnlyInspectorBanner")}
        </div>
      )}
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
      <Tabs defaultValue="details" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <TabsList className="mx-4 mt-2 shrink-0">
          <TabsTrigger value="details">{t("designer.inspector.details")}</TabsTrigger>
          {hasAssetCapability && (
            <TabsTrigger value="equipment">
              {t("designer.inspector.equipment")}
              {equipment.length > 0 && (
                <Badge variant="secondary" className="ml-1 text-[10px] h-4 px-1">
                  {equipment.length}
                </Badge>
              )}
            </TabsTrigger>
          )}
          <TabsTrigger value="responsibilities">
            {t("designer.inspector.responsibilities")}
          </TabsTrigger>
          <TabsTrigger value="bindings">{t("designer.inspector.bindings")}</TabsTrigger>
          <TabsTrigger value="actions">{t("designer.inspector.actions")}</TabsTrigger>
        </TabsList>

        {/* Details tab */}
        <TabsContent
          value="details"
          className="flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          {!readOnly && (
            <PermissionGate permission="org.manage">
              <div className="mb-4 space-y-2 rounded-lg border border-surface-border p-3">
                <p className="text-xs font-medium text-text-muted">
                  {t("designer.metadata.title")}
                </p>
                {nodeContextLoading && !selectedNode ? (
                  <p className="text-xs text-text-muted">{t("designer.metadata.loading")}</p>
                ) : (
                  selectedNode && (
                    <>
                      <div className="space-y-1.5">
                        <Label className="text-xs">{t("designer.metadata.name")}</Label>
                        <Input
                          value={metaName}
                          onChange={(e) => setMetaName(e.target.value)}
                          className="h-8 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label className="text-xs text-text-muted">
                          {t("designer.metadata.description")}
                        </Label>
                        <Textarea
                          value={metaDescription}
                          onChange={(e) => setMetaDescription(e.target.value)}
                          className="min-h-[64px] text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      {selectedRow.can_carry_cost_center && (
                        <div className="space-y-1.5">
                          <Label className="text-xs text-text-muted">
                            {t("designer.metadata.costCenter")}
                          </Label>
                          <Input
                            value={metaCostCenter}
                            onChange={(e) => setMetaCostCenter(e.target.value)}
                            className="h-8 text-sm font-mono"
                            disabled={metaSaving}
                          />
                        </div>
                      )}
                      <div className="space-y-1.5">
                        <Label className="text-xs text-text-muted">
                          {t("designer.metadata.externalRef")}
                        </Label>
                        <Input
                          value={metaExternalRef}
                          onChange={(e) => setMetaExternalRef(e.target.value)}
                          className="h-8 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label className="text-xs text-text-muted">
                          {t("designer.metadata.erpRef")}
                        </Label>
                        <Input
                          value={metaErpRef}
                          onChange={(e) => setMetaErpRef(e.target.value)}
                          className="h-8 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label className="text-xs text-text-muted">
                          {t("designer.metadata.notes")}
                        </Label>
                        <Textarea
                          value={metaNotes}
                          onChange={(e) => setMetaNotes(e.target.value)}
                          className="min-h-[48px] text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      {metaError && (
                        <p className="text-xs text-status-danger" role="alert">
                          {metaError}
                        </p>
                      )}
                      <Button
                        type="button"
                        size="sm"
                        onClick={() => void handleMetadataSave()}
                        disabled={!isMetaDirty || !selectedNode || metaSaving}
                        className="h-8"
                      >
                        {metaSaving ? t("designer.metadata.saving") : t("designer.metadata.save")}
                      </Button>
                    </>
                  )
                )}
              </div>
            </PermissionGate>
          )}

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

        {/* Equipment tab (GAP ORG-02) */}
        {hasAssetCapability && (
          <TabsContent
            value="equipment"
            className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
          >
            {equipment.length === 0 && !equipSearch ? (
              <div className="flex flex-col items-center justify-center py-8 text-text-muted text-sm">
                <Package className="h-6 w-6 mb-2 opacity-40" />
                <p>{t("designer.inspector.noEquipment")}</p>
              </div>
            ) : (
              <div className="space-y-2">
                {equipment.map((eq) => (
                  <div
                    key={eq.id}
                    className="rounded-lg border border-surface-border p-3 text-sm flex items-center justify-between"
                  >
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-text-muted">
                          {eq.asset_id_code}
                        </span>
                        <span className="font-medium text-text-primary">{eq.name}</span>
                      </div>
                      <Badge variant="outline" className="text-[10px] mt-1">
                        {eq.lifecycle_status}
                      </Badge>
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 w-7 p-0"
                      onClick={() => void handleUnassign(eq.id)}
                      disabled={readOnly}
                    >
                      <X className="h-3.5 w-3.5 text-status-danger" />
                    </Button>
                  </div>
                ))}
              </div>
            )}

            {/* Assign search */}
            <div className="mt-4 space-y-2">
              <Input
                value={equipSearch}
                onChange={(e) => void handleEquipSearch(e.target.value)}
                placeholder={t("designer.inspector.equipSearchPlaceholder")}
                className="h-8 text-xs"
                disabled={readOnly}
              />
              {equipSearching && (
                <p className="text-xs text-text-muted">{t("designer.inspector.searching")}</p>
              )}
              {equipResults.length > 0 && (
                <div className="space-y-1 max-h-48 overflow-y-auto border border-surface-border rounded-md p-2">
                  {equipResults.map((eq) => (
                    <button
                      key={eq.id}
                      type="button"
                      className="w-full text-left rounded-md p-2 text-sm hover:bg-surface-2 cursor-pointer disabled:opacity-50"
                      onClick={() => void handleAssign(eq.id)}
                      disabled={readOnly}
                    >
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-text-muted">
                          {eq.asset_id_code}
                        </span>
                        <span>{eq.name}</span>
                      </div>
                      {eq.current_node_name && (
                        <p className="text-[11px] text-status-warning mt-0.5">
                          {t("designer.inspector.equipCurrentlyAssigned", {
                            node: eq.current_node_name,
                          })}
                        </p>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </TabsContent>
        )}

        {/* Responsibilities tab */}
        <TabsContent
          value="responsibilities"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
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
        <TabsContent
          value="bindings"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
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
        <TabsContent
          value="actions"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          <div className="space-y-3">
            <p className="text-xs text-text-muted">{t("designer.inspector.actionsDescription")}</p>

            <Button
              variant="outline"
              className="w-full justify-start gap-2"
              onClick={() => openPreview({ action: "MoveNode", node_id: selectedRow.node_id })}
              disabled={readOnly}
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
              disabled={readOnly}
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
              disabled={readOnly}
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
