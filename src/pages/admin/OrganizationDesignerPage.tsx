/**
 * OrganizationDesignerPage.tsx
 *
 * Admin workspace for designing and reviewing the org model.
 * Three-pane layout: filters/model info | tree panel | node inspector.
 */

import { AlertTriangle, Building2, RefreshCw, Settings2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AuditTimeline } from "@/components/org/AuditTimeline";
import { ImpactPreviewDrawer } from "@/components/org/ImpactPreviewDrawer";
import { NodeInspectorPanel } from "@/components/org/NodeInspectorPanel";
import { NodeTypeManagerPanel } from "@/components/org/NodeTypeManagerPanel";
import { OrgExportMenu } from "@/components/org/OrgExportMenu";
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
import { useOrgDesignerStore } from "@/stores/org-designer-store";

export function OrganizationDesignerPage() {
  const { t } = useTranslation("org");
  const [typesPanelOpen, setTypesPanelOpen] = useState(false);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const loading = useOrgDesignerStore((s) => s.loading);
  const error = useOrgDesignerStore((s) => s.error);
  const statusFilter = useOrgDesignerStore((s) => s.statusFilter);
  const typeFilter = useOrgDesignerStore((s) => s.typeFilter);
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
      {/* Page header — DI/OT pattern */}
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
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          <Button
            variant="outline"
            size="sm"
            onClick={() => hasDraftModel && setTypesPanelOpen(true)}
            disabled={!hasDraftModel}
            title={!hasDraftModel ? t("designer.manageTypesNoDraftHint") : undefined}
            className="gap-1.5"
          >
            <Settings2 className="h-3.5 w-3.5" />
            {t("designer.manageTypes")}
          </Button>
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

      {/* No active model banner */}
      {!hasActiveModel && (
        <div className="mx-6 mt-4 rounded-lg border border-status-warning/30 bg-status-warning/10 p-4 flex items-center gap-3">
          <AlertTriangle className="h-5 w-5 text-status-warning shrink-0" />
          <div>
            <p className="text-sm font-medium text-text-primary">{t("designer.noActiveModel")}</p>
            <p className="text-xs text-text-muted mt-0.5">{t("designer.noActiveModelHint")}</p>
          </div>
        </div>
      )}

      {/* Publish-readiness banner — shown when a draft model exists */}
      <PublishReadinessBanner draftModelId={snapshot?.draft_model_id ?? null} />

      {/* Three-pane workspace */}
      {hasActiveModel && (
        <div className="flex flex-1 min-h-0">
          {/* Left sidebar: filters */}
          <aside className="w-56 shrink-0 border-r border-surface-border flex flex-col">
            <div className="p-4 space-y-4">
              <h2 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("designer.filters")}
              </h2>

              {/* Status filter */}
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

              {/* Type filter */}
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

            {/* Model summary */}
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

          {/* Center: tree */}
          <main id="org-tree-container" className="flex-1 min-w-0 border-r border-surface-border">
            <OrganizationTreePanel />
          </main>

          {/* Right: inspector + audit tabs */}
          <aside className="w-80 shrink-0 flex flex-col">
            <Tabs defaultValue="inspector" className="flex flex-col h-full">
              <TabsList className="mx-2 mt-2">
                <TabsTrigger value="inspector">{t("designer.inspectorTab")}</TabsTrigger>
                <TabsTrigger value="audit">{t("designer.auditTab")}</TabsTrigger>
              </TabsList>
              <TabsContent value="inspector" className="flex-1 min-h-0 overflow-y-auto">
                <NodeInspectorPanel />
              </TabsContent>
              <TabsContent value="audit" className="flex-1 min-h-0 overflow-y-auto">
                <AuditTimeline />
              </TabsContent>
            </Tabs>
          </aside>
        </div>
      )}

      {/* Node type manager sheet */}
      <NodeTypeManagerPanel
        open={typesPanelOpen}
        onOpenChange={setTypesPanelOpen}
        structureModelId={snapshot?.draft_model_id ?? null}
        onTypesChanged={() => void loadSnapshot()}
      />

      {/* Impact preview drawer — always mounted, visibility controlled by store */}
      <ImpactPreviewDrawer />
    </div>
  );
}
