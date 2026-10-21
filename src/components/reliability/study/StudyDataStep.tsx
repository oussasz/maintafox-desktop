/**
 * Study step 1 — failure and suspension records, include/exclude with a reason kept in the
 * study history, and the data checks with a direct way to correct each one.
 */
import { ArrowRight } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_CHECK_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type DataCheck,
  type StudyDataset,
  type StudyRecord,
  type StudyRecordAction,
  setReliabilityStudyRecord,
} from "@/services/reliability-study-service";

import { RAMS_TABLE_FRAME, RamsCallout, RamsCard } from "./rams-ui";

export function StudyDataStep({
  dataset,
  editable,
  onChanged,
  onNext,
}: {
  dataset: StudyDataset;
  editable: boolean;
  onChanged: (d: StudyDataset) => void;
  onNext: () => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatNumber, formatDateTime, formatDecimal } = useFormatters();
  const navigate = useNavigate();
  const [excludeTarget, setExcludeTarget] = useState<StudyRecord | null>(null);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [showHistory, setShowHistory] = useState(false);

  const act = async (key: string, action: StudyRecordAction, why?: string) => {
    setBusy(key);
    try {
      onChanged(
        await setReliabilityStudyRecord({
          study_id: dataset.study.id,
          record_key: key,
          action,
          reason: why ?? null,
        }),
      );
      return true;
    } catch {
      return false; // global IPC toast
    } finally {
      setBusy(null);
    }
  };

  const hasAge = (r: StudyRecord) => r.age_hours != null;
  const unit =
    dataset.effective_time_basis === "operating"
      ? t("rams.units.opHours")
      : t("rams.units.calHours");

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
      <RamsCard
        title={t("rams.data.title")}
        subtitle={t("rams.data.hint")}
        headingId="rams-records"
      >
        {dataset.records.length === 0 ? (
          <EmptyState title={t("rams.data.emptyTitle")} description={t("rams.data.emptyBody")} />
        ) : (
          <div className={RAMS_TABLE_FRAME}>
            <div className="overflow-x-auto">
              <table className="w-full text-sm" aria-labelledby="rams-records">
                <thead>
                  <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                    <th className="px-3 py-2 text-start">{t("rams.data.colUsed")}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colSource")}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colAsset")}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colEventAt")}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colRestored")}</th>
                    <th className="px-3 py-2 text-end">{t("rams.data.colAge", { unit })}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colType")}</th>
                    <th className="px-3 py-2 text-start">{t("rams.data.colCoding")}</th>
                  </tr>
                </thead>
                <tbody>
                  {dataset.records.map((r) => {
                    const canToggle = editable && hasAge(r) && busy == null;
                    const label = t("rams.data.useRecord", {
                      record: r.work_order_code ?? r.asset_code,
                    });
                    return (
                      <tr
                        key={r.key}
                        className={cn(
                          "h-11 border-t border-surface-border",
                          !r.included && "text-text-muted",
                        )}
                      >
                        <td className="px-3">
                          <input
                            type="checkbox"
                            className="h-5 w-5 accent-primary"
                            checked={r.included}
                            disabled={!canToggle}
                            aria-label={label}
                            title={!hasAge(r) ? t("rams.data.reason.no_known_start") : undefined}
                            onChange={() => {
                              if (r.included) {
                                setReason("");
                                setExcludeTarget(r);
                              } else {
                                void act(r.key, "include");
                              }
                            }}
                          />
                        </td>
                        <td className="px-3">
                          {r.work_order_code ? (
                            <LinkedEntityBadge
                              entity="work_order"
                              code={r.work_order_code}
                              entityId={r.work_order_id}
                            />
                          ) : (
                            <span className="text-xs">{t("rams.data.endSource")}</span>
                          )}
                        </td>
                        <td className="px-3 font-mono text-xs">{r.asset_code}</td>
                        <td className={cn("px-3", !r.included && "line-through")}>
                          {formatDateTime(r.event_at)}
                          <span className="block text-[11px] text-text-muted">
                            {t(`rams.data.timeSource.${r.event_time_source}`)}
                          </span>
                        </td>
                        <td className="px-3">
                          {r.restored_at ? formatDateTime(r.restored_at) : "—"}
                        </td>
                        <td className="px-3 text-end font-mono tabular-nums">
                          {r.age_hours == null ? "—" : formatNumber(Math.round(r.age_hours))}
                        </td>
                        <td className="px-3">
                          <div className="flex flex-wrap gap-1">
                            <StatusChip
                              tone={r.kind === "failure" ? "danger" : "neutral"}
                              label={t(`rams.data.kind.${r.kind}`)}
                            />
                            {r.flags.some((f) => f !== "no_known_start") ? (
                              <StatusChip tone="warning" label={t("rams.data.toCheck")} />
                            ) : null}
                          </div>
                          {r.state_reason ? (
                            <span className="mt-0.5 block text-[11px] text-text-muted">
                              {t(`rams.data.reason.${r.state_reason}`)}
                              {r.analyst_note ? ` — ${r.analyst_note}` : ""}
                            </span>
                          ) : null}
                        </td>
                        <td className="px-3 text-xs">
                          {r.kind === "failure"
                            ? [
                                r.failure_mode_code
                                  ? `${r.failure_mode_code} ${r.failure_mode_label ?? ""}`.trim()
                                  : t("rams.data.modeMissing"),
                                r.failure_cause_label ??
                                  (r.cause_not_determined
                                    ? t("rams.data.causeNotDetermined")
                                    : t("rams.data.causeMissing")),
                              ].join(" · ")
                            : t(`rams.data.kindHint.${r.kind}`)}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </RamsCard>

      <div className="grid content-start gap-4">
        <RamsCard title={t("rams.data.checksTitle")} subtitle={t("rams.data.checksHint")}>
          <div className="grid gap-2">
            {dataset.checks.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.data.noChecks")}</p>
            ) : (
              dataset.checks.map((c, i) => (
                <CheckCallout
                  key={`${c.kind}-${c.record_key ?? c.equipment_id ?? i}`}
                  check={c}
                  editable={editable}
                  busy={busy != null}
                  onUseDi={(key) => void act(key, "use_di_time")}
                  onOpenWo={(id) => navigate(`/work-orders?openWo=${id}`)}
                  formatDecimal={formatDecimal}
                />
              ))
            )}
          </div>
        </RamsCard>
        <RamsCard title={t("rams.data.retainedTitle")}>
          <div className="grid grid-cols-3 gap-3">
            <KpiTile label={t("rams.data.failures")} value={formatNumber(dataset.failures_used)} />
            <KpiTile
              label={t("rams.data.suspensions")}
              value={formatNumber(dataset.suspensions_used)}
            />
            <KpiTile label={t("rams.data.excluded")} value={formatNumber(dataset.excluded)} />
          </div>
          <div className="mt-3 flex flex-wrap justify-between gap-2">
            <Button
              variant="ghost"
              className="h-11"
              aria-expanded={showHistory}
              onClick={() => setShowHistory((v) => !v)}
              disabled={dataset.history.length === 0}
            >
              {t("rams.data.history", { count: dataset.history.length })}
            </Button>
            <Button className={mfLayout.primaryAction} onClick={onNext}>
              {t("rams.data.next")}
              <ArrowRight className="h-4 w-4" aria-hidden />
            </Button>
          </div>
          {showHistory ? (
            <ul className="mt-3 grid gap-1.5 text-xs text-text-secondary">
              {dataset.history.map((h, i) => (
                <li key={i}>
                  <span className="font-medium text-text-primary">
                    {t(`rams.data.action.${h.action}`)}
                  </span>{" "}
                  · {h.work_order_code ?? h.asset_code ?? ""} · {formatDateTime(h.created_at)}
                  {h.actor_name ? ` · ${h.actor_name}` : ""}
                  {h.reason ? ` — ${h.reason}` : ""}
                </li>
              ))}
            </ul>
          ) : null}
        </RamsCard>
      </div>

      <Dialog open={excludeTarget != null} onOpenChange={(o) => !o && setExcludeTarget(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("rams.data.excludeTitle")}</DialogTitle>
            <DialogDescription>
              {t("rams.data.excludeBody", {
                record: excludeTarget?.work_order_code ?? excludeTarget?.asset_code ?? "",
              })}
            </DialogDescription>
          </DialogHeader>
          <label className="grid gap-1 text-sm font-medium text-text-secondary">
            {t("rams.data.excludeReason")}
            <Textarea value={reason} onChange={(e) => setReason(e.target.value)} rows={3} />
          </label>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setExcludeTarget(null)}>
              {t("rams.common.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={reason.trim().length === 0 || busy != null}
              onClick={() => {
                const target = excludeTarget;
                if (!target) return;
                void act(target.key, "exclude", reason.trim()).then((ok) => {
                  if (ok) setExcludeTarget(null);
                });
              }}
            >
              {t("rams.data.excludeAction")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function CheckCallout({
  check,
  editable,
  busy,
  onUseDi,
  onOpenWo,
  formatDecimal,
}: {
  check: DataCheck;
  editable: boolean;
  busy: boolean;
  onUseDi: (key: string) => void;
  onOpenWo: (id: number) => void;
  formatDecimal: (v: number, dp?: number) => string;
}) {
  const { t } = useTranslation("reliability");
  const tone = toneOf(RELIABILITY_CHECK_TONE, check.severity);
  const vars = {
    wo: check.work_order_code ?? "",
    di: check.di_code ?? "",
    asset: check.asset_code ?? "",
    hours: formatDecimal(check.hours ?? 0, 0),
    count: check.count ?? 0,
    coverage: formatDecimal((check.coverage ?? 0) * 100, 0),
  };
  let action = null;
  if (check.kind === "di_earlier" && check.record_key && editable) {
    const key = check.record_key;
    action = (
      <Button variant="outline" className="h-10" disabled={busy} onClick={() => onUseDi(key)}>
        {t("rams.checks.useDi")}
      </Button>
    );
  } else if (
    (check.kind === "cause_missing" || check.kind === "mode_missing") &&
    check.work_order_id != null
  ) {
    const id = check.work_order_id;
    action = (
      <Button variant="outline" className="h-10" onClick={() => onOpenWo(id)}>
        {t("rams.checks.openWo")}
      </Button>
    );
  }
  const kindKey =
    check.kind === "operating_hours" && check.severity !== "success"
      ? "operating_hours_low"
      : check.kind;
  return (
    <RamsCallout tone={tone} title={t(`rams.checks.${kindKey}.title`, vars)} action={action}>
      {t(`rams.checks.${kindKey}.body`, vars)}
    </RamsCallout>
  );
}
