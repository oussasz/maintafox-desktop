/**
 * OrganizationDesignerPage.tsx
 *
 * Admin workspace for designing and reviewing the org model.
 * Governed lifecycle: explicit “published (read-only)” vs “draft (editing)” modes.
 */

import { AlertTriangle, Building2, RefreshCw, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AuditTimeline } from "@/components/org/AuditTimeline";
import { ImpactPreviewDrawer } from "@/components/org/ImpactPreviewDrawer";
import { NodeInspectorPanel } from "@/components/org/NodeInspectorPanel";
import { NodeTypeManagerPanel } from "@/components/org/NodeTypeManagerPanel";
import { OrgDesignerLifecycleBar } from "@/components/org/OrgDesignerLifecycleBar";
import { OrgExportMenu } from "@/components/org/OrgExportMenu";
import { OrgNodeCreateDialog } from "@/components/org/OrgNodeCreateDialog";
import { OrgRelationshipRulesPanel } from "@/components/org/OrgRelationshipRulesPanel";
import { OrganizationTreePanel } from "@/components/org/OrganizationTreePanel";
import { PublishReadinessBanner } from "@/components/org/PublishReadinessBanner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { isOrgStructureDesignMode, useOrgDesignerStore } from "@/stores/org-designer-store";

export function OrganizationDesignerPage() {
  const { t } = useTranslation("org");
  const [typesPanelOpen, setTypesPanelOpen] = useState(false);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const loading = useOrgDesignerStore((s) => s.loading);
  const error = useOrgDesignerStore((s) => s.error);
  const statusFilter = useOrgDesignerStore((s) => s.statusFilter);
  const typeFilter = useOrgDesignerStore((s) => s.typeFilter);
  const workspaceMode = useOrgDesignerStore((s) => s.workspaceMode);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);
  const setStatusFilter = useOrgDesignerStore((s) => s.setStatusFilter);
  const setTypeFilter = useOrgDesignerStore((s) => s.setTypeFilter);

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
  const activeModelId = snapshot?.active_model_id ?? null;
  const canAddLiveNodes = isDesign && activeModelId != null;

  const [createNodeOpen, setCreateNodeOpen] = useState(false);
  const [createNodeMode, setCreateNodeMode] = useState<"root" | "child">("root");
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
          {hasActiveModel && (
            <Badge variant="default" className="text-xs">
              {t("designer.modelVersion", {
                version: snapshot?.active_model_version ?? 0,
              })}
            </Badge>
          )}
          {hasDraftModel && (
            <Badge variant="secondary" className="text-xs">
              {t("lifecycle.draftVersionBadge", { version: snapshot?.draft_model_version ?? "—" })}
            </Badge>
          )}
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

      {isDesign && snapshot?.draft_model_id != null && (
        <OrgRelationshipRulesPanel
          structureModelId={snapshot.draft_model_id}
          onChanged={() => void loadSnapshot()}
        />
      )}

      <div className="shrink-0">
        <PublishReadinessBanner
          draftModelId={snapshot?.draft_model_id ?? null}
          visible={isDesign}
        />
      </div>

      {hasWorkspace && (
        <div className={mfLayout.moduleWorkspaceSplit}>
          <aside className="w-56 shrink-0 border-r border-surface-border flex min-h-0 flex-col">
            <div className="p-4 space-y-4">
              <h2 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("designer.filters")}
              </h2>

              <div className="space-y-1.5">
                <label className="text-xs text-text-muted">{t("designer.statusFilter")}</label>
                <Select
                  value={statusFilter ?? "__all__"}
                  onValueChange={(v) => setStatusFilter(v === "__all__" ? null : v)}
                >
                  <SelectTrigger className="h-8 text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">{t("designer.allStatuses")}</SelectItem>
                    <SelectItem value="active">{t("designer.statusActive")}</SelectItem>
                    <SelectItem value="inactive">{t("designer.statusInactive")}</SelectItem>
                    <SelectItem value="draft">{t("designer.statusDraft")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-1.5">
                <label className="text-xs text-text-muted">{t("designer.typeFilter")}</label>
                <Select
                  value={typeFilter ?? "__all__"}
                  onValueChange={(v) => setTypeFilter(v === "__all__" ? null : v)}
                >
                  <SelectTrigger className="h-8 text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">{t("designer.allTypes")}</SelectItem>
                    {nodeTypes.map((nt) => (
                      <SelectItem key={nt.code} value={nt.code}>
                        {nt.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <Separator />

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
            className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border-r border-surface-border"
          >
            <OrganizationTreePanel
              readOnly={!isDesign}
              canAddLiveNodes={canAddLiveNodes}
              showNoActiveModelHint={isDesign && hasDraftModel && activeModelId == null}
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

          <aside className="flex w-80 min-h-0 shrink-0 flex-col self-stretch overflow-hidden">
            <Tabs
              defaultValue="inspector"
              className="flex h-full min-h-0 flex-1 flex-col overflow-hidden"
            >
              <TabsList className="mx-2 mt-2 shrink-0">
                <TabsTrigger value="inspector">{t("designer.inspectorTab")}</TabsTrigger>
                <TabsTrigger value="audit">{t("designer.auditTab")}</TabsTrigger>
              </TabsList>
              <TabsContent
                value="inspector"
                className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden p-0 data-[state=inactive]:hidden"
              >
                <NodeInspectorPanel readOnly={!isDesign} />
              </TabsContent>
              <TabsContent
                value="audit"
                className="mt-0 min-h-0 flex-1 overflow-y-auto p-0 data-[state=inactive]:hidden"
              >
                <AuditTimeline />
              </TabsContent>
            </Tabs>
          </aside>
        </div>
      )}

      {activeModelId != null && canAddLiveNodes && (
        <OrgNodeCreateDialog
          open={createNodeOpen}
          onOpenChange={setCreateNodeOpen}
          mode={createNodeMode}
          parentNode={createNodeMode === "child" ? selectedNodeRow : null}
          activeModelId={activeModelId}
          onCreated={onNodeCreated}
        />
      )}

      <NodeTypeManagerPanel
        open={typesPanelOpen}
        onOpenChange={setTypesPanelOpen}
        structureModelId={snapshot?.draft_model_id ?? null}
        onTypesChanged={() => void loadSnapshot()}
      />

      <ImpactPreviewDrawer />
    </div>
  );
}
