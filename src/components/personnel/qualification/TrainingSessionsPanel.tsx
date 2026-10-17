/**
 * Training sessions and attendance: plan a session (course, certification it leads to, dates,
 * place, instructor), register people, record the result. A passed session that leads to a
 * certification issues it automatically (backend), valid for the certification's period.
 */
import { GraduationCap, Plus, UserPlus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateTimeLocalField } from "@/components/ui/date-time-local-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import type { Tone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  listTrainingAttendance,
  listTrainingSessions,
  upsertTrainingAttendance,
  upsertTrainingSession,
} from "@/services/qualification-service";
import type {
  CertificationType,
  Personnel,
  TrainingAttendance,
  TrainingSession,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { todayIso } from "./qualification-tones";
import { personLabel } from "./use-qualification-lookups";

export const ATTENDANCE_STATUSES = [
  "registered",
  "attended",
  "passed",
  "failed",
  "no_show",
] as const;
export const ATTENDANCE_TONE: Record<string, Tone> = {
  registered: "info",
  attended: "progress",
  passed: "success",
  failed: "danger",
  no_show: "warning",
};

/** Pass mark proposed for a new session (editable per session). */
export const DEFAULT_PASS_SCORE = 70;

interface SessionDraft {
  id: number | null;
  row_version: number | null;
  course_code: string;
  certification_type_id: string;
  start: string;
  end: string;
  location: string;
  instructor_id: string;
  min_pass_score: string;
}

function toIso(local: string): string {
  // `YYYY-MM-DDTHH:MM` (local) → RFC 3339 in UTC.
  const d = new Date(local);
  return Number.isNaN(d.getTime()) ? local : d.toISOString();
}

function toLocal(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function newDraft(certTypeId: number | null, certTypes: CertificationType[]): SessionDraft {
  const day = todayIso();
  const ct = certTypes.find((c) => c.id === certTypeId);
  return {
    id: null,
    row_version: null,
    course_code: ct ? ct.code : "",
    certification_type_id: certTypeId != null ? String(certTypeId) : "",
    start: `${day}T08:00`,
    end: `${day}T16:00`,
    location: "",
    instructor_id: "",
    min_pass_score: String(DEFAULT_PASS_SCORE),
  };
}

export interface TrainingSessionsPanelProps {
  people: Personnel[];
  /** The people list is empty because the user may not read personnel (pickers say so). */
  peopleRestricted?: boolean;
  certTypes: CertificationType[];
  /** Opens the "new session" form for this certification (from the renewal plan). */
  presetCertificationTypeId?: number | null;
  onPresetConsumed?: () => void;
  onChanged?: () => void;
}

export function TrainingSessionsPanel({
  people,
  peopleRestricted = false,
  certTypes,
  presetCertificationTypeId,
  onPresetConsumed,
  onChanged,
}: TrainingSessionsPanelProps) {
  const { t, i18n } = useTranslation("training");
  const { can } = usePermissions();
  const canManage = can(P.TRN_MANAGE);
  const [sessions, setSessions] = useState<TrainingSession[]>([]);
  const [attendance, setAttendance] = useState<TrainingAttendance[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [draft, setDraft] = useState<SessionDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [newAttendee, setNewAttendee] = useState("");

  const personById = useMemo(() => new Map(people.map((p) => [p.id, p])), [people]);
  const certById = useMemo(() => new Map(certTypes.map((c) => [c.id, c])), [certTypes]);
  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" }),
    [i18n.language],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [s, a] = await Promise.all([
        listTrainingSessions(),
        listTrainingAttendance({ limit: 5000 }),
      ]);
      setSessions([...s].sort((x, y) => y.scheduled_start.localeCompare(x.scheduled_start)));
      setAttendance(a);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (presetCertificationTypeId != null && canManage) {
      setDraft(newDraft(presetCertificationTypeId, certTypes));
      onPresetConsumed?.();
    }
  }, [presetCertificationTypeId, canManage, certTypes, onPresetConsumed]);

  const selected = sessions.find((s) => s.id === selectedId) ?? null;
  const roster = attendance.filter((a) => a.session_id === selectedId);
  const countBySession = useMemo(() => {
    const m = new Map<number, number>();
    for (const a of attendance) m.set(a.session_id, (m.get(a.session_id) ?? 0) + 1);
    return m;
  }, [attendance]);

  const draftValid =
    draft != null &&
    draft.course_code.trim() !== "" &&
    draft.start !== "" &&
    draft.end !== "" &&
    draft.end > draft.start &&
    Number.isInteger(Number(draft.min_pass_score)) &&
    Number(draft.min_pass_score) >= 0 &&
    Number(draft.min_pass_score) <= 100;

  const saveSession = async () => {
    if (!draft || !draftValid) return;
    setSaving(true);
    try {
      const saved = await upsertTrainingSession({
        id: draft.id,
        expected_row_version: draft.row_version,
        course_code: draft.course_code.trim(),
        certification_type_id: draft.certification_type_id
          ? Number(draft.certification_type_id)
          : null,
        scheduled_start: toIso(draft.start),
        scheduled_end: toIso(draft.end),
        location: draft.location.trim() || null,
        instructor_id: draft.instructor_id ? Number(draft.instructor_id) : null,
        min_pass_score: Number(draft.min_pass_score),
      });
      setDraft(null);
      setSelectedId(saved.id);
      await load();
      onChanged?.();
    } catch {
      // Reported by the global toast.
    } finally {
      setSaving(false);
    }
  };

  const register = async () => {
    if (!selected || newAttendee === "") return;
    try {
      await upsertTrainingAttendance({
        id: null,
        expected_row_version: null,
        session_id: selected.id,
        personnel_id: Number(newAttendee),
        attendance_status: "registered",
        completed_at: null,
        score: null,
      });
      setNewAttendee("");
      await load();
    } catch {
      // Reported by the global toast.
    }
  };

  const recordResult = async (a: TrainingAttendance, status: string, score: number | null) => {
    try {
      await upsertTrainingAttendance({
        id: a.id,
        expected_row_version: a.row_version,
        session_id: a.session_id,
        personnel_id: a.personnel_id,
        attendance_status: status,
        completed_at:
          status === "passed" || status === "failed" || status === "attended"
            ? (a.completed_at ?? selected?.scheduled_end ?? new Date().toISOString())
            : a.completed_at,
        score,
      });
      await load();
      onChanged?.();
    } catch {
      // Reported by the global toast.
    }
  };

  if (error && sessions.length === 0)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (loading && sessions.length === 0) return <LoadingState />;

  const registered = new Set(roster.map((a) => a.personnel_id));
  const instructor =
    selected?.instructor_id != null ? personById.get(selected.instructor_id) : undefined;
  const nameOf = (id: number): string => {
    const p = personById.get(id);
    return p ? personLabel(p) : "—";
  };

  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
      <section className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-semibold text-text-primary">{t("sessions.title")}</h3>
          <PermissionGate permission={P.TRN_MANAGE}>
            <Button
              className={mfLayout.primaryAction}
              onClick={() => setDraft(newDraft(null, certTypes))}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("sessions.add")}
            </Button>
          </PermissionGate>
        </div>
        {sessions.length === 0 ? (
          <EmptyState
            icon={<GraduationCap className="h-8 w-8" aria-hidden />}
            title={t("sessions.empty")}
            description={t("sessions.emptyHint")}
          />
        ) : (
          <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
            {sessions.map((s) => {
              const ct =
                s.certification_type_id != null ? certById.get(s.certification_type_id) : undefined;
              const upcoming = s.scheduled_start >= new Date().toISOString();
              return (
                <li key={s.id}>
                  <button
                    type="button"
                    onClick={() => setSelectedId(s.id)}
                    aria-pressed={s.id === selectedId}
                    className={cn(
                      "w-full px-3 py-2 text-left text-sm hover:bg-surface-2/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      s.id === selectedId && "bg-surface-2",
                    )}
                  >
                    <span className="flex items-center justify-between gap-2">
                      <span className="font-medium text-text-primary">
                        {ct?.name ?? s.course_code}
                      </span>
                      <StatusChip
                        label={upcoming ? t("sessions.upcoming") : t("sessions.past")}
                        tone={upcoming ? "info" : "neutral"}
                      />
                    </span>
                    <span className="block text-xs text-text-muted">
                      {dateFmt.format(new Date(s.scheduled_start))}
                      {s.location ? ` · ${s.location}` : ""}
                      {" · "}
                      {t("sessions.attendees", { count: countBySession.get(s.id) ?? 0 })}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section className="space-y-3">
        {selected == null ? (
          <p className="rounded-md border border-dashed border-surface-border p-6 text-center text-sm text-text-muted">
            {t("sessions.selectHint")}
          </p>
        ) : (
          <>
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div>
                <h3 className="text-sm font-semibold text-text-primary">
                  {selected.certification_type_id != null
                    ? (certById.get(selected.certification_type_id)?.name ?? selected.course_code)
                    : selected.course_code}
                </h3>
                <p className="text-xs text-text-muted">
                  {dateFmt.format(new Date(selected.scheduled_start))} →{" "}
                  {dateFmt.format(new Date(selected.scheduled_end))}
                  {instructor ? ` · ${t("sessions.instructor")} ${personLabel(instructor)}` : ""}
                  {` · ${t("sessions.passMark", { score: selected.min_pass_score })}`}
                </p>
                {selected.certification_type_id != null ? (
                  <p className="mt-1 text-xs text-text-muted">
                    {t("sessions.issuesCertification")}
                  </p>
                ) : null}
              </div>
              {canManage ? (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setDraft({
                      id: selected.id,
                      row_version: selected.row_version,
                      course_code: selected.course_code,
                      certification_type_id:
                        selected.certification_type_id != null
                          ? String(selected.certification_type_id)
                          : "",
                      start: toLocal(selected.scheduled_start),
                      end: toLocal(selected.scheduled_end),
                      location: selected.location ?? "",
                      instructor_id:
                        selected.instructor_id != null ? String(selected.instructor_id) : "",
                      min_pass_score: String(selected.min_pass_score),
                    })
                  }
                >
                  {t("sessions.edit")}
                </Button>
              ) : null}
            </div>

            {canManage ? (
              <div className="flex flex-wrap items-end gap-2">
                <FormField
                  name="attendee"
                  label={t("sessions.registerPerson")}
                  className="min-w-[240px] flex-1"
                >
                  <Select
                    value={newAttendee}
                    onValueChange={setNewAttendee}
                    disabled={peopleRestricted}
                  >
                    <SelectTrigger id="attendee">
                      <SelectValue placeholder={t("sessions.choosePerson")} />
                    </SelectTrigger>
                    <SelectContent>
                      {people
                        .filter((p) => !registered.has(p.id))
                        .map((p) => (
                          <SelectItem key={p.id} value={String(p.id)}>
                            {personLabel(p)}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                  {peopleRestricted ? (
                    <p className="mt-1 text-xs text-text-muted">{t("sessions.peopleRestricted")}</p>
                  ) : null}
                </FormField>
                <Button
                  variant="outline"
                  className="h-10 gap-1"
                  disabled={newAttendee === ""}
                  onClick={() => void register()}
                >
                  <UserPlus className="h-4 w-4" aria-hidden />
                  {t("sessions.register")}
                </Button>
              </div>
            ) : null}

            {roster.length === 0 ? (
              <p className="text-sm text-text-muted">{t("sessions.noAttendees")}</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                    <th className="py-1.5 pr-2 font-medium">{t("col.person")}</th>
                    <th className="py-1.5 pr-2 font-medium">{t("col.status")}</th>
                    <th className="py-1.5 pr-2 font-medium">{t("col.score")}</th>
                    <th className="py-1.5 font-medium">
                      <span className="sr-only">{t("col.actions")}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {roster.map((a) => (
                    <AttendanceRow
                      key={a.id}
                      attendance={a}
                      personName={nameOf(a.personnel_id)}
                      passMark={selected.min_pass_score}
                      issuesCertification={selected.certification_type_id != null}
                      canManage={canManage}
                      onRecord={recordResult}
                    />
                  ))}
                </tbody>
              </table>
            )}
          </>
        )}
      </section>

      <Dialog open={draft != null} onOpenChange={(o) => !o && !saving && setDraft(null)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {draft?.id ? t("sessions.editTitle") : t("sessions.addTitle")}
            </DialogTitle>
            <DialogDescription>{t("sessions.formDescription")}</DialogDescription>
          </DialogHeader>
          {draft ? (
            <div className="grid gap-3 sm:grid-cols-2">
              <FormField name="ts-cert" label={t("sessions.leadsTo")} className="sm:col-span-2">
                <Select
                  value={draft.certification_type_id || "__none__"}
                  onValueChange={(v) => {
                    const id = v === "__none__" ? "" : v;
                    const ct = certById.get(Number(id));
                    setDraft({
                      ...draft,
                      certification_type_id: id,
                      course_code: draft.course_code || (ct?.code ?? ""),
                    });
                  }}
                >
                  <SelectTrigger id="ts-cert">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none__">{t("sessions.noCertification")}</SelectItem>
                    {certTypes
                      .filter((c) => c.is_active)
                      .map((c) => (
                        <SelectItem key={c.id} value={String(c.id)}>
                          {c.name}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField
                name="ts-course"
                label={t("sessions.course")}
                required
                className="sm:col-span-2"
              >
                <Input
                  id="ts-course"
                  value={draft.course_code}
                  onChange={(e) => setDraft({ ...draft, course_code: e.target.value })}
                />
              </FormField>
              <DateTimeLocalField
                id="ts-start"
                label={t("sessions.start")}
                value={draft.start}
                onChange={(v) => setDraft({ ...draft, start: v })}
              />
              <DateTimeLocalField
                id="ts-end"
                label={t("sessions.end")}
                value={draft.end}
                minDate={draft.start.slice(0, 10)}
                onChange={(v) => setDraft({ ...draft, end: v })}
              />
              {draft.end !== "" && draft.start !== "" && draft.end <= draft.start ? (
                <p className="text-sm text-status-danger sm:col-span-2" role="alert">
                  {t("sessions.endBeforeStart")}
                </p>
              ) : null}
              <FormField name="ts-location" label={t("sessions.location")}>
                <Input
                  id="ts-location"
                  value={draft.location}
                  onChange={(e) => setDraft({ ...draft, location: e.target.value })}
                />
              </FormField>
              <FormField name="ts-pass" label={t("sessions.passScore")}>
                <Input
                  id="ts-pass"
                  inputMode="numeric"
                  value={draft.min_pass_score}
                  onChange={(e) => setDraft({ ...draft, min_pass_score: e.target.value })}
                />
              </FormField>
              <FormField
                name="ts-instructor"
                label={t("sessions.instructor")}
                className="sm:col-span-2"
              >
                <Select
                  value={draft.instructor_id || "__none__"}
                  onValueChange={(v) =>
                    setDraft({ ...draft, instructor_id: v === "__none__" ? "" : v })
                  }
                >
                  <SelectTrigger id="ts-instructor">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none__">{t("sessions.externalInstructor")}</SelectItem>
                    {people.map((p) => (
                      <SelectItem key={p.id} value={String(p.id)}>
                        {personLabel(p)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {peopleRestricted ? (
                  <p className="mt-1 text-xs text-text-muted">{t("sessions.peopleRestricted")}</p>
                ) : null}
              </FormField>
            </div>
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              onClick={() => setDraft(null)}
              disabled={saving}
            >
              {t("action.cancel")}
            </Button>
            <Button
              className="h-11"
              onClick={() => void saveSession()}
              disabled={!draftValid || saving}
            >
              {t("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function AttendanceRow({
  attendance,
  personName,
  passMark,
  issuesCertification,
  canManage,
  onRecord,
}: {
  attendance: TrainingAttendance;
  personName: string;
  passMark: number;
  /** Recording "passed" issues the session's certification (backend). */
  issuesCertification: boolean;
  canManage: boolean;
  onRecord: (a: TrainingAttendance, status: string, score: number | null) => Promise<void>;
}) {
  const { t } = useTranslation("training");
  const [score, setScore] = useState(attendance.score != null ? String(attendance.score) : "");
  const parsed = score.trim() === "" ? null : Number(score);
  const scoreValid = parsed == null || (Number.isFinite(parsed) && parsed >= 0 && parsed <= 100);
  const belowPassMark = parsed != null && scoreValid && parsed < passMark;
  /** A result waiting for confirmation: a certificate issue, or a "passed" turned into "failed". */
  const [pending, setPending] = useState<{ status: string; demoted: boolean } | null>(null);

  const choose = (status: string) => {
    if (!scoreValid) return;
    // A score below the pass mark cannot be recorded as passed: it is recorded as failed, and the
    // user is told so before anything is saved.
    if (status === "passed" && belowPassMark) {
      setPending({ status: "failed", demoted: true });
      return;
    }
    // Passing issues the certification: ask first.
    if (status === "passed" && issuesCertification) {
      setPending({ status: "passed", demoted: false });
      return;
    }
    void onRecord(attendance, status, parsed);
  };

  return (
    <tr className="border-b border-surface-border/60">
      <td className="py-1.5 pr-2">{personName}</td>
      <td className="py-1.5 pr-2">
        <StatusChip
          label={t(`attendance.${attendance.attendance_status}`, {
            defaultValue: attendance.attendance_status,
          })}
          tone={ATTENDANCE_TONE[attendance.attendance_status] ?? "neutral"}
          dot
        />
      </td>
      <td className="py-1.5 pr-2">
        {canManage ? (
          <div>
            <Input
              aria-label={t("col.score")}
              className="h-9 w-20"
              inputMode="decimal"
              value={score}
              onChange={(e) => setScore(e.target.value)}
            />
            {belowPassMark ? (
              <p className="mt-0.5 text-xs text-tone-warning-fg">
                {t("sessions.belowPassMark", { score: passMark })}
              </p>
            ) : null}
          </div>
        ) : (
          (attendance.score ?? "—")
        )}
      </td>
      <td className="py-1.5 text-right">
        {canManage ? (
          <Select value="" onValueChange={choose}>
            <SelectTrigger
              className="h-9 w-40"
              aria-label={t("sessions.recordResult", { person: personName })}
            >
              <SelectValue placeholder={t("sessions.recordResultShort")} />
            </SelectTrigger>
            <SelectContent>
              {ATTENDANCE_STATUSES.map((s) => (
                <SelectItem key={s} value={s}>
                  {t(`attendance.${s}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}
        <ConfirmDialog
          open={pending != null}
          onOpenChange={(o) => !o && setPending(null)}
          title={
            pending?.demoted
              ? t("sessions.recordedAsFailedTitle")
              : t("sessions.issueCertificateTitle")
          }
          description={
            pending?.demoted
              ? t("sessions.recordedAsFailed", { person: personName, score: parsed, passMark })
              : t("sessions.issueCertificate", { person: personName })
          }
          confirmLabel={
            pending?.demoted ? t("sessions.recordAsFailed") : t("sessions.recordPassed")
          }
          onConfirm={async () => {
            if (pending) await onRecord(attendance, pending.status, parsed);
          }}
        />
      </td>
    </tr>
  );
}
