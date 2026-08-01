/**
 * ImpactPreviewDrawer.tsx
 *
 * Side sheet that displays the impact preview before a structural change
 * (move, deactivate, reassign). Blockers disable the confirm button;
 * warnings require explicit acknowledgement before proceeding.
 */

import {
  AlertTriangle,
  Ban,
  Check,
  ExternalLink,
  GitBranch,
  Loader2,
  Link2,
  ShieldAlert,
  Users,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useStepUp } from "@/hooks/use-step-up";
import { formatOrgValidationIssue } from "@/lib/format-org-validation-issue";
import { deactivateOrgNode, moveOrgNode } from "@/services/org-node-service";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgNodeStore } from "@/stores/org-node-store";
import { formatOrgIpcError } from "@/utils/errors";

export function ImpactPreviewDrawer() {
  const { t } = useTranslation("org");
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const preview = useOrgDesignerStore((s) => s.preview);
  const previewPayload = useOrgDesignerStore((s) => s.previewPayload);
  const previewOpen = useOrgDesignerStore((s) => s.previewOpen);
  const previewLoading = useOrgDesignerStore((s) => s.previewLoading);
  const closePreview = useOrgDesignerStore((s) => s.closePreview);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);
  const selectedNode = useOrgNodeStore((s) => s.selectedNode);
  const refreshSelectedNodeContext = useOrgNodeStore((s) => s.refreshSelectedNodeContext);

  const [warningsAcknowledged, setWarningsAcknowledged] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [confirmError, setConfirmError] = useState<string | null>(null);

  const hasBlockers = (preview?.blockers.length ?? 0) > 0;
  const hasWarnings = (preview?.warnings.length ?? 0) > 0;
  const confirmDisabled = hasBlockers || (hasWarnings && !warningsAcknowledged) || confirming;

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      closePreview();
      setWarningsAcknowledged(false);
      setConfirmError(null);
    }
  };

  const handleConfirm = async () => {
    if (!preview || !previewPayload || hasBlockers) return;
    setConfirmError(null);
    setConfirming(true);
    try {
      if (preview.action === "MoveNode") {
        const newParentId = previewPayload.new_parent_id;
        if (newParentId == null) {
          setConfirmError(t("preview.moveRequiresParent"));
          return;
        }
        const rowVersion = selectedNode?.row_version;
        if (rowVersion == null || selectedNode?.id !== preview.subject_node_id) {
          setConfirmError(t("preview.staleNodeContext"));
          return;
        }
        await withStepUp(() =>
          moveOrgNode({
            node_id: preview.subject_node_id,
            new_parent_id: newParentId,
            expected_row_version: rowVersion,
          }),
        );
      } else if (preview.action === "DeactivateNode") {
        const rowVersion = selectedNode?.row_version;
        if (rowVersion == null || selectedNode?.id !== preview.subject_node_id) {
          setConfirmError(t("preview.staleNodeContext"));
          return;
        }
        await withStepUp(() => deactivateOrgNode(preview.subject_node_id, rowVersion));
      } else {
        setConfirmError(t("preview.actionNotSupported"));
        return;
      }

      closePreview();
      setWarningsAcknowledged(false);
      void loadSnapshot();
      void refreshSelectedNodeContext();
    } catch (err) {
      setConfirmError(formatOrgIpcError(err));
    } finally {
      setConfirming(false);
    }
  };

  const actionLabel = preview ? t(`preview.actionLabel.${preview.action}`) : "";

  return (
    <Sheet open={previewOpen} onOpenChange={handleOpenChange}>
      <SheetContent side="right" className="flex flex-col sm:max-w-md">
        <SheetHeader>
          <SheetTitle>{t("preview.title")}</SheetTitle>
          <SheetDescription>{actionLabel}</SheetDescription>
        </SheetHeader>

        {previewLoading && (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
          </div>
        )}

        {preview && !previewLoading && (
          <div className="flex-1 overflow-y-auto space-y-5 py-2">
            <section className="space-y-2">
              <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("preview.impactSummary")}
              </h4>
              <div className="grid grid-cols-2 gap-2">
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.affectedNodes")}
                  value={preview.affected_node_count}
                />
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.descendants")}
                  value={preview.descendant_count}
                />
                <SummaryCard
                  icon={<Users className="h-4 w-4" />}
                  label={t("preview.responsibilities")}
                  value={preview.active_responsibility_count}
                />
                <SummaryCard
                  icon={<Link2 className="h-4 w-4" />}
                  label={t("preview.activeBindings")}
                  value={preview.active_binding_count}
                />
              </div>
            </section>

            {hasBlockers && (
              <section className="space-y-2">
                <h4 className="text-xs font-semibold text-status-danger uppercase tracking-wider flex items-center gap-1.5">
                  <Ban className="h-3.5 w-3.5" />
                  {t("preview.blockers")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.blockers")}>
                  {preview.blockers.map((issue, i) => (
                    <li
                      key={i}
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
                  <AlertTriangle className="h-3.5 w-3.5" />
                  {t("preview.warnings")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.warnings")}>
                  {preview.warnings.map((issue, i) => (
                    <li
                      key={i}
                      className="rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm text-text-primary"
                    >
                      {formatOrgValidationIssue(issue, t)}
                    </li>
                  ))}
                </ul>

                {!hasBlockers && (
                  <label className="flex items-center gap-2 cursor-pointer pt-1">
                    <input
                      type="checkbox"
                      checked={warningsAcknowledged}
                      onChange={(e) => setWarningsAcknowledged(e.target.checked)}
                      className="rounded border-surface-border"
                    />
                    <span className="text-xs text-text-muted">
                      {t("preview.acknowledgeWarnings")}
                    </span>
                  </label>
                )}
              </section>
            )}

            <Separator />

            <section className="space-y-2">
              <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider flex items-center gap-1.5">
                <ExternalLink className="h-3.5 w-3.5" />
                {t("preview.dependencies")}
              </h4>
              <div className="space-y-1.5">
                {preview.dependencies.map((dep) => (
                  <div
                    key={dep.domain}
                    className="flex items-center justify-between rounded-md border border-surface-border px-3 py-2"
                  >
                    <div className="space-y-0.5">
                      <span className="text-sm text-text-primary capitalize">
                        {t(`preview.dependencyDomain.${dep.domain}`, {
                          defaultValue: dep.domain.replace(/_/g, " "),
                        })}
                      </span>
                      {(dep.note || dep.status === "unavailable") && (
                        <p className="text-[11px] text-text-muted">
                          {t(`preview.dependencyNote.${dep.domain}`, {
                            defaultValue:
                              dep.note ??
                              t("preview.dependencyUnavailable", {
                                defaultValue: "Impact details for this area are not available yet.",
                              }),
                          })}
                        </p>
                      )}
                    </div>
                    <Badge
                      variant={dep.status === "unavailable" ? "outline" : "secondary"}
                      className="text-[10px] shrink-0"
                    >
                      {t(`preview.dependencyStatus.${dep.status}`, {
                        defaultValue: dep.status,
                      })}
                    </Badge>
                  </div>
                ))}
              </div>
            </section>

            {confirmError && (
              <p className="text-xs text-status-danger" role="alert">
                {confirmError}
              </p>
            )}
          </div>
        )}

        {preview && !previewLoading && (
          <SheetFooter className="border-t border-surface-border pt-4">
            <Button variant="outline" onClick={() => handleOpenChange(false)}>
              {t("preview.cancel")}
            </Button>
            <Button
              disabled={confirmDisabled}
              onClick={() => void handleConfirm()}
              className="gap-1.5"
              variant={hasBlockers ? "outline" : "default"}
            >
              {confirming ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : hasBlockers ? (
                <>
                  <ShieldAlert className="h-3.5 w-3.5" />
                  {t("preview.blocked")}
                </>
              ) : (
                <>
                  <Check className="h-3.5 w-3.5" />
                  {t("preview.confirm")}
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
  value: number;
}) {
  return (
    <div className="rounded-lg border border-surface-border p-3 space-y-1">
      <div className="flex items-center gap-1.5 text-text-muted">
        {icon}
        <span className="text-[11px]">{label}</span>
      </div>
      <p className="text-lg font-semibold text-text-primary">{value}</p>
    </div>
  );
}
