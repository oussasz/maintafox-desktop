/**
 * Variance reviews: why a cost centre × category is off budget (governed reason codes), who acts
 * and the review workflow open → in review → actioned → accepted → closed (acceptance and
 * closure need `fin.approve` + step-up; any later stage can be reopened with a reason).
 * Reviews are opened from the cockpit with the variance and the figures at that moment.
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { REVIEW_STATUS_TONE, VARIANCE_DRIVERS } from "@/components/finance/finance-tone";
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
import { EmptyState } from "@/components/ui/empty-state";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { useStepUp } from "@/hooks/use-step-up";
import {
  createBudgetVarianceReview,
  listBudgetVarianceReviews,
  transitionBudgetVarianceReview,
} from "@/services/budget-service";
import type { BudgetCockpit, BudgetCockpitRow } from "@/services/finance-cost-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { BudgetVarianceReview } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Forward step of a review (backend `valid_variance_transition`). */
export const NEXT_REVIEW_STATUS: Record<string, string | undefined> = {
  open: "in_review",
  in_review: "actioned",
  actioned: "accepted",
  accepted: "closed",
};
const APPROVAL_STEPS = new Set(["accepted", "closed"]);

/** Variance as a share of budget, signed like the variance (negative = overrun). */
export function variancePct(row: Pick<BudgetCockpitRow, "variance" | "budget">): number {
  return row.budget > 0 ? (row.variance / row.budget) * 100 : 0;
}

export interface VarianceReviewsPanelProps {
  budgetVersionId: number | null;
  currency: string | null;
  reloadToken?: number;
}

