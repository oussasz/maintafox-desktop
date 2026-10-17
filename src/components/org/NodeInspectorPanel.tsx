/**
 * NodeInspectorPanel.tsx
 *
 * Right-panel inspector for the selected org unit:
 *  - Overview: where the unit sits, its level, cost centre, who is responsible, how much
 *    equipment it hosts; editable details for org.manage.
 *  - Equipment: equipment installed at the unit (links to the equipment register), assign /
 *    unassign on the published structure.
 *  - Responsibilities: who is responsible for what (person or team), assign, hand over, end.
 *  - External links: identifiers in other systems (ERP, SAP functional location…).
 *  - Structure changes: move / deactivate through the impact preview (draft workspace).
 *
 * Operational records (equipment, responsibilities, links) always live on the published unit;
 * a draft copy shows those of the unit it was copied from, read-only.
 */

import {
  ArrowRightLeft,
  ExternalLink,
  Info,
  Link2,
  Package,
  ShieldAlert,
  Users,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { MoveNodeDialog } from "@/components/org/MoveNodeDialog";
import { ResponsibilityAssignDialog } from "@/components/org/ResponsibilityAssignDialog";
import { orgNodePathLabel, orgNodeStatusLabel } from "@/components/org/org-display";
import { useResponsibilityTypes } from "@/components/org/use-responsibility-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { ORG_NODE_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { createLatestRequestGuard } from "@/lib/latest-request";
import {
  VersionConflictError,
  assignEquipmentToNode,
  endOrgNodeResponsibility,
  listOrgNodeEquipment,
  searchUnassignedEquipment,
  unassignEquipmentFromNode,
  updateOrgNodeMetadata,
} from "@/services/org-node-service";
import {
  isOrgStructureDesignMode,
  operationalNodeIdOf,
  useOrgDesignerStore,
} from "@/stores/org-designer-store";
import { useOrgNodeStore } from "@/stores/org-node-store";
import type {
  OrgDesignerNodeRow,
  OrgNodeEquipmentRow,
  OrgNodeResponsibility,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Characters typed before the equipment search runs. */
const EQUIPMENT_SEARCH_MIN_CHARS = 2;
/** Results shown per equipment search. */
const EQUIPMENT_SEARCH_LIMIT = 10;

type InspectorTab = "details" | "equipment" | "responsibilities" | "bindings" | "actions";

function DetailRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <dt className="w-36 shrink-0 text-xs text-text-muted">{label}</dt>
      <dd className="min-w-0 flex-1 text-sm text-text-primary">{children}</dd>
    </div>
  );
}

interface NodeInspectorPanelProps {
  /** Structure is read-only (published view): structural actions and detail edits are off. */
  readOnly?: boolean;
}

