/**
 * The weekly maintenance schedule of a planning site (planning step D): the site's crews as rows,
 * the seven days of the site's week as columns (the week starts the day after the site's
 * weekend), and the week's lifecycle — draft → approved → frozen → published, the automatic
 * freeze at the end of the previous week's last working day, and the four-eyes unfreeze.
 */

import { AlertTriangle, ChevronLeft, ChevronRight, Lock } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  addDays,
  commitmentsOnDay,
  errorText,
  isoDay,
  parseDay,
} from "@/components/planning/weekly-schedule-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { listOrgTree } from "@/services/org-node-service";
import {
  approveWeeklySchedule,
  decideWeeklyUnfreeze,
  freezeWeeklySchedule,
  getWeeklySchedule,
  publishWeeklySchedule,
  requestWeeklyUnfreeze,
} from "@/services/planning-service";
import type { OrgTreeRow, WeeklyScheduleRef, WeeklyScheduleView } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function WeeklySchedulePanel() {
  const { t, i18n } = useTranslation("planning");
  const { formatDate, formatDateTime } = useFormatters();
  const [sites, setSites] = useState<OrgTreeRow[]>([]);
  const [siteId, setSiteId] = useState<number | null>(null);
  const [day, setDay] = useState<string>(() => isoDay(new Date()));
  const [view, setView] = useState<WeeklyScheduleView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unfreezeOpen, setUnfreezeOpen] = useState(false);
  const [unfreezeReason, setUnfreezeReason] = useState("");

  useEffect(() => {
    let cancelled = false;
    listOrgTree()
      .then((rows) => {
        if (cancelled) return;
        const planningSites = rows.filter((r) => r.can_own_work && r.node.status === "active");
        setSites(planningSites);
        setSiteId((current) => current ?? planningSites[0]?.node.id ?? null);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(errorText(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const load = useCallback(async () => {
    if (siteId == null) return;
    setError(null);
    try {
      setView(await getWeeklySchedule(siteId, day));
    } catch (err) {
      setView(null);
      setError(errorText(err));
    }
  }, [siteId, day]);

  useEffect(() => {
    void load();
  }, [load]);

  const ref = (): WeeklyScheduleRef => ({
    site_id: siteId ?? 0,
    day,
    expected_row_version: view?.schedule?.row_version ?? null,
  });

  async function act(action: () => Promise<WeeklyScheduleView>) {
    setBusy(true);
    setError(null);
    try {
      setView(await action());
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  const days = useMemo(
    () => (view ? Array.from({ length: 7 }, (_, i) => addDays(view.week.week_start, i)) : []),
    [view],
  );
  const working = useMemo(() => new Set(view?.week.working_days ?? []), [view]);
  const weekdayFormat = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { weekday: "short" }),
    [i18n.language],
  );
  const schedule = view?.schedule ?? null;
  const status = schedule?.status ?? "draft";
  const pendingUnfreeze = schedule?.unfreeze_requested_at != null;
  const weekStart = view?.week.week_start;

  let freezeLine: string | null = null;
  if (view && (status === "draft" || status === "approved")) {
    freezeLine = t("weekly.freezeDue", { at: formatDateTime(view.freeze_due_at) });
  } else if (schedule?.frozen_at) {
    freezeLine =
      schedule.frozen_by_id == null
        ? t("weekly.frozenAutomatically", { at: formatDateTime(schedule.frozen_at) })
        : t("weekly.frozenAt", { at: formatDateTime(schedule.frozen_at) });
  }

  return (
    <section
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-3"
      aria-label={t("weekly.title")}
    >
      <div className="flex flex-wrap items-end gap-3">
        <div className="min-w-[220px]">
          <Label>{t("weekly.site")}</Label>
          <Select
            value={siteId == null ? "" : String(siteId)}
            onValueChange={(v) => setSiteId(Number(v))}
          >
            <SelectTrigger className="h-8 text-sm" aria-label={t("weekly.site")}>
              <SelectValue placeholder={t("weekly.noSite")} />
            </SelectTrigger>
            <SelectContent>
              {sites.map((row) => (
                <SelectItem key={row.node.id} value={String(row.node.id)}>
                  {row.node.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-1">
          <Button
            type="button"
            size="sm"
            variant="outline"
            aria-label={t("weekly.previous")}
            onClick={() => setDay((d) => addDays(weekStart ?? d, -7))}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <span className="min-w-[200px] text-center text-sm font-medium">
            {view
              ? t("weekly.range", {
                  from: formatDate(parseDay(view.week.week_start)),
                  to: formatDate(parseDay(view.week.week_end)),
                })
              : "—"}
          </span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            aria-label={t("weekly.next")}
            onClick={() => setDay((d) => addDays(weekStart ?? d, 7))}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
        {view ? (
          <Badge variant={status === "draft" ? "outline" : "secondary"} data-testid="weekly-status">
            {t(`weekly.status.${status}`)}
          </Badge>
        ) : null}
        {view?.awaiting_approval ? (
          <Badge variant="outline" className="border-amber-200 bg-amber-50 text-amber-800">
            <AlertTriangle className="mr-1 h-3 w-3" />
            {t("weekly.frozenWithoutApproval")}
          </Badge>
        ) : null}
        {view ? (
          <PermissionGate permission={P.PLAN_CONFIRM}>
            <div className="ml-auto flex flex-wrap gap-2">
              {status === "draft" || view.awaiting_approval ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => void act(() => approveWeeklySchedule(ref()))}
                >
                  {view.awaiting_approval ? t("weekly.approveAfterFreeze") : t("weekly.approve")}
                </Button>
              ) : null}
              {status === "draft" || status === "approved" ? (
                <Button
                  type="button"
                  size="sm"
                  disabled={busy}
                  onClick={() => void act(() => freezeWeeklySchedule(ref()))}
                >
                  <Lock className="mr-1 h-3 w-3" />
                  {t("weekly.freeze")}
                </Button>
              ) : null}
              {status === "frozen" ? (
                <Button
                  type="button"
                  size="sm"
                  disabled={busy}
                  onClick={() => void act(() => publishWeeklySchedule(ref()))}
                >
                  {t("weekly.publish")}
                </Button>
              ) : null}
              {(status === "frozen" || status === "published") && !pendingUnfreeze ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => setUnfreezeOpen(true)}
                >
                  {t("weekly.requestUnfreeze")}
                </Button>
              ) : null}
            </div>
          </PermissionGate>
        ) : null}
      </div>

      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}

      {sites.length === 0 && !error ? (
        <p className="text-sm text-text-muted">{t("weekly.noPlanningSite")}</p>
      ) : null}

      {view?.awaiting_approval ? (
        <div className="rounded-md border border-amber-200 bg-amber-50 p-2 text-sm text-amber-900">
          {t("weekly.frozenWithoutApprovalHint")}
        </div>
      ) : null}

      {freezeLine ? <p className="text-xs text-text-muted">{freezeLine}</p> : null}

      {pendingUnfreeze && schedule ? (
        <div className="rounded-md border border-amber-200 bg-amber-50 p-2 text-sm text-amber-900">
          <p>
            {t("weekly.unfreezePending", {
              reason: schedule.unfreeze_reason ?? "",
              at: formatDateTime(schedule.unfreeze_requested_at),
            })}
          </p>
          <PermissionGate permission={P.PLAN_CONFIRM}>
            <div className="mt-2 flex gap-2">
              <Button
                type="button"
                size="sm"
                disabled={busy}
                onClick={() =>
                  void act(() => decideWeeklyUnfreeze({ ...ref(), approve: true, comment: null }))
                }
              >
                {t("weekly.approveUnfreeze")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void act(() => decideWeeklyUnfreeze({ ...ref(), approve: false, comment: null }))
                }
              >
                {t("weekly.rejectUnfreeze")}
              </Button>
            </div>
          </PermissionGate>
        </div>
      ) : null}

      {view ? (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-xs">
            <thead>
              <tr>
                <th className="w-40 border border-surface-border p-1 text-left">
                  {t("weekly.crew")}
                </th>
                {days.map((d) => (
                  <th
                    key={d}
                    className={cn(
                      "border border-surface-border p-1 text-left font-medium",
                      !working.has(d) && "bg-surface-2 text-text-muted",
                    )}
                  >
                    {weekdayFormat.format(parseDay(d))} {formatDate(parseDay(d))}
                    {!working.has(d) ? (
                      <span className="ml-1 font-normal">({t("weekly.dayOff")})</span>
                    ) : null}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {view.crews.map((crew) => (
                <tr key={crew.team_id}>
                  <th
                    scope="row"
                    className="border border-surface-border p-1 text-left font-medium"
                  >
                    {crew.team_code} — {crew.team_name}
                  </th>
                  {days.map((d) => (
                    <td
                      key={d}
                      className={cn(
                        "border border-surface-border p-1 align-top",
                        !working.has(d) && "bg-surface-2",
                      )}
                    >
                      <ul className="space-y-1">
                        {commitmentsOnDay(view.commitments, crew.team_id, d).map((c) => (
                          <li
                            key={c.id}
                            className={cn(
                              "rounded border px-1 py-0.5",
                              c.status === "closed"
                                ? "border-surface-border text-text-muted line-through"
                                : c.has_blocking_conflict
                                  ? "border-status-danger/40 bg-status-danger/10"
                                  : "border-surface-border bg-surface-0",
                            )}
                            title={c.source_title ?? undefined}
                          >
                            {c.frozen_at ? (
                              <Lock
                                className="mr-1 inline h-3 w-3"
                                aria-label={t("weekly.frozenJob")}
                              />
                            ) : null}
                            {c.source_code ?? `${c.source_type} #${c.source_id}`}
                            {c.assigned_personnel_name ? ` · ${c.assigned_personnel_name}` : ""}
                            {c.status === "closed" ? ` (${t("commitmentStatus.closed")})` : ""}
                          </li>
                        ))}
                      </ul>
                    </td>
                  ))}
                </tr>
              ))}
              {view.crews.length === 0 ? (
                <tr>
                  <td colSpan={8} className="p-2 text-text-muted">
                    {t("weekly.noCrews")}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      ) : null}

      {view && view.events.length > 0 ? (
        <div>
          <h3 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
            {t("weekly.history")}
          </h3>
          <ul className="mt-1 space-y-0.5 text-xs text-text-secondary">
            {view.events.map((e) => (
              <li key={e.id}>
                {formatDateTime(e.created_at)} — {t(`weekly.events.${e.action}`)}
                {" · "}
                {e.actor_name ?? t("weekly.system")}
                {e.approver_name ? ` → ${e.approver_name}` : ""}
                {e.reason ? ` — ${e.reason}` : ""}
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      <Dialog open={unfreezeOpen} onOpenChange={setUnfreezeOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("weekly.requestUnfreeze")}</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-text-secondary">{t("weekly.unfreezeHint")}</p>
          <Label htmlFor="weekly-unfreeze-reason">{t("weekly.unfreezeReason")}</Label>
          <Textarea
            id="weekly-unfreeze-reason"
            value={unfreezeReason}
            onChange={(e) => setUnfreezeReason(e.target.value)}
          />
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setUnfreezeOpen(false)}
            >
              {t("dialogs.cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={busy || unfreezeReason.trim() === ""}
              onClick={() =>
                void act(async () => {
                  const next = await requestWeeklyUnfreeze({ ...ref(), reason: unfreezeReason });
                  setUnfreezeOpen(false);
                  setUnfreezeReason("");
                  return next;
                })
              }
            >
              {t("weekly.sendUnfreezeRequest")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
