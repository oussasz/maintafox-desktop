/**
 * ImpactPreviewDrawer.tsx
 *
 * Side sheet that displays the impact preview before a structural change
 * (move, deactivate, reassign). Blockers disable the confirm button;
 * warnings require explicit acknowledgement before proceeding.
 *
 * Confirm applies the previewed change:
 *  - MoveNode → `move_org_node` (step-up) with the parent chosen in MoveNodeDialog
 *  - DeactivateNode → `deactivate_org_node` (step-up)
 *  - ReassignResponsibility → `assign_org_node_responsibility` with `replace_active`, on the
 *    published unit that carries the operational records.
 * A failed apply is reported by the global IPC toast; the drawer stays open to retry.
 */

import {
  AlertTriangle,
  Ban,
  Check,
  ExternalLink,
  GitBranch,
  Link2,
  ShieldAlert,
  Users,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { LoadingState } from "@/components/ui/data-state";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { StatusChip } from "@/components/ui/status-chip";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { useStepUp } from "@/hooks/use-step-up";
import { formatOrgValidationIssue } from "@/lib/format-org-validation-issue";
import {
  assignOrgNodeResponsibility,
  deactivateOrgNode,
  moveOrgNode,
} from "@/services/org-node-service";
import { operationalNodeIdOf, useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgNodeStore } from "@/stores/org-node-store";

const DEPENDENCY_TONE: Record<string, Tone> = {
  ok: "success",
  impacted: "warning",
  unavailable: "neutral",
};

export function ImpactPreviewDrawer() {
  const { t } = useTranslation("org");
  const { formatNumber } = useFormatters();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const preview = useOrgDesignerStore((s) => s.preview);
  const previewPayload = useOrgDesignerStore((s) => s.previewPayload);
  const previewTargetLabel = useOrgDesignerStore((s) => s.previewTargetLabel);
  const previewOpen = useOrgDesignerStore((s) => s.previewOpen);
  const previewLoading = useOrgDesignerStore((s) => s.previewLoading);
  const closePreview = useOrgDesignerStore((s) => s.closePreview);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const selectedNode = useOrgNodeStore((s) => s.selectedNode);
  const refreshSelectedNodeContext = useOrgNodeStore((s) => s.refreshSelectedNodeContext);

  const [warningsAcknowledged, setWarningsAcknowledged] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [confirmError, setConfirmError] = useState<string | null>(null);

  const hasBlockers = (preview?.blockers.length ?? 0) > 0;
  const hasWarnings = (preview?.warnings.length ?? 0) > 0;
  const confirmDisabled = hasBlockers || (hasWarnings && !warningsAcknowledged) || confirming;

  const subjectRow =
    preview != null
      ? (snapshot?.nodes.find((n) => n.node_id === preview.subject_node_id) ?? null)
      : null;

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      closePreview();
      setWarningsAcknowledged(false);
      setConfirmError(null);
    }
  };

  /** Row version of the subject: the loaded node when it is the subject, else the tree row. */
  const subjectRowVersion = (): number | null => {
    if (!preview) return null;
    if (selectedNode?.id === preview.subject_node_id) return selectedNode.row_version;
    return subjectRow?.row_version ?? null;
  };

  const handleConfirm = async () => {
    if (!preview || !previewPayload || hasBlockers) return;
    setConfirmError(null);

    let apply: (() => Promise<unknown>) | null = null;
    if (preview.action === "MoveNode") {
      const newParentId = previewPayload.new_parent_id;
      const rowVersion = subjectRowVersion();
      if (newParentId == null) {
        setConfirmError(t("preview.moveRequiresParent"));
        return;
      }
      if (rowVersion == null) {
        setConfirmError(t("preview.staleNodeContext"));
        return;
      }
      apply = () =>
        withStepUp(() =>
          moveOrgNode({
            node_id: preview.subject_node_id,
            new_parent_id: newParentId,
            expected_row_version: rowVersion,
          }),
        );
    } else if (preview.action === "DeactivateNode") {
      const rowVersion = subjectRowVersion();
      if (rowVersion == null) {
        setConfirmError(t("preview.staleNodeContext"));
        return;
      }
      apply = () => withStepUp(() => deactivateOrgNode(preview.subject_node_id, rowVersion));
    } else {
      const responsibilityType = previewPayload.responsibility_type?.trim();
      const opsNodeId = operationalNodeIdOf(snapshot, preview.subject_node_id);
      const personId = previewPayload.replacement_person_id ?? null;
      const teamId = previewPayload.replacement_team_id ?? null;
      if (!responsibilityType || (personId == null && teamId == null)) {
        setConfirmError(t("preview.reassignIncomplete"));
        return;
      }
      if (opsNodeId == null) {
        setConfirmError(t("responsibilities.notPublished"));
        return;
      }
      apply = () =>
        assignOrgNodeResponsibility({
          node_id: opsNodeId,
          responsibility_type: responsibilityType,
          person_id: personId,
          team_id: teamId,
          replace_active: true,
        });
    }

    setConfirming(true);
    try {
      await apply();
      closePreview();
      setWarningsAcknowledged(false);
      void loadSnapshot();
      void refreshSelectedNodeContext();
    } catch {
      // The global IPC toast reports the failure; reload so a retry uses fresh versions.
      void loadSnapshot();
      void refreshSelectedNodeContext();
    } finally {
      setConfirming(false);
    }
  };

  const actionLabel = preview ? t(`preview.actionLabel.${preview.action}`) : "";
  const targetSentence =
    preview && subjectRow
      ? preview.action === "MoveNode" && previewTargetLabel
        ? t("preview.target.MoveNode", { unit: subjectRow.name, target: previewTargetLabel })
        : preview.action === "ReassignResponsibility" && previewTargetLabel
          ? t("preview.target.ReassignResponsibility", {
              unit: subjectRow.name,
              target: previewTargetLabel,
            })
          : preview.action === "DeactivateNode"
            ? t("preview.target.DeactivateNode", { unit: subjectRow.name })
            : null
      : null;

  return (
    <Sheet open={previewOpen} onOpenChange={handleOpenChange}>
      <SheetContent side="right" className="flex flex-col sm:max-w-md">
        <SheetHeader>
          <SheetTitle>{t("preview.title")}</SheetTitle>
          <SheetDescription>{actionLabel}</SheetDescription>
        </SheetHeader>

        {previewLoading && <LoadingState className="flex-1" label={t("preview.loading")} />}

        {preview && !previewLoading && (
          <div className="flex-1 overflow-y-auto space-y-5 py-2">
            {targetSentence && (
              <p className="text-sm font-medium text-text-primary">{targetSentence}</p>
            )}
            <section className="space-y-2">
              <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("preview.impactSummary")}
              </h4>
              <div className="grid grid-cols-2 gap-2">
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.affectedNodes")}
                  value={formatNumber(preview.affected_node_count)}
                />
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.descendants")}
                  value={formatNumber(preview.descendant_count)}
                />
                <SummaryCard
                  icon={<Users className="h-4 w-4" />}
                  label={t("preview.responsibilities")}
                  value={formatNumber(preview.active_responsibility_count)}
                />
                <SummaryCard
                  icon={<Link2 className="h-4 w-4" />}
                  label={t("preview.activeBindings")}
                  value={formatNumber(preview.active_binding_count)}
                />
              </div>
            </section>

            {hasBlockers && (
              <section className="space-y-2">
                <h4 className="text-xs font-semibold text-status-danger uppercase tracking-wider flex items-center gap-1.5">
                  <Ban className="h-3.5 w-3.5" aria-hidden />
                  {t("preview.blockers")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.blockers")}>
                  {preview.blockers.map((issue, i) => (
                    <li
                      key={`${issue.code}-${i}`}
                      className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-text-primary"
                    >
                      {formatOrgValidationIssue(issue, t)}
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {hasWarnings && (
              <section className="space-y-2">
                <h4 className="text-xs font-semibold text-status-warning uppercase tracking-wider flex items-center gap-1.5">
                  <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                  {t("preview.warnings")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.warnings")}>
                  {preview.warnings.map((issue, i) => (
                    <li
                      key={`${issue.code}-${i}`}
                      className="rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm text-text-primary"
                    >
                      {formatOrgValidationIssue(issue, t)}
                    </li>
                  ))}
                </ul>

                {!hasBlockers && (
                  <label className="flex min-h-11 cursor-pointer items-center gap-2 pt-1">
                    <input
                      type="checkbox"
                      checked={warningsAcknowledged}
                      onChange={(e) => setWarningsAcknowledged(e.target.checked)}
                      className="h-4 w-4 rounded border-surface-border"
                    />
                    <span className="text-sm text-text-secondary">
                      {t("preview.acknowledgeWarnings")}
                    </span>
                  </label>
                )}
              </section>
            )}

            {preview.dependencies.length > 0 && (
              <>
                <Separator />
                <section className="space-y-2">
                  <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider flex items-center gap-1.5">
                    <ExternalLink className="h-3.5 w-3.5" aria-hidden />
                    {t("preview.dependencies")}
                  </h4>
                  <div className="space-y-1.5">
                    {preview.dependencies.map((dep) => (
                      <div
                        key={dep.domain}
                        className="flex items-center justify-between gap-2 rounded-md border border-surface-border px-3 py-2"
                      >
                        <span className="text-sm text-text-primary">
                          {t(`preview.dependencyDomain.${dep.domain}`)}
                          {dep.count != null && (
                            <span className="ml-2 font-semibold tabular-nums">
                              {formatNumber(dep.count)}
                            </span>
                          )}
                        </span>
                        <StatusChip
                          label={t(`preview.dependencyStatus.${dep.status}`)}
                          tone={DEPENDENCY_TONE[dep.status] ?? "neutral"}
                        />
                      </div>
                    ))}
                  </div>
                  <p className="text-xs text-text-muted">{t("preview.dependenciesHint")}</p>
                </section>
              </>
            )}

            {confirmError && (
              <p className="text-sm text-status-danger" role="alert">
                {confirmError}
              </p>
            )}
          </div>
        )}

        {preview && !previewLoading && (
          <SheetFooter className="border-t border-surface-border pt-4 gap-2">
            <Button variant="outline" className="h-11" onClick={() => handleOpenChange(false)}>
              {t("preview.cancel")}
            </Button>
            <Button
              disabled={confirmDisabled}
              onClick={() => void handleConfirm()}
              className="h-11 gap-1.5"
              variant={hasBlockers ? "outline" : "default"}
            >
              {hasBlockers ? (
                <>
                  <ShieldAlert className="h-3.5 w-3.5" aria-hidden />
                  {t("preview.blocked")}
                </>
              ) : (
                <>
                  <Check className="h-3.5 w-3.5" aria-hidden />
                  {confirming
                    ? t("preview.applying")
                    : t(`preview.confirmAction.${preview.action}`)}
                </>
              )}
            </Button>
          </SheetFooter>
        )}
        {StepUpDialogElement}
      </SheetContent>
    </Sheet>
  );
}

function SummaryCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-lg border border-surface-border p-3 space-y-1">
      <div className="flex items-center gap-1.5 text-text-muted">
        {icon}
        <span className="text-xs">{label}</span>
      </div>
      <p className="text-lg font-semibold text-text-primary tabular-nums">{value}</p>
    </div>
  );
}
