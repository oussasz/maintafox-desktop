/**
 * Supervisor review of a completed round (PRD §6.25 "Reviewed state"): every open anomaly gets
 * a decision (request, work order or deferral) before the round can be marked reviewed. The
 * decided findings stay listed with a link to the request or work order created from them.
 */
import { ClipboardList, FileWarning, Hammer, Loader2, PauseCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Button, Label, Textarea } from "@/components/ui";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  deferInspectionAnomaly,
  listInspectionAnomalyRows,
  reviewInspectionRound,
} from "@/services/inspection-service";
import type {
  InspectionAnomaly,
  InspectionAnomalyRow,
  InspectionRoundRunner,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  type FollowUpCreated,
  FollowUpCreatedNotice,
  type FollowUpKind,
  InspectionFollowUpDialog,
} from "./InspectionFollowUpDialog";
import { ANOMALY_STATUS_TONE } from "./inspection-rules";

interface InspectionRoundReviewProps {
  runner: InspectionRoundRunner;
  onChanged: () => Promise<void>;
}

interface OpenFinding {
  anomaly: InspectionAnomaly;
  label: string;
}

export function InspectionRoundReview({ runner, onChanged }: InspectionRoundReviewProps) {
  const { t } = useTranslation("inspections");
  const { can, canAny } = usePermissions();
  const { formatDateTime } = useFormatters();
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [followUp, setFollowUp] = useState<{ id: number; kind: FollowUpKind } | null>(null);
  const [created, setCreated] = useState<FollowUpCreated | null>(null);
  const [deferTarget, setDeferTarget] = useState<InspectionAnomaly | null>(null);
  const [decided, setDecided] = useState<InspectionAnomalyRow[]>([]);
  /** The decided findings could not be read: said so, never shown as "none decided". */
  const [decidedError, setDecidedError] = useState<unknown>(null);
  const round = runner.summary.round;

  const loadDecided = useCallback(async () => {
    try {
      const page = await listInspectionAnomalyRows({
        round_id: round.id,
        statuses: ["triaged", "deferred", "resolved"],
      });
      setDecided(page.items);
      setDecidedError(null);
    } catch (err) {
      // The open findings stay usable; the decided list says it could not be read.
      setDecidedError(err);
    }
  }, [round.id]);

  useEffect(() => {
    void loadDecided();
  }, [loadDecided, round.row_version]);

  const findings: OpenFinding[] = runner.checkpoints.flatMap((c) =>
    c.open_anomalies.map((anomaly) => ({
      anomaly,
      label: c.checkpoint.description ?? c.checkpoint.checkpoint_code,
    })),
  );
  const canDecide = can(P.INS_EXECUTE);
  const canDi = canDecide && canAny(P.DI_CREATE, P.DI_CREATE_OWN);
  const canWo = canDecide && can(P.OT_CREATE);
  const canReview = can(P.INS_ADMIN);

  const refresh = async () => {
    await onChanged();
    await loadDecided();
  };

  const review = async () => {
    setBusy(true);
    try {
      await reviewInspectionRound({
        round_id: round.id,
        expected_row_version: round.row_version,
        note: note.trim() || null,
      });
      await refresh();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className="space-y-4 rounded-lg border border-surface-border bg-surface-1 p-4"
      aria-label={t("review.title")}
    >
      <h3 className="flex items-center gap-2 text-base font-semibold">
        <ClipboardList className="h-5 w-5" aria-hidden />
        {t("review.title")}
      </h3>

      {created ? <FollowUpCreatedNotice created={created} /> : null}

      {round.status === "reviewed" ? (
        <p className="text-sm text-text-muted">
          {t("review.reviewedBy", {
            name: runner.summary.reviewed_by_name ?? "—",
            date: formatDateTime(round.reviewed_at),
          })}
          {round.review_note ? ` — ${round.review_note}` : ""}
        </p>
      ) : findings.length === 0 ? (
        <p className="text-sm text-text-muted">{t("review.none")}</p>
      ) : (
        <ul className="space-y-3">
          {findings.map(({ anomaly, label }) => (
            <li
              key={anomaly.id}
              className="space-y-2 rounded-md border border-tone-attention-border p-3"
              data-testid="review-finding"
            >
              <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                <FileWarning className="h-4 w-4 text-tone-attention-fg" aria-hidden />
                {label} —{" "}
                {t(`review.anomalyType.${anomaly.anomaly_type}`, {
                  defaultValue: t("anomalies.otherType"),
                })}
                <span className="text-text-muted">
                  ({t("review.severity", { level: anomaly.severity })})
                </span>
              </p>
              {canDecide ? (
                <div className="flex flex-wrap gap-2">
                  {canDi ? (
                    <Button
                      variant="outline"
                      className="h-12"
                      disabled={busy}
                      onClick={() => {
                        setCreated(null);
                        setFollowUp({ id: anomaly.id, kind: "di" });
                      }}
                    >
                      {t("review.toDi")}
                    </Button>
                  ) : null}
                  {canWo ? (
                    <Button
                      variant="outline"
                      className="h-12"
                      disabled={busy}
                      onClick={() => {
                        setCreated(null);
                        setFollowUp({ id: anomaly.id, kind: "wo" });
                      }}
                    >
                      <Hammer className="mr-2 h-4 w-4" aria-hidden />
                      {t("review.toWo")}
                    </Button>
                  ) : null}
                  <Button
                    variant="ghost"
                    className="h-12"
                    disabled={busy}
                    onClick={() => setDeferTarget(anomaly)}
                  >
                    <PauseCircle className="mr-2 h-4 w-4" aria-hidden />
                    {t("review.defer")}
                  </Button>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      {decidedError != null ? (
        <ErrorState
          className="py-4"
          title={t("review.decidedLoadError")}
          error={decidedError}
          onRetry={() => void loadDecided()}
        />
      ) : null}
      {decided.length > 0 ? (
        <div className="space-y-2" data-testid="review-decided">
          <h4 className="text-sm font-semibold text-text-primary">{t("review.decided")}</h4>
          <ul className="space-y-1.5">
            {decided.map((r) => (
              <li key={r.anomaly.id} className="flex flex-wrap items-center gap-2 text-sm">
                <span className="font-mono text-xs">{r.checkpoint_code}</span>
                <span>
                  {t(`review.anomalyType.${r.anomaly.anomaly_type}`, {
                    defaultValue: t("anomalies.otherType"),
                  })}
                </span>
                <StatusChip
                  label={t(`anomalyStatus.${r.anomaly.resolution_status}`)}
                  tone={toneOf(ANOMALY_STATUS_TONE, r.anomaly.resolution_status)}
                />
                <LinkedEntityBadge
                  entity="di"
                  code={r.linked_di_code}
                  entityId={r.anomaly.linked_di_id}
                  title={r.linked_di_title}
                />
                <LinkedEntityBadge
                  entity="work_order"
                  code={r.linked_wo_code}
                  entityId={r.anomaly.linked_work_order_id}
                  title={r.linked_wo_title}
                />
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {canReview && round.status !== "reviewed" ? (
        <div className="space-y-2 border-t border-surface-border pt-4">
          <Label htmlFor="review-note">{t("review.note")}</Label>
          <Textarea
            id="review-note"
            rows={2}
            placeholder={t("review.notePlaceholder")}
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
          <div className="flex flex-wrap items-center gap-3">
            <Button
              className="h-12"
              disabled={busy || findings.length > 0}
              onClick={() => void review()}
            >
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
              {t("review.submit")}
            </Button>
            {findings.length > 0 ? (
              <span className="text-sm text-text-muted">
                {t("review.decideFirst", { count: findings.length })}
              </span>
            ) : null}
          </div>
        </div>
      ) : null}

      <InspectionFollowUpDialog
        anomalyId={followUp?.id ?? null}
        kind={followUp?.kind ?? "di"}
        onClose={() => setFollowUp(null)}
        onCreated={(c) => {
          setFollowUp(null);
          setCreated(c);
          void refresh();
        }}
      />
      <ConfirmDialog
        open={deferTarget != null}
        onOpenChange={(open) => (!open ? setDeferTarget(null) : undefined)}
        title={t("anomalies.deferTitle")}
        description={t("anomalies.deferBody", { checkpoint: "" })}
        confirmLabel={t("review.defer")}
        onConfirm={async () => {
          if (!deferTarget) return;
          await deferInspectionAnomaly({
            anomaly_id: deferTarget.id,
            expected_row_version: deferTarget.row_version,
          });
          await refresh();
        }}
      />
    </section>
  );
}
