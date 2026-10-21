/**
 * PublishReadinessPanel.tsx
 *
 * Banner at the top of ReferenceValueEditor for a candidate version (draft or validated) of a
 * controlled catalogue. Shows what still blocks publication, the impact on records, and runs the
 * governed publish: validate the draft (validation engine) → impact preview → publish.
 */

import {
  AlertTriangle,
  CheckCircle2,
  ClipboardCheck,
  Eye,
  Send,
  Shield,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import type {
  ReferenceImpactSummary,
  ReferencePublishIssue,
  ReferencePublishResult,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/**
 * Readiness checks the publish action itself takes care of: a draft is validated again by the
 * action (so an older validation report does not count), and the impact preview is computed when
 * the confirmation opens. A failed validation is reported with each problem by the action.
 */
const CHECKS_SATISFIED_BY_ACTION: Record<string, ReadonlySet<string>> = {
  draft: new Set([
    "set_status",
    "validation_report_missing",
    "validation_blockers",
    "validation_failed",
    "impact_preview_required",
  ]),
  validated: new Set(["impact_preview_required"]),
};

/** Issues that still need the user, for a candidate in `setStatus`. */
export function outstandingPublishIssues(
  issues: readonly ReferencePublishIssue[],
  setStatus: string,
): ReferencePublishIssue[] {
  const satisfied = CHECKS_SATISFIED_BY_ACTION[setStatus] ?? new Set<string>();
  return issues.filter((i) => !satisfied.has(i.check));
}

interface PublishReadinessPanelProps {
  setId: number;
  /** `draft` or `validated`. */
  setStatus: string;
  isProtected: boolean;
  /** Called with the publish result once the version is live. */
  onPublished: (result: ReferencePublishResult) => void;
}

export function PublishReadinessPanel({
  setId,
  setStatus,
  isProtected,
  onPublished,
}: PublishReadinessPanelProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const mayPublish = can(P.REF_PUBLISH);

  const readiness = useReferenceGovernanceStore((s) => s.readiness);
  const readinessLoading = useReferenceGovernanceStore((s) => s.readinessLoading);
  const readinessError = useReferenceGovernanceStore((s) => s.readinessError);
  const impactSummary = useReferenceGovernanceStore((s) => s.impactSummary);
  const impactLoading = useReferenceGovernanceStore((s) => s.impactLoading);
  const loadReadiness = useReferenceGovernanceStore((s) => s.loadReadiness);
  const loadImpact = useReferenceGovernanceStore((s) => s.loadImpact);
  const validateAndPublish = useReferenceGovernanceStore((s) => s.validateAndPublish);

  const [impactOpen, setImpactOpen] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  useEffect(() => {
    void loadReadiness(setId);
  }, [setId, setStatus, loadReadiness]);

  const currentReadiness = readiness?.set_id === setId ? readiness : null;
  const outstanding = currentReadiness
    ? outstandingPublishIssues(currentReadiness.issues, setStatus)
    : [];
  const blockers = outstanding.filter((i) => i.severity === "blocker");
  const warnings = outstanding.filter((i) => i.severity !== "blocker");
  const ready = currentReadiness != null && blockers.length === 0;
  const currentImpact = impactSummary?.set_id === setId ? impactSummary : null;

  const issueText = (issue: ReferencePublishIssue) =>
    t(`publish.check.${issue.check}`, { defaultValue: issue.message });

  const openImpact = useCallback(() => {
    void loadImpact(setId);
    setImpactOpen(true);
  }, [loadImpact, setId]);

  const openConfirm = useCallback(() => {
    void loadImpact(setId);
    setConfirmOpen(true);
  }, [loadImpact, setId]);

  const runPublish = useCallback(async () => {
    const result = await validateAndPublish(setId, setStatus);
    onPublished(result);
  }, [validateAndPublish, setId, setStatus, onPublished]);

  const publishLabel =
    setStatus === "draft" ? t("publish.validateAndPublish") : t("publish.publishSet");

  return (
    <>
      <section
        aria-label={t("publish.title")}
        className="border-b border-surface-border bg-surface-0 px-4 py-3"
      >
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <ClipboardCheck className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
            <h2 className="text-sm font-medium text-text-primary">{t("publish.title")}</h2>
            {readinessLoading && !currentReadiness ? null : ready ? (
              <StatusChip label={t("publish.ready")} tone="success" icon={CheckCircle2} />
            ) : currentReadiness ? (
              <StatusChip
                label={t("publish.notReady", { count: blockers.length })}
                tone="danger"
                icon={XCircle}
              />
            ) : null}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11 gap-1.5"
              onClick={openImpact}
              disabled={impactLoading}
            >
              <Eye className="h-4 w-4" aria-hidden />
              {t("publish.previewImpact")}
            </Button>
            {mayPublish ? (
              <Button
                type="button"
                className="h-11 gap-1.5"
                onClick={openConfirm}
                disabled={!ready || readinessLoading}
              >
                {isProtected ? <Shield className="h-4 w-4" aria-hidden /> : null}
                <Send className="h-4 w-4" aria-hidden />
                {publishLabel}
              </Button>
            ) : null}
          </div>
        </div>

        <p className="mt-1 text-xs text-text-muted">
          {setStatus === "draft" ? t("publish.draftHint") : t("publish.validatedHint")}
        </p>
        {!mayPublish ? (
          <p className="mt-1 text-xs text-text-secondary">{t("publish.noPublishPermission")}</p>
        ) : null}
        {!readinessLoading && currentReadiness == null && readinessError?.setId === setId ? (
          <ErrorState
            className="mt-2 py-4"
            error={readinessError.error}
            onRetry={() => void loadReadiness(setId)}
          />
        ) : null}

        {blockers.length > 0 ? (
          <ul className="mt-2 space-y-1" aria-label={t("publish.blockers")}>
            {blockers.map((issue) => (
              <li
                key={issue.check}
                className={cn("flex items-start gap-1.5 text-xs", mfTone.danger.text)}
              >
                <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
                <span>{issueText(issue)}</span>
              </li>
            ))}
          </ul>
        ) : null}
        {warnings.length > 0 ? (
          <ul className="mt-2 space-y-1" aria-label={t("publish.warnings")}>
            {warnings.map((issue) => (
              <li
                key={issue.check}
                className={cn("flex items-start gap-1.5 text-xs", mfTone.warning.text)}
              >
                <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
                <span>{issueText(issue)}</span>
              </li>
            ))}
          </ul>
        ) : null}
      </section>

      {/* Impact preview (read-only) */}
      <Dialog open={impactOpen} onOpenChange={setImpactOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("publish.impactTitle")}</DialogTitle>
            <DialogDescription>{t("publish.impactDescription")}</DialogDescription>
          </DialogHeader>
          <ImpactSummaryView summary={currentImpact} loading={impactLoading} />
          <DialogFooter>
            <Button variant="outline" className="h-11" onClick={() => setImpactOpen(false)}>
              {t("publish.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Publish confirmation with the impact of this version */}
      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t("publish.confirmTitle")}
        description={isProtected ? t("publish.confirmProtected") : t("publish.confirmDescription")}
        confirmLabel={publishLabel}
        confirmDisabled={impactLoading || currentImpact == null}
        onConfirm={runPublish}
      >
        <ImpactSummaryView summary={currentImpact} loading={impactLoading} />
      </ConfirmDialog>
    </>
  );
}

function ImpactSummaryView({
  summary,
  loading,
}: {
  summary: ReferenceImpactSummary | null;
  loading: boolean;
}) {
  const { t } = useTranslation("reference");
  if (loading) return <LoadingState label={t("publish.impactLoading")} className="py-6" />;
  if (!summary) return <p className="text-sm text-text-muted">{t("publish.noImpactData")}</p>;

  const affected = summary.dimensions.filter((d) => d.affected_count > 0);
  const unverified = summary.dimensions.filter((d) => d.status === "unavailable");
  return (
    <div className="space-y-3 text-sm">
      <p>
        {summary.total_affected > 0
          ? t("publish.totalAffected", { count: summary.total_affected })
          : t("publish.noRecordAffected")}
      </p>
      {summary.removed_codes.length > 0 ? (
        <p className="text-xs text-text-secondary">
          {t("publish.removedCodes", { codes: summary.removed_codes.join(", ") })}
        </p>
      ) : null}
      {summary.deactivated_codes.length > 0 ? (
        <p className="text-xs text-text-secondary">
          {t("publish.deactivatedCodes", { codes: summary.deactivated_codes.join(", ") })}
        </p>
      ) : null}
      {affected.length > 0 ? (
        <ul className="space-y-1">
          {affected.map((dim) => (
            <li
              key={dim.module}
              className="flex items-center justify-between border-b border-surface-border pb-1"
            >
              <span className="text-text-secondary">
                {t(`publish.module.${dim.module}`, { defaultValue: t("publish.module.other") })}
              </span>
              <span className="font-mono tabular-nums">{dim.affected_count}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {unverified.length > 0 ? (
        <p className="text-xs text-text-muted">
          {t("publish.unverifiedModules", {
            modules: unverified
              .map((d) =>
                t(`publish.module.${d.module}`, { defaultValue: t("publish.module.other") }),
              )
              .join(", "),
          })}
        </p>
      ) : null}
      <p className="text-xs text-text-muted">{t("publish.remapHint")}</p>
    </div>
  );
}
