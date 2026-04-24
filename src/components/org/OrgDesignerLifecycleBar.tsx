/**
 * Governs the organization designer “view published” vs “edit draft” mode and
 * the explicit actions to create, fork, or abandon structure drafts.
 */

import { BookOpen, PenLine, Plus } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  isOrgStructureDesignMode,
  type OrgDesignerWorkspaceMode,
  useOrgDesignerStore,
} from "@/stores/org-designer-store";

import { AbandonOrgDraftDialog, OrgStructureDraftDialog } from "./OrgStructureDraftDialog";

export function OrgDesignerLifecycleBar() {
  const { t } = useTranslation("org");
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const workspaceMode = useOrgDesignerStore((s) => s.workspaceMode);
  const setWorkspaceMode = useOrgDesignerStore((s) => s.setWorkspaceMode);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  const [bootstrapOpen, setBootstrapOpen] = useState(false);
  const [forkOpen, setForkOpen] = useState(false);
  const [abandonOpen, setAbandonOpen] = useState(false);

  const hasActive = snapshot?.active_model_id != null;
  const hasDraft = snapshot?.draft_model_id != null;
  const activeVersion = snapshot?.active_model_version;
  const draftVersion = snapshot?.draft_model_version;

  const isDesign = isOrgStructureDesignMode(snapshot, workspaceMode);
  const canUseModeSwitch = hasActive && hasDraft;

  const onWorkspaceChange = (mode: OrgDesignerWorkspaceMode) => {
    setWorkspaceMode(mode);
  };

  const afterDraftMutation = () => {
    void loadSnapshot();
  };

  return (
    <div className="mx-6 mt-3 space-y-2 shrink-0">
      {/* Mode switch — only when both published and a draft exist */}
      {canUseModeSwitch && (
        <div
          className="flex flex-wrap items-center gap-2 rounded-lg border border-surface-border bg-surface-1/50 p-2"
          role="group"
          aria-label={t("lifecycle.modeGroupLabel")}
        >
          <span className="text-xs text-text-muted px-1 shrink-0">{t("lifecycle.modeLabel")}:</span>
          <div className="inline-flex rounded-md border border-surface-border bg-surface-0 p-0.5">
            <button
              type="button"
              onClick={() => onWorkspaceChange("published")}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-sm px-3 py-1.5 text-xs font-medium transition-colors",
                workspaceMode === "published"
                  ? "bg-primary text-primary-foreground"
                  : "text-text-muted hover:text-text-primary",
              )}
            >
              <BookOpen className="h-3.5 w-3.5" />
              {t("lifecycle.modePublished")}
            </button>
            <button
              type="button"
              onClick={() => onWorkspaceChange("draft")}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-sm px-3 py-1.5 text-xs font-medium transition-colors",
                workspaceMode === "draft"
                  ? "bg-primary text-primary-foreground"
                  : "text-text-muted hover:text-text-primary",
              )}
            >
              <PenLine className="h-3.5 w-3.5" />
              {t("lifecycle.modeDraft", { version: draftVersion ?? "—" })}
            </button>
          </div>
          {hasActive && (
            <Badge variant="outline" className="text-[10px]">
              {t("lifecycle.publishedVersionBadge", { version: activeVersion ?? "—" })}
            </Badge>
          )}
          {hasDraft && (
            <Badge variant="secondary" className="text-[10px]">
              {t("lifecycle.draftVersionBadge", { version: draftVersion ?? "—" })}
            </Badge>
          )}
        </div>
      )}

      {/* Descriptive read-only / design status */}
      {isDesign && hasActive && (
        <div
          className="rounded-md border border-primary/20 bg-primary/5 px-3 py-2 text-xs text-text-primary"
          role="status"
        >
          {t("lifecycle.draftModeBanner")}
        </div>
      )}

      {hasDraft && !hasActive && (
        <div
          className="rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-xs text-text-primary"
          role="status"
        >
          {t("lifecycle.prePublishDraftOnly")}
        </div>
      )}

      {!isDesign && hasDraft && (
        <div className="rounded-md border border-surface-border bg-surface-0 px-3 py-2 text-xs text-text-muted">
          {t("lifecycle.publishedViewBanner")}
        </div>
      )}

      {!hasActive && !hasDraft && (
        <div className="rounded-md border border-status-warning/30 bg-status-warning/10 p-3 flex flex-wrap items-center justify-between gap-2">
          <p className="text-sm text-text-primary">{t("lifecycle.noModelYet")}</p>
          <PermissionGate permission="org.admin">
            <Button size="sm" className="gap-1.5" onClick={() => setBootstrapOpen(true)}>
              <Plus className="h-3.5 w-3.5" />
              {t("lifecycle.createInitialDraft")}
            </Button>
          </PermissionGate>
        </div>
      )}

      {hasActive && !hasDraft && (
        <div className="rounded-md border border-surface-border bg-surface-0 p-3 flex flex-wrap items-center justify-between gap-2">
          <p className="text-sm text-text-primary max-w-prose">{t("lifecycle.publishedNoDraft")}</p>
          <PermissionGate permission="org.admin">
            <Button size="sm" className="gap-1.5" onClick={() => setForkOpen(true)}>
              <Plus className="h-3.5 w-3.5" />
              {t("lifecycle.startVersionFromPublished")}
            </Button>
          </PermissionGate>
        </div>
      )}

      {hasDraft && isDesign && (
        <div className="flex flex-wrap gap-2 justify-end">
          <PermissionGate permission="org.admin">
            <Button size="sm" variant="outline" onClick={() => setAbandonOpen(true)}>
              {t("lifecycle.abandonDraft")}
            </Button>
          </PermissionGate>
        </div>
      )}

      <OrgStructureDraftDialog
        open={bootstrapOpen}
        onOpenChange={setBootstrapOpen}
        variant="bootstrap"
        onSuccess={afterDraftMutation}
      />
      <OrgStructureDraftDialog
        open={forkOpen}
        onOpenChange={setForkOpen}
        variant="fork"
        onSuccess={afterDraftMutation}
      />
      {snapshot?.draft_model_id != null && (
        <AbandonOrgDraftDialog
          open={abandonOpen}
          onOpenChange={setAbandonOpen}
          draftModelId={snapshot.draft_model_id}
          onSuccess={afterDraftMutation}
        />
      )}
    </div>
  );
}
