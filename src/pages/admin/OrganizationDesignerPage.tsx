/**
 * OrganizationDesignerPage.tsx
 *
 * Admin workspace for designing and reviewing the org model.
 * Governed lifecycle: explicit “published (read-only)” vs “draft (editing)” modes.
 */

import { AlertTriangle, Building2, Network, RefreshCw, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { ImpactPreviewDrawer } from "@/components/org/ImpactPreviewDrawer";
import { NodeTypeManagerPanel } from "@/components/org/NodeTypeManagerPanel";
import { OrgDesignerLifecycleBar } from "@/components/org/OrgDesignerLifecycleBar";
import { OrgDesignerPropertyPanel } from "@/components/org/OrgDesignerPropertyPanel";
import { OrgExportMenu } from "@/components/org/OrgExportMenu";
import { OrgNodeCreateDialog } from "@/components/org/OrgNodeCreateDialog";
import { OrgRelationshipRulesPanel } from "@/components/org/OrgRelationshipRulesPanel";
import { OrganizationTreePanel } from "@/components/org/OrganizationTreePanel";
import { DraftStatusBanner } from "@/components/org/DraftStatusBanner";
import { Button } from "@/components/ui/button";
import { mfLayout } from "@/design-system/tokens";
import { isOrgStructureDesignMode, useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";

export function OrganizationDesignerPage() {
  const { t } = useTranslation("org");
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

  // Loading state
  if (loading && !snapshot) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  // Error state
  if (error && !snapshot) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="text-center space-y-3">
          <AlertTriangle className="h-8 w-8 mx-auto text-status-danger" />
          <p className="text-sm text-status-danger">{error}</p>
          <Button variant="outline" size="sm" onClick={() => void loadSnapshot()}>
            {t("designer.retry")}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Building2 className={mfLayout.moduleHeaderIcon} />
          <h1 className={mfLayout.moduleTitle}>{t("designer.title")}</h1>
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          <PermissionGate permission="org.admin">
            <Button
              variant="outline"
              size="sm"
              onClick={() => canOpenTypeManager && setTypesPanelOpen(true)}
              disabled={!canOpenTypeManager}
              title={
                !hasDraftModel
                  ? t("designer.manageTypesNoDraftHint")
                  : !isDesign
                    ? t("designer.manageTypesReadOnlyHint")
                    : undefined
              }
              className="gap-1.5"
            >
              <Settings2 className="h-3.5 w-3.5" />
              {t("designer.manageTypes")}
            </Button>
          </PermissionGate>
          <PermissionGate permission="org.admin">
            <Button
              variant="outline"
              size="sm"
              onClick={() => canOpenTypeManager && setRulesPanelOpen(true)}
              disabled={!canOpenTypeManager}
              title={
                !hasDraftModel
                  ? t("designer.manageHierarchyRulesNoDraftHint")
                  : !isDesign
                    ? t("designer.manageHierarchyRulesReadOnlyHint")
                    : undefined
              }
              className="gap-1.5"
            >
              <Network className="h-3.5 w-3.5" />
              {t("designer.manageHierarchyRules")}
            </Button>
          </PermissionGate>
          <OrgExportMenu treeContainerId="org-tree-container" />
          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadSnapshot()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            {t("designer.refresh")}
          </Button>
        </div>
      </div>

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
                    {snapshot?.nodes.length ?? 0}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>{t("designer.nodeTypesCount")}</span>
                  <span className="text-text-primary font-medium">{nodeTypes.length}</span>
                </div>
              </div>
            </div>
          </aside>

          <main
            id="org-tree-container"
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
          </main>

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
    </div>
  );
}