export function VarianceReviewsPanel({
  budgetVersionId,
  currency,
  reloadToken = 0,
}: VarianceReviewsPanelProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatPercent, formatDate } = useFormatters();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [reviews, setReviews] = useState<BudgetVarianceReview[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState<{ review: BudgetVarianceReview; next: string } | null>(null);
  const [comment, setComment] = useState("");

  const load = useCallback(async () => {
    if (budgetVersionId == null) {
      setReviews([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setReviews(await listBudgetVarianceReviews({ budget_version_id: budgetVersionId }));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [budgetVersionId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken forces a refresh
  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const canAct = (next: string) =>
    APPROVAL_STEPS.has(next) ? can(P.FIN_APPROVE) : can(P.FIN_BUDGET) || can(P.FIN_MANAGE);

  const confirm = async () => {
    if (!acting) return;
    const { review, next } = acting;
    const run = () =>
      transitionBudgetVarianceReview({
        review_id: review.id,
        expected_row_version: review.row_version,
        next_status: next,
        review_commentary: next === "open" ? null : comment.trim() || null,
        reopen_reason: next === "open" ? comment.trim() : null,
      });
    await (APPROVAL_STEPS.has(next) ? withStepUp(run) : run());
    pushAppToast({
      title: t("reviews.moved", { status: t(`reviewStatus.${next}`) }),
      variant: "success",
    });
    await load();
  };

  if (budgetVersionId == null) {
    return <EmptyState title={t("reviews.noVersion")} description={t("reviews.noVersionBody")} />;
  }
  if (loading && reviews.length === 0) return <LoadingState />;
  if (error)
    return <ErrorState error={error} title={t("reviews.loadError")} onRetry={() => void load()} />;

  return (
    <div className="space-y-3">
      <p className="max-w-3xl text-sm text-text-secondary">{t("reviews.intro")}</p>
      {reviews.length === 0 ? (
        <EmptyState title={t("reviews.empty")} description={t("reviews.emptyBody")} />
      ) : (
        <ul className="space-y-2">
          {reviews.map((r) => {
            const next = NEXT_REVIEW_STATUS[r.review_status];
            return (
              <li key={r.id} className="rounded-lg border border-surface-border bg-surface-1 p-3">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="font-medium text-text-primary">
                      {r.cost_center_code} ·{" "}
                      {t(`bucket.${r.budget_bucket}`, { defaultValue: r.budget_bucket })}
                      {r.period_month ? ` · ${t(`month.${r.period_month}`)}` : ""}
                    </div>
                    <div className="text-xs text-text-muted">
                      {t("reviews.opened", { date: formatDate(r.opened_at) })} ·{" "}
                      {t(`driver.${r.driver_code}`, { defaultValue: r.driver_code })}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-sm tabular-nums">
                      {formatCurrency(r.variance_amount, currency ?? undefined)} (
                      {formatPercent(r.variance_pct)})
                    </span>
                    <StatusChip
                      tone={toneOf(REVIEW_STATUS_TONE, r.review_status)}
                      label={t(`reviewStatus.${r.review_status}`)}
                    />
                  </div>
                </div>
                <p className="mt-2 whitespace-pre-line text-sm text-text-secondary">
                  {r.review_commentary}
                </p>
                {r.reopen_reason ? (
                  <p className="mt-1 text-xs text-text-muted">
                    {t("reviews.reopenReason", { reason: r.reopen_reason })}
                  </p>
                ) : null}
                <div className="mt-2 flex flex-wrap gap-2">
                  {next && canAct(next) ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 text-xs"
                      onClick={() => {
                        setComment("");
                        setActing({ review: r, next });
                      }}
                    >
                      {t(`reviewAction.${next}`)}
                    </Button>
                  ) : null}
                  {r.review_status !== "open" && canAct("open") ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 text-xs"
                      onClick={() => {
                        setComment("");
                        setActing({ review: r, next: "open" });
                      }}
                    >
                      {t("reviewAction.open")}
                    </Button>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      <ConfirmDialog
        open={acting != null}
        onOpenChange={(open) => (open ? undefined : setActing(null))}
        title={acting ? t(`reviewAction.${acting.next}`) : ""}
        description={acting ? t(`reviews.confirm.${acting.next}`) : ""}
        confirmLabel={acting ? t(`reviewAction.${acting.next}`) : ""}
        confirmDisabled={acting?.next === "open" && comment.trim().length === 0}
        onConfirm={confirm}
      >
        <div className="space-y-1">
          <Label htmlFor="review-comment">
            {acting?.next === "open" ? t("reviews.reopenLabel") : t("reviews.commentLabel")}
          </Label>
          <Textarea
            id="review-comment"
            rows={3}
            value={comment}
            onChange={(e) => setComment(e.target.value)}
          />
        </div>
      </ConfirmDialog>
      {StepUpDialogElement}
    </div>
  );
}

export interface VarianceReviewDialogProps {
  row: BudgetCockpitRow | null;
  cockpit: BudgetCockpit | null;
  onClose: () => void;
  onCreated: () => void;
}

/** Opens a variance review on a cockpit row, with its figures as the snapshot. */
export function VarianceReviewDialog({
  row,
  cockpit,
  onClose,
  onCreated,
}: VarianceReviewDialogProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency } = useFormatters();
  const { info } = useSession();
  const [driver, setDriver] = useState<string>(VARIANCE_DRIVERS[0]);
  const [commentary, setCommentary] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    if (row) {
      setDriver(VARIANCE_DRIVERS[0]);
      setCommentary("");
      setError(null);
    }
  }, [row]);

  const versionId = cockpit?.version?.id ?? null;
  const ownerId = info?.user_id ?? null;
  const valid = commentary.trim().length > 0 && versionId != null && ownerId != null;

  const submit = async () => {
    if (!row || !cockpit || !valid || versionId == null || ownerId == null) return;
    setSaving(true);
    setError(null);
    try {
      await createBudgetVarianceReview({
        budget_version_id: versionId,
        cost_center_id: row.cost_center_id,
        period_month: null,
        budget_bucket: row.bucket,
        variance_amount: row.variance,
        variance_pct: variancePct(row),
        driver_code: driver,
        action_owner_id: ownerId,
        review_commentary: commentary.trim(),
        snapshot_context_json: JSON.stringify({
          fiscal_year: cockpit.fiscal_year,
          as_of_month: cockpit.as_of_month,
          budget: row.budget,
          committed: row.committed,
          actual: row.actual,
          forecast: row.forecast,
        }),
      });
      pushAppToast({ title: t("reviews.created"), variant: "success" });
      onCreated();
      onClose();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  };

  const money = (v: number) => formatCurrency(v, cockpit?.currency_code ?? undefined);

  return (
    <Dialog open={row != null} onOpenChange={(open) => (open ? undefined : onClose())}>
      <DialogContent className="max-w-lg">
        {row ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {t("reviews.newTitle", {
                  cc: row.cost_center_code,
                  bucket: t(`bucket.${row.bucket}`, { defaultValue: row.bucket }),
                })}
              </DialogTitle>
              <DialogDescription>
                {t("reviews.newBody", {
                  budget: money(row.budget),
                  forecast: money(row.forecast),
                  variance: money(row.variance),
                })}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-3">
              <div className="space-y-1">
                <Label htmlFor="review-driver">{t("reviews.driver")}</Label>
                <Select value={driver} onValueChange={setDriver}>
                  <SelectTrigger id="review-driver">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {VARIANCE_DRIVERS.map((d) => (
                      <SelectItem key={d} value={d}>
                        {t(`driver.${d}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label htmlFor="review-text">{t("reviews.commentary")}</Label>
                <Textarea
                  id="review-text"
                  rows={3}
                  value={commentary}
                  onChange={(e) => setCommentary(e.target.value)}
                  placeholder={t("reviews.commentaryPlaceholder")}
                />
              </div>
              <p className="text-xs text-text-muted">
                {t("reviews.owner", { name: info?.display_name ?? info?.username ?? "—" })}
              </p>
              {error ? <ErrorState error={error} title={t("reviews.saveError")} /> : null}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                {t("actions.cancel")}
              </Button>
              <Button onClick={() => void submit()} disabled={!valid || saving}>
                {saving ? t("actions.saving") : t("reviews.open")}
              </Button>
            </DialogFooter>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
