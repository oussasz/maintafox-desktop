/**
 * OrganizationDesignerPage.tsx
 *
 * Admin workspace for designing and reviewing the org model.
 * Governed lifecycle: explicit “published (read-only)” vs “draft (editing)” modes.
 */

import { Building2, Network, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { DraftStatusBanner } from "@/components/org/DraftStatusBanner";
import { ImpactPreviewDrawer } from "@/components/org/ImpactPreviewDrawer";
import { NodeTypeManagerPanel } from "@/components/org/NodeTypeManagerPanel";
import { OrgDesignerLifecycleBar } from "@/components/org/OrgDesignerLifecycleBar";
import { OrgDesignerPropertyPanel } from "@/components/org/OrgDesignerPropertyPanel";
import { OrgExportMenu } from "@/components/org/OrgExportMenu";
import { OrgNodeCreateDialog } from "@/components/org/OrgNodeCreateDialog";
import { OrgRelationshipRulesPanel } from "@/components/org/OrgRelationshipRulesPanel";
import { OrganizationTreePanel } from "@/components/org/OrganizationTreePanel";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { isOrgStructureDesignMode, useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import { P } from "@shared/rbac/permissions.generated";

export function OrganizationDesignerPage() {
  const { t } = useTranslation("org");
  const { formatNumber } = useFormatters();
  const [typesPanelOpen, setTypesPanelOpen] = useState(false);
  const [rulesPanelOpen, setRulesPanelOpen] = useState(false);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const loading = useOrgDesignerStore((s) => s.loading);
  const error = useOrgDesignerStore((s) => s.error);
  const workspaceMode = useOrgDesignerStore((s) => s.workspaceMode);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);
  const loadPublishValidation = useOrgGovernanceStore((s) => s.loadPublishValidation);

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  const nodeTypes = useMemo(() => {
    if (!snapshot) return [];
    const seen = new Map<string, string>();
    for (const node of snapshot.nodes) {
      if (!seen.has(node.node_type_code)) {
        seen.set(node.node_type_code, node.node_type_label);
      }
    }
    return Array.from(seen.entries()).map(([code, label]) => ({ code, label }));
  }, [snapshot]);

  const hasActiveModel = snapshot?.active_model_id != null;
  const hasDraftModel = snapshot?.draft_model_id != null;
  const hasWorkspace = hasActiveModel || hasDraftModel;

  const isDesign = isOrgStructureDesignMode(snapshot, workspaceMode);
  const canOpenTypeManager = hasDraftModel && isDesign;
  const draftModelId = snapshot?.draft_model_id ?? null;
  /** Draft workspace owns its tree — nodes are created on the draft model. */
  const canAddDraftNodes = isDesign && draftModelId != null;

  const [createNodeOpen, setCreateNodeOpen] = useState(false);
  const [createNodeMode, setCreateNodeMode] = useState<"root" | "child">("root");
  /** Bumps when draft types/rules change so create-dialog reloads allowed children. */
  const [schemaRevision, setSchemaRevision] = useState(0);
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);
  const setSelectedNodeId = useOrgDesignerStore((s) => s.setSelectedNodeId);

  const selectedNodeRow = useMemo(() => {
    if (!snapshot || selectedNodeId == null) return null;
    return snapshot.nodes.find((n) => n.node_id === selectedNodeId) ?? null;
  }, [snapshot, selectedNodeId]);

  const onNodeCreated = useCallback(
    async (nodeId: number) => {
      await loadSnapshot();
      setSelectedNodeId(nodeId);
    },
    [loadSnapshot, setSelectedNodeId],
  );

  /** After draft schema mutations: refresh tree snapshot, re-run publish validation, reload create-dialog rules. */
  const refreshAfterSchemaChange = useCallback(async () => {
    await loadSnapshot();
    setSchemaRevision((n) => n + 1);
    const draftId = useOrgDesignerStore.getState().snapshot?.draft_model_id;
    if (draftId != null) {
      await loadPublishValidation(draftId);
    }
  }, [loadSnapshot, loadPublishValidation]);

  const headerActions = (
    <>
      <PermissionGate permission={P.ORG_ADMIN}>
        <Button
          variant="outline"
          onClick={() => canOpenTypeManager && setTypesPanelOpen(true)}
          disabled={!canOpenTypeManager}
          title={
            !hasDraftModel
              ? t("designer.manageTypesNoDraftHint")
              : !isDesign
                ? t("designer.manageTypesReadOnlyHint")
                : undefined
          }
          className="h-11 gap-1.5"
        >
          <Settings2 className="h-4 w-4" aria-hidden />
          {t("designer.manageTypes")}
        </Button>
      </PermissionGate>
      <PermissionGate permission={P.ORG_ADMIN}>
        <Button
          variant="outline"
          onClick={() => canOpenTypeManager && setRulesPanelOpen(true)}
          disabled={!canOpenTypeManager}
          title={
            !hasDraftModel
              ? t("designer.manageHierarchyRulesNoDraftHint")
              : !isDesign
                ? t("designer.manageHierarchyRulesReadOnlyHint")
                : undefined
          }
          className="h-11 gap-1.5"
        >
          <Network className="h-4 w-4" aria-hidden />
          {t("designer.manageHierarchyRules")}
        </Button>
      </PermissionGate>
      <OrgExportMenu treeContainerId="org-tree-container" />
      <RefreshButton onRefresh={() => void loadSnapshot()} loading={loading} />
    </>
  );

  // First load / failed first load: the page has nothing else to show yet.
  if (!snapshot) {
    return (
      <ModulePageShell
        icon={Building2}
        title={t("designer.title")}
        actions={<RefreshButton onRefresh={() => void loadSnapshot()} loading={loading} />}
      >
        {error != null && !loading ? (
          <ErrorState error={error} onRetry={() => void loadSnapshot()} />
        ) : (
          <LoadingState label={t("designer.loading")} />
        )}
      </ModulePageShell>
    );
  }

  return (
    <ModulePageShell
      icon={Building2}
      title={t("designer.title")}
      count={snapshot.nodes.length}
      actions={headerActions}
      bodyClassName={null}
    >
      <OrgDesignerLifecycleBar />

      <DraftStatusBanner
        draftModelId={snapshot?.draft_model_id ?? null}
        draftVersion={snapshot?.draft_model_version ?? null}
        hasActivePublished={hasActiveModel}
        visible={isDesign}
      />

      {hasWorkspace && (
        <div className={mfLayout.moduleWorkspaceSplit}>
          <aside className="w-44 shrink-0 border-r border-surface-border flex min-h-0 flex-col">
            <div className="p-4 space-y-2">
              <h2 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("designer.modelSummary")}
              </h2>
              <div className="text-xs text-text-muted space-y-1">
                <div className="flex justify-between">
                  <span>{t("designer.totalNodes")}</span>
                  <span className="text-text-primary font-medium">
                    {formatNumber(snapshot.nodes.length)}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>{t("designer.nodeTypesCount")}</span>
                  <span className="text-text-primary font-medium">
                    {formatNumber(nodeTypes.length)}
                  </span>
                </div>
              </div>
            </div>
          </aside>

          <section
            id="org-tree-container"
            aria-label={t("designer.treeLabel")}
            className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
          >
            <OrganizationTreePanel
              readOnly={!isDesign}
              canAddDraftNodes={canAddDraftNodes}
              showNoActiveModelHint={false}
              onAddRoot={() => {
                setCreateNodeMode("root");
                setCreateNodeOpen(true);
              }}
              onAddChild={() => {
                setCreateNodeMode("child");
                setCreateNodeOpen(true);
              }}
            />
          </section>

          <OrgDesignerPropertyPanel readOnly={!isDesign} />
        </div>
      )}

      {canAddDraftNodes && draftModelId != null && (
        <OrgNodeCreateDialog
          open={createNodeOpen}
          onOpenChange={setCreateNodeOpen}
          mode={createNodeMode}
          parentNode={createNodeMode === "child" ? selectedNodeRow : null}
          structureModelId={draftModelId}
          schemaRevision={schemaRevision}
          onCreated={onNodeCreated}
        />
      )}

      <NodeTypeManagerPanel
        open={typesPanelOpen}
        onOpenChange={setTypesPanelOpen}
        structureModelId={snapshot?.draft_model_id ?? null}
        onTypesChanged={() => void refreshAfterSchemaChange()}
      />

      <OrgRelationshipRulesPanel
        open={rulesPanelOpen}
        onOpenChange={setRulesPanelOpen}
        structureModelId={snapshot?.draft_model_id ?? null}
        onChanged={() => void refreshAfterSchemaChange()}
      />

      <ImpactPreviewDrawer />
    </ModulePageShell>
  );
}
