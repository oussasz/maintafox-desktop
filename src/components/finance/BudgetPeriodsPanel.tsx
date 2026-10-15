/**
 * Budget period close (month by month, calendar fiscal year). A closed month refuses new actuals;
 * OT settlements then land in the next open month. A month can only be closed once it has ended
 * and once every OT closed in it is settled. Closing and reopening both change reported figures:
 * they need `fin.approve` and a step-up.
 */
import { Lock, LockOpen } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PERIOD_STATUS_TONE } from "@/components/finance/finance-tone";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Label } from "@/components/ui/label";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import {
  type BudgetPeriod,
  closeBudgetPeriod,
  listBudgetPeriods,
  reopenBudgetPeriod,
} from "@/services/finance-cost-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

export interface BudgetPeriodsPanelProps {
  fiscalYear: number;
  currency: string | null;
  reloadToken?: number;
  onChanged?: () => void;
}

/** Why a month cannot be closed, or null. */
export function closeBlocker(p: BudgetPeriod): "closed" | "future" | "pending" | null {
  if (p.status === "closed") return "closed";
  if (p.is_future) return "future";
  if (p.pending_settlements > 0) return "pending";
  return null;
}

export function BudgetPeriodsPanel({
  fiscalYear,
  currency,
  reloadToken = 0,
  onChanged,
}: BudgetPeriodsPanelProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatDateTime } = useFormatters();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [periods, setPeriods] = useState<BudgetPeriod[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [closing, setClosing] = useState<BudgetPeriod | null>(null);
  const [reopening, setReopening] = useState<BudgetPeriod | null>(null);
  const [text, setText] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setPeriods(await listBudgetPeriods(fiscalYear));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [fiscalYear]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken forces a refresh
  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const monthLabel = (p: BudgetPeriod) => `${t(`month.${p.calendar_month}`)} ${p.calendar_year}`;

  const confirmClose = async () => {
    if (!closing) return;
    const target = closing;
    const updated = await withStepUp(() =>
      closeBudgetPeriod({
        fiscal_year: fiscalYear,
        period_month: target.period_month,
        notes: text.trim() || null,
      }),
    );
    setPeriods(updated);
    pushAppToast({
      title: t("periods.closedToast", { month: monthLabel(target) }),
      variant: "success",
    });
    onChanged?.();
  };

  const confirmReopen = async () => {
    if (!reopening) return;
    const target = reopening;
    const updated = await withStepUp(() =>
      reopenBudgetPeriod({
        fiscal_year: fiscalYear,
        period_month: target.period_month,
        reason: text.trim(),
      }),
    );
    setPeriods(updated);
    pushAppToast({
      title: t("periods.reopenedToast", { month: monthLabel(target) }),
      variant: "success",
    });
    onChanged?.();
  };

  if (loading && periods.length === 0) return <LoadingState />;
  if (error && periods.length === 0) {
    return <ErrorState error={error} title={t("periods.loadError")} onRetry={() => void load()} />;
  }

  return (
    <div className="space-y-3">
      <p className="max-w-3xl text-sm text-text-secondary">{t("periods.intro")}</p>
      <div className="overflow-x-auto rounded-lg border border-surface-border">
        <table className="w-full text-sm">
          <caption className="sr-only">{t("periods.caption", { year: fiscalYear })}</caption>
          <thead className="bg-surface-2 text-xs uppercase tracking-wide text-text-secondary">
            <tr>
              <th scope="col" className="px-3 py-2 text-start font-semibold">
                {t("periods.col.month")}
              </th>
              <th scope="col" className="px-3 py-2 text-start font-semibold">
                {t("periods.col.status")}
              </th>
              <th scope="col" className="px-3 py-2 text-end font-semibold">
                {t("periods.col.actual")}
              </th>
              <th scope="col" className="px-3 py-2 text-end font-semibold">
                {t("periods.col.pending")}
              </th>
              <th scope="col" className="px-3 py-2 text-start font-semibold">
                {t("periods.col.closedBy")}
              </th>
              <th scope="col" className="px-3 py-2 text-end font-semibold">
                <span className="sr-only">{t("periods.col.actions")}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {periods.map((p) => {
              const blocker = closeBlocker(p);
              return (
                <tr key={p.period_month} className="border-t border-surface-border">
                  <th scope="row" className="px-3 py-2 text-start font-medium">
                    {monthLabel(p)}
                  </th>
                  <td className="px-3 py-2">
                    <StatusChip
                      tone={toneOf(PERIOD_STATUS_TONE, p.status)}
                      label={t(`periodStatus.${p.status}`)}
                    />
                  </td>
                  <td className="px-3 py-2 text-end font-mono tabular-nums">
                    {formatCurrency(p.actual_amount, currency ?? undefined)}
                  </td>
                  <td className="px-3 py-2 text-end">
                    {p.pending_settlements > 0 ? p.pending_settlements : "—"}
                  </td>
                  <td className="px-3 py-2 text-xs text-text-muted">
                    {p.status === "closed" && p.closed_at
                      ? t("periods.closedBy", {
                          who: p.closed_by_name ?? "—",
                          at: formatDateTime(p.closed_at),
                        })
                      : p.reopened_at
                        ? t("periods.reopenedAt", { at: formatDateTime(p.reopened_at) })
                        : "—"}
                    {p.notes ? <div className="whitespace-pre-line">{p.notes}</div> : null}
                  </td>
                  <td className="px-3 py-2 text-end">
                    {p.status === "open" && can(P.FIN_APPROVE) ? (
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-7 gap-1 text-xs"
                        disabled={blocker != null}
                        title={
                          blocker
                            ? t(`periods.blocker.${blocker}`, { count: p.pending_settlements })
                            : undefined
                        }
                        onClick={() => {
                          setText("");
                          setClosing(p);
                        }}
                      >
                        <Lock className="h-3.5 w-3.5" aria-hidden />
                        {t("periods.close")}
                      </Button>
                    ) : null}
                    {p.status === "closed" && can(P.FIN_APPROVE) ? (
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-7 gap-1 text-xs"
                        onClick={() => {
                          setText("");
                          setReopening(p);
                        }}
                      >
                        <LockOpen className="h-3.5 w-3.5" aria-hidden />
                        {t("periods.reopen")}
                      </Button>
                    ) : null}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <ConfirmDialog
        open={closing != null}
        onOpenChange={(open) => (open ? undefined : setClosing(null))}
        title={closing ? t("periods.closeTitle", { month: monthLabel(closing) }) : ""}
        description={t("periods.closeDescription")}
        confirmLabel={t("periods.close")}
        onConfirm={confirmClose}
      >
        <div className="space-y-1">
          <Label htmlFor="period-notes">{t("periods.notes")}</Label>
          <Textarea
            id="period-notes"
            rows={2}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </div>
      </ConfirmDialog>
      <ConfirmDialog
        open={reopening != null}
        onOpenChange={(open) => (open ? undefined : setReopening(null))}
        title={reopening ? t("periods.reopenTitle", { month: monthLabel(reopening) }) : ""}
        description={t("periods.reopenDescription")}
        confirmLabel={t("periods.reopen")}
        destructive
        confirmDisabled={text.trim().length === 0}
        onConfirm={confirmReopen}
      >
        <div className="space-y-1">
          <Label htmlFor="period-reason">{t("periods.reason")}</Label>
          <Textarea
            id="period-reason"
            rows={2}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </div>
      </ConfirmDialog>
      {StepUpDialogElement}
    </div>
  );
}