export function NodeInspectorPanel({ readOnly = false }: NodeInspectorPanelProps) {
  const { t } = useTranslation("org");
  const { formatDate, formatNumber } = useFormatters();
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const workspaceMode = useOrgDesignerStore((s) => s.workspaceMode);
  const openPreview = useOrgDesignerStore((s) => s.openPreview);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  const responsibilities = useOrgNodeStore((s) => s.responsibilities);
  const bindings = useOrgNodeStore((s) => s.bindings);
  const selectNode = useOrgNodeStore((s) => s.selectNode);
  const selectedNode = useOrgNodeStore((s) => s.selectedNode);
  const nodeContextLoading = useOrgNodeStore((s) => s.loading);
  const nodeContextError = useOrgNodeStore((s) => s.error);
  const refreshSelectedNodeContext = useOrgNodeStore((s) => s.refreshSelectedNodeContext);

  const responsibilityTypes = useResponsibilityTypes();

  const isDraftWorkspace = isOrgStructureDesignMode(snapshot, workspaceMode);

  const selectedRow: OrgDesignerNodeRow | null = useMemo(() => {
    if (!snapshot || selectedNodeId === null) return null;
    return snapshot.nodes.find((n) => n.node_id === selectedNodeId) ?? null;
  }, [snapshot, selectedNodeId]);

  /** Published unit carrying equipment / responsibilities / links (null: draft-only unit). */
  const opsNodeId = selectedNodeId != null ? operationalNodeIdOf(snapshot, selectedNodeId) : null;
  /** Operational records are edited on the published structure only. */
  const canEditOps = !isDraftWorkspace && opsNodeId != null;

  useEffect(() => {
    void selectNode(selectedNodeId);
  }, [selectedNodeId, selectNode]);

  const [tab, setTab] = useState<InspectorTab>("details");
  useEffect(() => {
    setTab("details");
  }, [selectedNodeId]);

  // ── Equipment ────────────────────────────────────────────────────────────
  const hasAssetCapability = selectedRow?.can_host_assets ?? false;
  const [equipment, setEquipment] = useState<OrgNodeEquipmentRow[]>([]);
  const [equipmentLoading, setEquipmentLoading] = useState(false);
  const [equipmentError, setEquipmentError] = useState<unknown>(null);
  const [equipSearch, setEquipSearch] = useState("");
  const [equipResults, setEquipResults] = useState<OrgNodeEquipmentRow[]>([]);
  const [equipSearching, setEquipSearching] = useState(false);
  const [equipSearchError, setEquipSearchError] = useState<unknown>(null);
  const [unassignTarget, setUnassignTarget] = useState<OrgNodeEquipmentRow | null>(null);

  const loadEquipment = useCallback(async () => {
    setEquipment([]);
    setEquipmentError(null);
    if (opsNodeId == null || !hasAssetCapability) return;
    setEquipmentLoading(true);
    try {
      setEquipment(await listOrgNodeEquipment(opsNodeId));
    } catch (err) {
      setEquipmentError(err);
    } finally {
      setEquipmentLoading(false);
    }
  }, [opsNodeId, hasAssetCapability]);

  // Typing fast starts several searches: only the answer to the latest query may fill the list.
  const equipSearchGuard = useRef(createLatestRequestGuard());

  useEffect(() => {
    void loadEquipment();
    equipSearchGuard.current.invalidate();
    setEquipSearch("");
    setEquipResults([]);
    setEquipSearching(false);
  }, [loadEquipment]);

  const handleEquipSearch = useCallback(
    async (query: string) => {
      setEquipSearch(query);
      setEquipSearchError(null);
      const isCurrent = equipSearchGuard.current.begin();
      if (!canEditOps || query.trim().length < EQUIPMENT_SEARCH_MIN_CHARS) {
        setEquipResults([]);
        setEquipSearching(false);
        return;
      }
      setEquipSearching(true);
      try {
        const rows = await searchUnassignedEquipment(query.trim(), EQUIPMENT_SEARCH_LIMIT);
        if (!isCurrent()) return;
        setEquipResults(rows);
      } catch (err) {
        if (!isCurrent()) return;
        setEquipSearchError(err);
      } finally {
        if (isCurrent()) setEquipSearching(false);
      }
    },
    [canEditOps],
  );

  const handleAssign = useCallback(
    async (equipmentId: number) => {
      if (opsNodeId == null || !canEditOps) return;
      try {
        await assignEquipmentToNode({ equipment_id: equipmentId, node_id: opsNodeId });
        setEquipSearch("");
        setEquipResults([]);
        await loadEquipment();
      } catch {
        // Reported by the global IPC toast.
      }
    },
    [opsNodeId, canEditOps, loadEquipment],
  );

  const confirmUnassign = useCallback(async () => {
    if (!unassignTarget) return;
    await unassignEquipmentFromNode(unassignTarget.id);
    await loadEquipment();
  }, [unassignTarget, loadEquipment]);

  // ── Responsibilities ─────────────────────────────────────────────────────
  const [assignOpen, setAssignOpen] = useState(false);
  const [assignInitialType, setAssignInitialType] = useState<string | null>(null);
  const [endTarget, setEndTarget] = useState<OrgNodeResponsibility | null>(null);
  const activeResponsibilities = responsibilities.filter((r) => r.valid_to == null);

  const openAssign = (initialType: string | null) => {
    setAssignInitialType(initialType);
    setAssignOpen(true);
  };

  const confirmEnd = useCallback(async () => {
    if (!endTarget) return;
    await endOrgNodeResponsibility(endTarget.id);
    await refreshSelectedNodeContext();
    void loadSnapshot();
  }, [endTarget, refreshSelectedNodeContext, loadSnapshot]);

  // ── Structure actions ────────────────────────────────────────────────────
  const [moveOpen, setMoveOpen] = useState(false);

  // ── Details form ─────────────────────────────────────────────────────────
  const [metaName, setMetaName] = useState("");
  const [metaDescription, setMetaDescription] = useState("");
  const [metaCostCenter, setMetaCostCenter] = useState("");
  const [metaExternalRef, setMetaExternalRef] = useState("");
  const [metaErpRef, setMetaErpRef] = useState("");
  const [metaNotes, setMetaNotes] = useState("");
  const [metaSaving, setMetaSaving] = useState(false);
  const [metaConflict, setMetaConflict] = useState(false);

  useEffect(() => {
    if (selectedNodeId == null || !selectedNode) return;
    if (selectedNode.id !== selectedNodeId) return;
    setMetaName(selectedNode.name);
    setMetaDescription(selectedNode.description ?? "");
    setMetaCostCenter(selectedNode.cost_center_code ?? "");
    setMetaExternalRef(selectedNode.external_reference ?? "");
    setMetaErpRef(selectedNode.erp_reference ?? "");
    setMetaNotes(selectedNode.notes ?? "");
    setMetaConflict(false);
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
    setMetaConflict(false);
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
      // Other failures are reported by the global IPC toast.
      if (e instanceof VersionConflictError) {
        setMetaConflict(true);
        void loadSnapshot();
        void refreshSelectedNodeContext();
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
  ]);

  if (!selectedRow) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-text-muted text-sm">
        <div className="text-center space-y-2">
          <Info className="h-8 w-8 mx-auto opacity-40" aria-hidden />
          <p>{t("designer.inspector.noSelection")}</p>
        </div>
      </div>
    );
  }

  const capabilities: string[] = [];
  if (selectedRow.can_host_assets) capabilities.push(t("capabilities.long.asset"));
  if (selectedRow.can_own_work) capabilities.push(t("capabilities.long.work"));
  if (selectedRow.can_carry_cost_center) capabilities.push(t("capabilities.long.cost"));
  if (selectedRow.can_aggregate_kpis) capabilities.push(t("capabilities.long.kpi"));
  if (selectedRow.can_receive_permits) capabilities.push(t("capabilities.long.permit"));

  const path = orgNodePathLabel(snapshot, selectedRow.node_id);
  const costCentre =
    selectedNode?.id === selectedRow.node_id ? selectedNode.cost_center_code : null;
  const assigneeName = (r: OrgNodeResponsibility) =>
    r.person_display_name ?? r.team_label ?? t("responsibilities.unknownAssignee");
  const opsNote = isDraftWorkspace
    ? opsNodeId == null
      ? t("responsibilities.notPublished")
      : t("responsibilities.draftReadOnly")
    : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-readonly={readOnly ? "true" : undefined}>
      {readOnly && (
        <div className="px-4 pt-3 pb-2 text-xs text-text-secondary border-b border-surface-border/60">
          {t("designer.readOnlyInspectorBanner")}
        </div>
      )}
      {/* Header */}
      <div className="p-4 border-b border-surface-border">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-mono text-xs text-text-muted">{selectedRow.code}</span>
          <StatusChip
            label={orgNodeStatusLabel(t, selectedRow.status)}
            tone={toneOf(ORG_NODE_STATUS_TONE, selectedRow.status)}
          />
        </div>
        <h3 className="text-lg font-semibold text-text-primary mt-1">{selectedRow.name}</h3>
        <p className="text-xs text-text-muted mt-0.5">{selectedRow.node_type_label}</p>
      </div>

      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v as InspectorTab)}
        className="flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        <div className="shrink-0 overflow-x-auto overscroll-x-contain border-b border-surface-border">
          <TabsList className="mx-0 mt-0 inline-flex h-11 w-max max-w-none justify-start gap-0.5 rounded-none bg-transparent p-1">
            <TabsTrigger value="details" className="h-9 shrink-0 px-2.5">
              {t("designer.inspector.details")}
            </TabsTrigger>
            {hasAssetCapability && (
              <TabsTrigger value="equipment" className="h-9 shrink-0 px-2.5">
                {t("designer.inspector.equipment")}
                {equipment.length > 0 && (
                  <Badge variant="secondary" className="ml-1 h-4 shrink-0 px-1 text-[10px]">
                    {formatNumber(equipment.length)}
                  </Badge>
                )}
              </TabsTrigger>
            )}
            <TabsTrigger value="responsibilities" className="h-9 shrink-0 px-2.5">
              {t("designer.inspector.responsibilities")}
            </TabsTrigger>
            <TabsTrigger value="bindings" className="h-9 shrink-0 px-2.5">
              {t("designer.inspector.bindings")}
            </TabsTrigger>
            <TabsTrigger value="actions" className="h-9 shrink-0 px-2.5">
              {t("designer.inspector.actions")}
            </TabsTrigger>
          </TabsList>
        </div>

        {/* Overview tab */}
        <TabsContent
          value="details"
          className="flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          {nodeContextError != null && (
            <ErrorState
              className="mb-4"
              error={nodeContextError}
              onRetry={() => void refreshSelectedNodeContext()}
            />
          )}
          <dl className="mb-4">
            <DetailRow label={t("designer.inspector.location")}>{path}</DetailRow>
            <DetailRow label={t("designer.inspector.type")}>
              {selectedRow.node_type_label}
            </DetailRow>
            <DetailRow label={t("designer.inspector.costCentre")}>
              {!selectedRow.can_carry_cost_center ? (
                <span className="text-text-muted">{t("designer.inspector.costCentreNotUsed")}</span>
              ) : costCentre ? (
                <span className="font-mono">{costCentre}</span>
              ) : (
                <span className="text-text-muted">{t("designer.inspector.costCentreNotSet")}</span>
              )}
            </DetailRow>
            <DetailRow label={t("designer.inspector.responsible")}>
              {activeResponsibilities.length === 0 ? (
                <button
                  type="button"
                  className="text-left text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
                  onClick={() => setTab("responsibilities")}
                >
                  {t("designer.inspector.nobodyResponsible")}
                </button>
              ) : (
                <ul className="space-y-0.5">
                  {activeResponsibilities.map((r) => (
                    <li key={r.id}>
                      <span className="text-text-muted">
                        {responsibilityTypes.labelOf(r.responsibility_type)}
                      </span>
                      {" : "}
                      {assigneeName(r)}
                    </li>
                  ))}
                </ul>
              )}
            </DetailRow>
            {hasAssetCapability && (
              <DetailRow label={t("designer.inspector.equipmentCount")}>
                {equipmentLoading ? (
                  t("designer.inspector.searching")
                ) : equipmentError != null ? (
                  // A failed load is not "0 equipment".
                  <button
                    type="button"
                    className="text-left text-tone-danger-fg underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
                    onClick={() => setTab("equipment")}
                  >
                    {t("designer.inspector.equipmentCountUnavailable")}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="text-left text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
                    onClick={() => setTab("equipment")}
                  >
                    {t("designer.inspector.equipmentCountValue", { count: equipment.length })}
                  </button>
                )}
              </DetailRow>
            )}
            <DetailRow label={t("designer.inspector.childCount")}>
              {formatNumber(selectedRow.child_count)}
            </DetailRow>
            <DetailRow label={t("designer.inspector.capabilitiesLabel")}>
              {capabilities.length > 0 ? (
                <span className="flex flex-wrap gap-1.5">
                  {capabilities.map((c) => (
                    <Badge key={c} variant="secondary" className="text-[11px]">
                      {c}
                    </Badge>
                  ))}
                </span>
              ) : (
                <span className="text-text-muted">{t("designer.inspector.noCaps")}</span>
              )}
            </DetailRow>
          </dl>

          {!readOnly && (
            <PermissionGate permission={P.ORG_MANAGE}>
              <Separator className="my-2" />
              <div className="mt-3 space-y-2 rounded-lg border border-surface-border p-3">
                <p className="text-xs font-medium text-text-muted">
                  {t("designer.metadata.title")}
                </p>
                {nodeContextLoading && !selectedNode ? (
                  <LoadingState label={t("designer.metadata.loading")} />
                ) : (
                  selectedNode && (
                    <>
                      <div className="space-y-1.5">
                        <Label htmlFor="org-meta-name" className="text-xs">
                          {t("designer.metadata.name")}
                        </Label>
                        <Input
                          id="org-meta-name"
                          value={metaName}
                          onChange={(e) => setMetaName(e.target.value)}
                          className="h-9 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor="org-meta-description" className="text-xs text-text-muted">
                          {t("designer.metadata.description")}
                        </Label>
                        <Textarea
                          id="org-meta-description"
                          value={metaDescription}
                          onChange={(e) => setMetaDescription(e.target.value)}
                          className="min-h-[64px] text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      {selectedRow.can_carry_cost_center && (
                        <div className="space-y-1.5">
                          <Label htmlFor="org-meta-cost" className="text-xs text-text-muted">
                            {t("designer.metadata.costCenter")}
                          </Label>
                          <Input
                            id="org-meta-cost"
                            value={metaCostCenter}
                            onChange={(e) => setMetaCostCenter(e.target.value)}
                            className="h-9 text-sm font-mono"
                            disabled={metaSaving}
                          />
                        </div>
                      )}
                      <div className="space-y-1.5">
                        <Label htmlFor="org-meta-extref" className="text-xs text-text-muted">
                          {t("designer.metadata.externalRef")}
                        </Label>
                        <Input
                          id="org-meta-extref"
                          value={metaExternalRef}
                          onChange={(e) => setMetaExternalRef(e.target.value)}
                          className="h-9 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor="org-meta-erp" className="text-xs text-text-muted">
                          {t("designer.metadata.erpRef")}
                        </Label>
                        <Input
                          id="org-meta-erp"
                          value={metaErpRef}
                          onChange={(e) => setMetaErpRef(e.target.value)}
                          className="h-9 text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor="org-meta-notes" className="text-xs text-text-muted">
                          {t("designer.metadata.notes")}
                        </Label>
                        <Textarea
                          id="org-meta-notes"
                          value={metaNotes}
                          onChange={(e) => setMetaNotes(e.target.value)}
                          className="min-h-[48px] text-sm"
                          disabled={metaSaving}
                        />
                      </div>
                      {metaConflict && (
                        <p className="text-xs text-status-danger" role="alert">
                          {t("designer.metadata.versionConflict")}
                        </p>
                      )}
                      <Button
                        type="button"
                        onClick={() => void handleMetadataSave()}
                        disabled={!isMetaDirty || !selectedNode || metaSaving}
                        className="h-10"
                      >
                        {metaSaving ? t("designer.metadata.saving") : t("designer.metadata.save")}
                      </Button>
                    </>
                  )
                )}
              </div>
            </PermissionGate>
          )}
        </TabsContent>

        {/* Equipment tab */}
        {hasAssetCapability && (
          <TabsContent
            value="equipment"
            className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
          >
            {opsNote && <p className="mb-3 text-xs text-text-secondary">{opsNote}</p>}
            {equipmentLoading ? (
              <LoadingState />
            ) : equipmentError != null ? (
              <ErrorState error={equipmentError} onRetry={() => void loadEquipment()} />
            ) : equipment.length === 0 ? (
              <EmptyState
                icon={<Package className="h-6 w-6" aria-hidden />}
                title={t("designer.inspector.noEquipment")}
                {...(canEditOps ? { description: t("designer.inspector.noEquipmentHint") } : {})}
              />
            ) : (
              <ul className="space-y-2">
                {equipment.map((eq) => (
                  <li
                    key={eq.id}
                    className="flex items-center justify-between gap-2 rounded-lg border border-surface-border p-3 text-sm"
                  >
                    <div className="min-w-0">
                      <Link
                        to={`/equipment?assetId=${eq.id}`}
                        className="inline-flex max-w-full items-center gap-1.5 rounded text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        title={t("designer.inspector.openEquipment", { code: eq.asset_id_code })}
                      >
                        <span className="font-mono text-xs">{eq.asset_id_code}</span>
                        <span className="truncate font-medium">{eq.name}</span>
                        <ExternalLink className="h-3 w-3 shrink-0" aria-hidden />
                      </Link>
                      <div className="mt-1">
                        <AssetStatusBadge code={eq.lifecycle_status} />
                      </div>
                    </div>
                    {canEditOps && (
                      <PermissionGate permission={P.ORG_MANAGE}>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-9 w-9 shrink-0 p-0"
                          onClick={() => setUnassignTarget(eq)}
                          aria-label={t("designer.inspector.unassignEquipment", {
                            code: eq.asset_id_code,
                          })}
                          title={t("designer.inspector.unassignEquipment", {
                            code: eq.asset_id_code,
                          })}
                        >
                          <X className="h-4 w-4 text-status-danger" aria-hidden />
                        </Button>
                      </PermissionGate>
                    )}
                  </li>
                ))}
              </ul>
            )}

            {canEditOps && (
              <PermissionGate permission={P.ORG_MANAGE}>
                <div className="mt-4 space-y-2">
                  <Label htmlFor="org-equip-search" className="text-xs text-text-muted">
                    {t("designer.inspector.assignEquipment")}
                  </Label>
                  <Input
                    id="org-equip-search"
                    value={equipSearch}
                    onChange={(e) => void handleEquipSearch(e.target.value)}
                    placeholder={t("designer.inspector.equipSearchPlaceholder")}
                    className="h-10 text-sm"
                  />
                  {equipSearching && (
                    <p className="text-xs text-text-muted">{t("designer.inspector.searching")}</p>
                  )}
                  {equipSearchError != null && (
                    <p className="text-xs text-status-danger" role="alert">
                      {describeAppError(equipSearchError).title}
                    </p>
                  )}
                  {equipResults.length > 0 && (
                    <div className="space-y-1 max-h-48 overflow-y-auto border border-surface-border rounded-md p-1">
                      {equipResults.map((eq) => (
                        <button
                          key={eq.id}
                          type="button"
                          className="min-h-11 w-full text-left rounded-md px-3 py-2 text-sm hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          onClick={() => void handleAssign(eq.id)}
                        >
                          <div className="flex items-center gap-2">
                            <span className="font-mono text-xs text-text-muted">
                              {eq.asset_id_code}
                            </span>
                            <span>{eq.name}</span>
                          </div>
                          {eq.current_node_name && (
                            <p className="text-xs text-status-warning mt-0.5">
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
              </PermissionGate>
            )}
          </TabsContent>
        )}

        {/* Responsibilities tab */}
        <TabsContent
          value="responsibilities"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          {opsNote && <p className="mb-3 text-xs text-text-secondary">{opsNote}</p>}
          {canEditOps && opsNodeId != null && (
            <PermissionGate permission={P.ORG_MANAGE}>
              <Button className="mb-3 h-10 w-full gap-2" onClick={() => openAssign(null)}>
                <Users className="h-4 w-4" aria-hidden />
                {t("responsibilities.assign")}
              </Button>
            </PermissionGate>
          )}
          {activeResponsibilities.length === 0 ? (
            <EmptyState
              icon={<Users className="h-6 w-6" aria-hidden />}
              title={t("designer.inspector.noResponsibilities")}
              description={t("responsibilities.emptyHint")}
            />
          ) : (
            <ul className="space-y-2">
              {activeResponsibilities.map((r) => (
                <li key={r.id} className="rounded-lg border border-surface-border p-3 text-sm">
                  <div className="font-medium text-text-primary">
                    {responsibilityTypes.labelOf(r.responsibility_type)}
                  </div>
                  <div className="text-sm text-text-secondary mt-0.5">
                    {r.person_id != null
                      ? t("responsibilities.personLine", { name: assigneeName(r) })
                      : t("responsibilities.teamLine", { name: assigneeName(r) })}
                  </div>
                  <div className="text-xs text-text-muted mt-0.5">
                    {t("responsibilities.since", {
                      date: formatDate(r.valid_from ?? r.created_at),
                    })}
                  </div>
                  {canEditOps && (
                    <PermissionGate permission={P.ORG_MANAGE}>
                      <div className="mt-2 flex flex-wrap gap-2">
                        <Button
                          variant="outline"
                          className="h-9"
                          onClick={() => openAssign(r.responsibility_type)}
                        >
                          {t("responsibilities.replace")}
                        </Button>
                        <Button
                          variant="outline"
                          className="h-9 text-status-danger hover:text-status-danger"
                          onClick={() => setEndTarget(r)}
                        >
                          {t("responsibilities.end")}
                        </Button>
                      </div>
                    </PermissionGate>
                  )}
                </li>
              ))}
            </ul>
          )}
        </TabsContent>

        {/* External links tab */}
        <TabsContent
          value="bindings"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          {bindings.length === 0 ? (
            <EmptyState
              icon={<Link2 className="h-6 w-6" aria-hidden />}
              title={t("designer.inspector.noBindings")}
            />
          ) : (
            <ul className="space-y-2">
              {bindings.map((b) => (
                <li key={b.id} className="rounded-lg border border-surface-border p-3 text-sm">
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-medium text-text-primary">{b.binding_type}</span>
                    {b.is_primary && (
                      <StatusChip label={t("designer.inspector.primary")} tone="info" />
                    )}
                  </div>
                  <div className="text-xs text-text-secondary mt-1">
                    {b.external_system} → <span className="font-mono">{b.external_id}</span>
                  </div>
                  {b.valid_from && (
                    <div className="text-xs text-text-muted mt-0.5">
                      {t("responsibilities.since", { date: formatDate(b.valid_from) })}
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </TabsContent>

        {/* Structure changes tab */}
        <TabsContent
          value="actions"
          className="mt-0 flex-1 min-h-0 overflow-y-auto p-4 data-[state=inactive]:hidden"
        >
          <div className="space-y-3">
            <p className="text-xs text-text-secondary">
              {t("designer.inspector.actionsDescription")}
            </p>
            {readOnly && (
              <p className="rounded-md border border-surface-border px-3 py-2 text-sm text-text-secondary">
                {t("designer.inspector.actionsNeedDraft")}
              </p>
            )}
            <PermissionGate permission={P.ORG_ADMIN}>
              <Button
                variant="outline"
                className="h-11 w-full justify-start gap-2"
                onClick={() => setMoveOpen(true)}
                disabled={readOnly}
              >
                <ArrowRightLeft className="h-4 w-4" aria-hidden />
                {t("designer.actions.moveNode")}
              </Button>

              <Button
                variant="outline"
                className="h-11 w-full justify-start gap-2 text-status-danger hover:text-status-danger"
                onClick={() =>
                  void openPreview({ action: "DeactivateNode", node_id: selectedRow.node_id })
                }
                disabled={readOnly || selectedRow.status !== "active"}
              >
                <ShieldAlert className="h-4 w-4" aria-hidden />
                {t("designer.actions.deactivateNode")}
              </Button>
              {selectedRow.status !== "active" && (
                <p className="text-xs text-text-muted">{t("designer.inspector.alreadyInactive")}</p>
              )}
            </PermissionGate>
            <Button
              variant="outline"
              className="h-11 w-full justify-start gap-2"
              onClick={() => setTab("responsibilities")}
            >
              <Users className="h-4 w-4" aria-hidden />
              {t("designer.actions.reassignResponsibility")}
            </Button>
          </div>
        </TabsContent>
      </Tabs>

      {moveOpen && <MoveNodeDialog open={moveOpen} onOpenChange={setMoveOpen} node={selectedRow} />}

      {assignOpen && canEditOps && opsNodeId != null && (
        <ResponsibilityAssignDialog
          open={assignOpen}
          onOpenChange={setAssignOpen}
          nodeId={selectedRow.node_id}
          nodeName={selectedRow.name}
          operationalNodeId={opsNodeId}
          types={responsibilityTypes.types}
          activeAssignments={activeResponsibilities}
          initialType={assignInitialType}
          onAssigned={() => {
            void refreshSelectedNodeContext();
            void loadSnapshot();
          }}
        />
      )}

      <ConfirmDialog
        open={endTarget != null}
        onOpenChange={(open) => !open && setEndTarget(null)}
        title={t("responsibilities.endTitle")}
        description={
          endTarget
            ? t("responsibilities.endDescription", {
                type: responsibilityTypes.labelOf(endTarget.responsibility_type),
                name: assigneeName(endTarget),
                unit: selectedRow.name,
              })
            : ""
        }
        confirmLabel={t("responsibilities.endConfirm")}
        destructive
        onConfirm={confirmEnd}
      />

      <ConfirmDialog
        open={unassignTarget != null}
        onOpenChange={(open) => !open && setUnassignTarget(null)}
        title={t("designer.inspector.unassignTitle")}
        description={
          unassignTarget
            ? t("designer.inspector.unassignDescription", {
                code: unassignTarget.asset_id_code,
                name: unassignTarget.name,
                unit: selectedRow.name,
              })
            : ""
        }
        confirmLabel={t("designer.inspector.unassignConfirm")}
        destructive
        onConfirm={confirmUnassign}
      />
    </div>
  );
}
