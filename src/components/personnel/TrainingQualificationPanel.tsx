import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  evaluateCrewPermitSkillGaps,
  listCertificationExpiryDrilldown,
  listMyPersonnelCertifications,
  listMyTrainingSessions,
  listPersonnelCertifications,
  listTrainingAttendance,
  listTrainingExpiryAlertEvents,
  listTrainingSessions,
  scanTrainingExpiryAlerts,
} from "@/services/qualification-service";
import type {
  CertificationExpiryDrilldownRow,
  CrewPermitSkillGapResult,
  PersonnelCertification,
  TrainingAttendance,
  TrainingExpiryAlertEvent,
  TrainingSession,
} from "@shared/ipc-types";

export function TrainingQualificationPanel() {
  const { t } = useTranslation("personnel");
  const [sessions, setSessions] = useState<TrainingSession[]>([]);
  const [attendance, setAttendance] = useState<TrainingAttendance[]>([]);
  const [myAttendance, setMyAttendance] = useState<TrainingAttendance[]>([]);
  const [myCerts, setMyCerts] = useState<PersonnelCertification[]>([]);
  const [registryCerts, setRegistryCerts] = useState<PersonnelCertification[]>([]);
  const [loading, setLoading] = useState(false);
  const [woId, setWoId] = useState("");
  const [crewIds, setCrewIds] = useState("");
  const [gapResult, setGapResult] = useState<CrewPermitSkillGapResult | null>(null);
  const [gapLoading, setGapLoading] = useState(false);
  const [lookaheadDays, setLookaheadDays] = useState(90);
  const [entityFilter, setEntityFilter] = useState("");
  const [alertSeverity, setAlertSeverity] = useState("");
  const [expiryAlerts, setExpiryAlerts] = useState<TrainingExpiryAlertEvent[]>([]);
  const [expiryDrilldown, setExpiryDrilldown] = useState<CertificationExpiryDrilldownRow[]>([]);
  const [scanLoading, setScanLoading] = useState(false);

  const expiryFiltersRef = useRef({ lookaheadDays, entityFilter, alertSeverity });
  expiryFiltersRef.current = { lookaheadDays, entityFilter, alertSeverity };

  const load = useCallback(async () => {
    setLoading(true);
    const { lookaheadDays: ld, entityFilter: ef, alertSeverity: sev } = expiryFiltersRef.current;
    const la = Math.max(1, Math.min(730, ld));
    const rawEid = ef.trim() === "" ? NaN : Number.parseInt(ef.trim(), 10);
    const entityId = !Number.isNaN(rawEid) && rawEid > 0 ? rawEid : null;
    try {
      const [s, a, ma, mc, rc, ea, ed] = await Promise.all([
        listTrainingSessions(),
        listTrainingAttendance({ limit: 100 }),
        listMyTrainingSessions(),
        listMyPersonnelCertifications(),
        listPersonnelCertifications({ limit: 80 }),
        listTrainingExpiryAlertEvents({
          limit: 100,
          severity: sev || null,
        }),
        listCertificationExpiryDrilldown(entityId, la),
      ]);
      setSessions(s);
      setAttendance(a);
      setMyAttendance(ma);
      setMyCerts(mc);
      setRegistryCerts(rc);
      setExpiryAlerts(ea);
      setExpiryDrilldown(ed);
    } catch {
      setSessions([]);
      setAttendance([]);
      setMyAttendance([]);
      setMyCerts([]);
      setRegistryCerts([]);
      setExpiryAlerts([]);
      setExpiryDrilldown([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, alertSeverity]);

  const runExpiryScan = useCallback(async () => {
    const la = Math.max(1, Math.min(730, expiryFiltersRef.current.lookaheadDays));
    setScanLoading(true);
    try {
      await scanTrainingExpiryAlerts(la);
      await load();
    } catch {
      /* ignore */
    } finally {
      setScanLoading(false);
    }
  }, [load]);

  const runGapCheck = useCallback(async () => {
    const wid = Number.parseInt(woId.trim(), 10);
    const ids = crewIds
      .split(/[\s,;]+/)
      .map((x) => Number.parseInt(x.trim(), 10))
      .filter((n) => !Number.isNaN(n) && n > 0);
    if (Number.isNaN(wid) || wid <= 0 || ids.length === 0) {
      setGapResult(null);
      return;
    }
    setGapLoading(true);
    try {
      const r = await evaluateCrewPermitSkillGaps({ work_order_id: wid, personnel_ids: ids });
      setGapResult(r);
    } catch {
      setGapResult(null);
    } finally {
      setGapLoading(false);
    }
  }, [woId, crewIds]);

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-text-primary">{t("training.heading")}</h2>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => void load()}
          disabled={loading}
        >
          <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
          {t("action.refresh")}
        </Button>
      </div>

      <PermissionGate permission="trn.view">
        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-muted">{t("training.selfService")}</h3>
          <div className="grid gap-4 md:grid-cols-2">
            <div className="rounded-lg border border-surface-border p-3">
              <div className="mb-2 text-xs font-medium uppercase text-text-muted">
                {t("training.myCerts")}
              </div>
              {myCerts.length === 0 ? (
                <div className="text-sm text-text-muted">{t("common.noData")}</div>
              ) : (
                <ul className="space-y-1 text-sm">
                  {myCerts.map((c) => (
                    <li key={c.id} className="flex justify-between gap-2">
                      <span>{c.certification_type_name ?? c.certification_type_code ?? "—"}</span>
                      <span className="text-text-muted">
                        {c.expires_at ?? "—"} ·{" "}
                        <Badge variant="outline">{c.readiness_status}</Badge>
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div className="rounded-lg border border-surface-border p-3">
              <div className="mb-2 text-xs font-medium uppercase text-text-muted">
                {t("training.mySessions")}
              </div>
              {myAttendance.length === 0 ? (
                <div className="text-sm text-text-muted">{t("common.noData")}</div>
              ) : (
                <ul className="space-y-1 text-sm">
                  {myAttendance.map((a) => (
                    <li key={a.id} className="flex justify-between gap-2">
                      <span>
                        {t("training.sessionId")} {a.session_id}
                      </span>
                      <Badge variant="secondary">{a.attendance_status}</Badge>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-muted">{t("training.sessions")}</h3>
          <div className="overflow-x-auto rounded-lg border border-surface-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-muted/50 text-xs uppercase text-text-muted">
                <tr>
                  <th className="px-3 py-2">{t("training.col.course")}</th>
                  <th className="px-3 py-2">{t("training.col.start")}</th>
                  <th className="px-3 py-2">{t("training.col.end")}</th>
                  <th className="px-3 py-2">{t("training.col.minPass")}</th>
                </tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <tr key={s.id} className="border-t border-surface-border">
                    <td className="px-3 py-2 font-mono text-xs">{s.course_code}</td>
                    <td className="px-3 py-2">{s.scheduled_start}</td>
                    <td className="px-3 py-2">{s.scheduled_end}</td>
                    <td className="px-3 py-2">{s.min_pass_score}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {sessions.length === 0 ? (
              <div className="px-3 py-4 text-sm text-text-muted">{t("common.noData")}</div>
            ) : null}
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-muted">{t("training.attendance")}</h3>
          <div className="overflow-x-auto rounded-lg border border-surface-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-muted/50 text-xs uppercase text-text-muted">
                <tr>
                  <th className="px-3 py-2">{t("training.col.session")}</th>
                  <th className="px-3 py-2">{t("training.col.personnel")}</th>
                  <th className="px-3 py-2">{t("training.col.status")}</th>
                  <th className="px-3 py-2">{t("training.col.score")}</th>
                </tr>
              </thead>
              <tbody>
                {attendance.map((a) => (
                  <tr key={a.id} className="border-t border-surface-border">
                    <td className="px-3 py-2">{a.session_id}</td>
                    <td className="px-3 py-2">{a.personnel_id}</td>
                    <td className="px-3 py-2">{a.attendance_status}</td>
                    <td className="px-3 py-2">{a.score ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {attendance.length === 0 ? (
              <div className="px-3 py-4 text-sm text-text-muted">{t("common.noData")}</div>
            ) : null}
          </div>
        </section>

        <section className="space-y-3">
          <h3 className="text-sm font-medium text-text-muted">{t("training.permitGaps")}</h3>
          <div className="flex flex-wrap items-end gap-2">
            <div>
              <div className="mb-1 text-xs text-text-muted">{t("training.woId")}</div>
              <Input
                className="h-9 w-32 font-mono text-sm"
                value={woId}
                onChange={(e) => setWoId(e.target.value)}
                placeholder="WO id"
              />
            </div>
            <div className="min-w-[200px] flex-1">
              <div className="mb-1 text-xs text-text-muted">{t("training.crewIds")}</div>
              <Input
                className="h-9 font-mono text-sm"
                value={crewIds}
                onChange={(e) => setCrewIds(e.target.value)}
                placeholder="1, 2, 3"
              />
            </div>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              disabled={gapLoading}
              onClick={() => void runGapCheck()}
            >
              {t("training.checkGaps")}
            </Button>
          </div>
          {gapResult ? (
            gapResult.permit_type_code || gapResult.rows.length > 0 ? (
              <div className="rounded-lg border border-surface-border p-3 text-sm">
                <div className="mb-2 text-text-muted">
                  {t("training.permitType")}:{" "}
                  <span className="font-mono">{gapResult.permit_type_code || "—"}</span> · WO{" "}
                  {gapResult.work_order_id}
                </div>
                <ul className="space-y-1">
                  {gapResult.rows.map((row) => (
                    <li key={row.personnel_id} className="flex flex-wrap items-center gap-2">
                      <span className="font-mono">#{row.personnel_id}</span>
                      {row.is_qualified ? (
                        <Badge className="bg-emerald-100 text-emerald-900">
                          {t("training.qualified")}
                        </Badge>
                      ) : (
                        <Badge variant="destructive">{t("training.blocked")}</Badge>
                      )}
                      {row.missing_certification_type_ids.length > 0 ? (
                        <span className="text-xs text-text-muted">
                          {t("training.missingTypes")}:{" "}
                          {row.missing_certification_type_ids.join(", ")}
                        </span>
                      ) : null}
                    </li>
                  ))}
                </ul>
              </div>
            ) : (
              <div className="text-sm text-text-muted">{t("training.noPermitRequired")}</div>
            )
          ) : null}
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-muted">{t("training.registryCerts")}</h3>
          <div className="overflow-x-auto rounded-lg border border-surface-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-muted/50 text-xs uppercase text-text-muted">
                <tr>
                  <th className="px-3 py-2">{t("training.col.personnel")}</th>
                  <th className="px-3 py-2">{t("training.col.type")}</th>
                  <th className="px-3 py-2">{t("training.col.expires")}</th>
                  <th className="px-3 py-2">{t("training.col.readiness")}</th>
                </tr>
              </thead>
              <tbody>
                {registryCerts.map((c) => (
                  <tr key={c.id} className="border-t border-surface-border">
                    <td className="px-3 py-2">{c.personnel_id}</td>
                    <td className="px-3 py-2">
                      {c.certification_type_name ??
                        c.certification_type_code ??
                        c.certification_type_id}
                    </td>
                    <td className="px-3 py-2">{c.expires_at ?? "—"}</td>
                    <td className="px-3 py-2">
                      <Badge variant="outline">{c.readiness_status}</Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {registryCerts.length === 0 ? (
              <div className="px-3 py-4 text-sm text-text-muted">{t("common.noData")}</div>
            ) : null}
          </div>
        </section>

        <section className="space-y-3">
          <h3 className="text-sm font-medium text-text-muted">{t("training.expiryGovernance")}</h3>
          <div className="flex flex-wrap items-end gap-2">
            <div>
              <div className="mb-1 text-xs text-text-muted">{t("training.lookaheadDays")}</div>
              <Input
                className="h-9 w-24 font-mono text-sm"
                type="number"
                min={1}
                max={730}
                value={lookaheadDays}
                onChange={(e) => setLookaheadDays(Number.parseInt(e.target.value, 10) || 90)}
              />
            </div>
            <div>
              <div className="mb-1 text-xs text-text-muted">
                {t("training.entityFilterOptional")}
              </div>
              <Input
                className="h-9 w-28 font-mono text-sm"
                value={entityFilter}
                onChange={(e) => setEntityFilter(e.target.value)}
                placeholder="—"
              />
            </div>
            <div>
              <div className="mb-1 text-xs text-text-muted">{t("training.alertSeverity")}</div>
              <select
                className={cn(
                  "h-9 rounded-md border border-input bg-background px-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                )}
                value={alertSeverity}
                onChange={(e) => setAlertSeverity(e.target.value)}
              >
                <option value="">{t("filters.all")}</option>
                <option value="critical">critical</option>
                <option value="warning">warning</option>
                <option value="info">info</option>
              </select>
            </div>
            <PermissionGate permission="trn.manage">
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={scanLoading}
                onClick={() => void runExpiryScan()}
              >
                {t("training.runScan")}
              </Button>
            </PermissionGate>
          </div>

          <div className="space-y-2">
            <h4 className="text-xs font-medium uppercase text-text-muted">
              {t("training.expiryAlerts")}
            </h4>
            <div className="overflow-x-auto rounded-lg border border-surface-border">
              <table className="w-full text-left text-sm">
                <thead className="bg-muted/50 text-xs uppercase text-text-muted">
                  <tr>
                    <th className="px-3 py-2">{t("training.col.severity")}</th>
                    <th className="px-3 py-2">{t("training.col.firedAt")}</th>
                    <th className="px-3 py-2">{t("training.col.certId")}</th>
                    <th className="px-3 py-2">{t("training.col.dedupe")}</th>
                  </tr>
                </thead>
                <tbody>
                  {expiryAlerts.map((ev) => (
                    <tr key={ev.id} className="border-t border-surface-border">
                      <td className="px-3 py-2">
                        <Badge
                          variant={
                            ev.severity === "critical"
                              ? "destructive"
                              : ev.severity === "warning"
                                ? "secondary"
                                : "outline"
                          }
                        >
                          {ev.severity}
                        </Badge>
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">{ev.fired_at}</td>
                      <td className="px-3 py-2 font-mono text-xs">{ev.certification_id}</td>
                      <td
                        className="max-w-[220px] truncate px-3 py-2 font-mono text-xs"
                        title={ev.alert_dedupe_key}
                      >
                        {ev.alert_dedupe_key}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {expiryAlerts.length === 0 ? (
                <div className="px-3 py-4 text-sm text-text-muted">{t("common.noData")}</div>
              ) : null}
            </div>
          </div>

          <div className="space-y-2">
            <h4 className="text-xs font-medium uppercase text-text-muted">
              {t("training.expiryDrilldown")}
            </h4>
            <div className="overflow-x-auto rounded-lg border border-surface-border">
              <table className="w-full text-left text-sm">
                <thead className="bg-muted/50 text-xs uppercase text-text-muted">
                  <tr>
                    <th className="px-3 py-2">{t("training.col.empCode")}</th>
                    <th className="px-3 py-2">{t("training.col.name")}</th>
                    <th className="px-3 py-2">{t("training.col.entity")}</th>
                    <th className="px-3 py-2">{t("training.col.type")}</th>
                    <th className="px-3 py-2">{t("training.col.expires")}</th>
                    <th className="px-3 py-2">{t("training.col.readiness")}</th>
                  </tr>
                </thead>
                <tbody>
                  {expiryDrilldown.map((row) => (
                    <tr key={row.certification_id} className="border-t border-surface-border">
                      <td className="px-3 py-2 font-mono text-xs">{row.employee_code}</td>
                      <td className="px-3 py-2">{row.full_name}</td>
                      <td className="px-3 py-2 font-mono text-xs">
                        {row.primary_entity_id ?? "—"}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">{row.certification_type_code}</td>
                      <td className="px-3 py-2">{row.expires_at ?? "—"}</td>
                      <td className="px-3 py-2">
                        <Badge variant="outline">{row.readiness_status}</Badge>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {expiryDrilldown.length === 0 ? (
                <div className="px-3 py-4 text-sm text-text-muted">{t("common.noData")}</div>
              ) : null}
            </div>
          </div>
        </section>
      </PermissionGate>
    </div>
  );
}
