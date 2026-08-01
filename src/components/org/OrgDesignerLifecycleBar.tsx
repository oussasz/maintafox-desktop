/**
 * Governs the organization designer “view published” vs “edit draft” mode and
 * the explicit actions to create, fork, abandon, or publish structure drafts.
 */

import { BookOpen, Loader2, PenLine, Plus, Upload } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { useStepUp } from "@/hooks/use-step-up";
import { cn } from "@/lib/utils";
import {
  isOrgStructureDesignMode,
  type OrgDesignerWorkspaceMode,
  useOrgDesignerStore,
} from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import { P } from "@shared/rbac/permissions.generated";

import { AbandonOrgDraftDialog, OrgStructureDraftDialog } from "./OrgStructureDraftDialog";

function AbandonDraftButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation("org");
  return (
    <PermissionGate permission={P.ORG_ADMIN}>
      <Button
        size="sm"
        variant="outline"
        onClick={onClick}
        className="text-status-danger border-status-danger/50 hover:bg-status-danger/10 hover:text-status-danger"
      >
        {t("lifecycle.abandonDraft")}
      </Button>
    </PermissionGate>
  );
}

function PublishDraftButton({ draftModelId }: { draftModelId: number }) {
  const { t } = useTranslation("org");
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const validation = useOrgGovernanceStore((s) => s.publishValidation);
  const validationLoading = useOrgGovernanceStore((s) => s.validationLoading);
  const storeError = useOrgGovernanceStore((s) => s.error);
  const publishModel = useOrgGovernanceStore((s) => s.publishModel);
  const loadAuditEvents = useOrgGovernanceStore((s) => s.loadAuditEvents);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  const canPublish = validation?.can_publish === true && !storeError && !validationLoading;

  const handlePublish = async () => {
    try {
      // Backend `publish_org_model` requires step-up; without withStepUp the
      // STEP_UP_REQUIRED error is silenced (no toast) and nothing appears to happen.
      await withStepUp(() => publishModel(draftModelId));
      void loadSnapshot();
      void loadAuditEvents();
    } catch {
      // Cancelled step-up or non-step-up failure already reflected in store error/banner.
    }
  };

  return (
    <>
      <Button
        size="sm"
        disabled={!canPublish}
        onClick={() => void handlePublish()}
        className="gap-1.5"
        data-testid="publish-button"
      >
        {validationLoading ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Upload className="h-3.5 w-3.5" />
        )}
        {t("governance.publish")}
      </Button>
      {StepUpDialogElement}
    </>
  );
}

function DraftPrimaryActions({
  draftModelId,
  onAbandon,
}: {
  draftModelId: number;
  onAbandon: () => void;
}) {
  return (
    <div className="ml-auto flex items-center gap-2 shrink-0">
      <AbandonDraftButton onClick={onAbandon} />
      <PublishDraftButton draftModelId={draftModelId} />
    </div>
  );
}

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
  const draftModelId = snapshot?.draft_model_id ?? null;
  const draftVersion = snapshot?.draft_model_version;

  const isDesign = isOrgStructureDesignMode(snapshot, workspaceMode);
  const canUseModeSwitch = hasActive && hasDraft;
  const showDraftActions = hasDraft && isDesign && draftModelId != null;

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
          {showDraftActions && draftModelId != null && (
            <DraftPrimaryActions
              draftModelId={draftModelId}
              onAbandon={() => setAbandonOpen(true)}
            />
          )}
        </div>
      )}

      {hasDraft && !hasActive && showDraftActions && draftModelId != null && (
        <div className="flex flex-wrap justify-end gap-2">
          <DraftPrimaryActions draftModelId={draftModelId} onAbandon={() => setAbandonOpen(true)} />
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
          <PermissionGate permission={P.ORG_ADMIN}>
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
          <PermissionGate permission={P.ORG_ADMIN}>
            <Button size="sm" className="gap-1.5" onClick={() => setForkOpen(true)}>
              <Plus className="h-3.5 w-3.5" />
              {t("lifecycle.startVersionFromPublished")}
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
