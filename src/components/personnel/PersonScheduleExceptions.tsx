/**
 * A person's dated timetable exceptions: another Classe horaire than the team's, and/or a
 * part-time percentage, over a period. One not yet started can be changed or removed; one that
 * has started can only be ended (past capacity is never rewritten). Absences (leave, training,
 * sickness) are recorded as availability blocks, listed below on the same sheet.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type PersonnelScheduleException,
  deletePersonnelScheduleException,
  listPersonnelScheduleExceptions,
  listScheduleClassVersions,
  upsertPersonnelScheduleException,
} from "@/services/workforce-service";
import { P } from "@shared/rbac/permissions.generated";

const TEAM_CLASS = "__team__";

function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

interface Draft {
  id: number | null;
  rowVersion: number | null;
  started: boolean;
  classId: string;
  workPercent: string;
  from: string;
  to: string;
  reason: string;
}

export function PersonScheduleExceptions({ personnelId }: { personnelId: number }) {
  const { t, i18n } = useTranslation("personnel");
  const { can } = usePermissions();
  const canManage = can(P.PER_MANAGE);
  const today = todayIso();
  const [rows, setRows] = useState<PersonnelScheduleException[]>([]);
  const [classes, setClasses] = useState<{ id: number; label: string }[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeZone: "UTC" }),
    [i18n.language],
  );
  const fmt = (iso: string) => dateFmt.format(new Date(`${iso}T00:00:00Z`));

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [list, versions] = await Promise.all([
        listPersonnelScheduleExceptions(personnelId),
        listScheduleClassVersions(null),
      ]);
      setRows(list);
      const seen = new Map<number, string>();
      for (const v of versions) seen.set(v.reference_value_id, v.class_label);
      setClasses(
        [...seen.entries()]
          .map(([id, label]) => ({ id, label }))
          .sort((a, b) => a.label.localeCompare(b.label)),
      );
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [personnelId]);

  useEffect(() => {
    void load();
  }, [load]);

  const percent = draft?.workPercent.trim() ? Number(draft.workPercent) : null;
  const draftValid =
    draft != null &&
    (draft.classId !== TEAM_CLASS || percent != null) &&
    (percent == null || (percent > 0 && percent <= 100)) &&
    /^\d{4}-\d{2}-\d{2}$/.test(draft.from) &&
    (draft.started || draft.from >= today) &&
    (draft.to === "" ||
      (draft.to >= draft.from && draft.to >= (draft.started ? today : draft.from)));

  const save = async () => {
    if (!draft || !draftValid) return;
    setSaving(true);
    setSaveError(null);
    try {
      await upsertPersonnelScheduleException({
        id: draft.id,
        expected_row_version: draft.rowVersion,
        personnel_id: personnelId,
        schedule_reference_value_id: draft.classId === TEAM_CLASS ? null : Number(draft.classId),
        work_percent: percent,
        valid_from: draft.from,
        valid_to: draft.to || null,
        reason: draft.reason.trim() || null,
      });
      setDraft(null);
      await load();
    } catch (e) {
      setSaveError(appErrorReason(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: number) => {
    setSaving(true);
    setSaveError(null);
    try {
      await deletePersonnelScheduleException(id);
      await load();
    } catch (e) {
      setSaveError(appErrorReason(e));
    } finally {
      setSaving(false);
    }
  };

  const edit = (x: PersonnelScheduleException) => {
    setSaveError(null);
    setDraft({
      id: x.id,
      rowVersion: x.row_version,
      started: x.valid_from <= today,
      classId:
        x.schedule_reference_value_id != null ? String(x.schedule_reference_value_id) : TEAM_CLASS,
      workPercent: x.work_percent != null ? String(x.work_percent) : "",
      from: x.valid_from,
      to: x.valid_to ?? "",
      reason: x.reason ?? "",
    });
  };

  if (error && rows.length === 0) return <ErrorState error={error} onRetry={() => void load()} />;
  if (loading && rows.length === 0) return <LoadingState />;

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-sm font-semibold text-text-primary">
          {t("timetable.exceptions.title")}
        </h4>
        {canManage ? (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setSaveError(null);
              setDraft({
                id: null,
                rowVersion: null,
                started: false,
                classId: TEAM_CLASS,
                workPercent: "",
                from: today,
                to: "",
                reason: "",
              });
            }}
          >
            {t("timetable.exceptions.add")}
          </Button>
        ) : null}
      </div>
      <p className="text-xs text-text-muted">{t("timetable.exceptions.intro")}</p>
      {rows.length === 0 ? (
        <p className="text-sm text-text-muted">{t("timetable.exceptions.empty")}</p>
      ) : (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {rows.map((x) => {
            const started = x.valid_from <= today;
            return (
              <li key={x.id} className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
                <span className="w-56 text-text-secondary">
                  {x.valid_to
                    ? t("timetable.exceptions.period", {
                        from: fmt(x.valid_from),
                        to: fmt(x.valid_to),
                      })
                    : t("timetable.exceptions.openPeriod", { from: fmt(x.valid_from) })}
                </span>
                <span className="flex-1">
                  {[
                    x.schedule_label
                      ? t("timetable.exceptions.otherClass", { name: x.schedule_label })
                      : null,
                    x.work_percent != null
                      ? t("timetable.exceptions.partTime", { percent: x.work_percent })
                      : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                  {x.reason ? (
                    <span className="ml-2 text-xs text-text-muted">{x.reason}</span>
                  ) : null}
                </span>
                {canManage ? (
                  <span className="flex gap-1">
                    <Button size="sm" variant="ghost" onClick={() => edit(x)}>
                      {started ? t("timetable.exceptions.end") : t("timetable.editor.edit")}
                    </Button>
                    {!started ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={saving}
                        onClick={() => void remove(x.id)}
                      >
                        {t("timetable.exceptions.remove")}
                      </Button>
                    ) : null}
                  </span>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
      {saveError && draft == null ? (
        <p className={cn("text-xs", mfTone.danger.text)} role="alert">
          {saveError}
        </p>
      ) : null}

      <Dialog open={draft != null} onOpenChange={(o) => !o && !saving && setDraft(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {draft?.started
                ? t("timetable.exceptions.endTitle")
                : draft?.id
                  ? t("timetable.exceptions.editTitle")
                  : t("timetable.exceptions.addTitle")}
            </DialogTitle>
            <DialogDescription>
              {draft?.started
                ? t("timetable.exceptions.endHint")
                : t("timetable.exceptions.formHint")}
            </DialogDescription>
          </DialogHeader>
          {draft ? (
            <div className="grid gap-3">
              <FormField name="exception-class" label={t("timetable.exceptions.classLabel")}>
                <Select
                  value={draft.classId}
                  disabled={draft.started}
                  onValueChange={(v) => setDraft({ ...draft, classId: v })}
                >
                  <SelectTrigger id="exception-class">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={TEAM_CLASS}>
                      {t("timetable.exceptions.keepTeamClass")}
                    </SelectItem>
                    {classes.map((c) => (
                      <SelectItem key={c.id} value={String(c.id)}>
                        {c.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              <FormField
                name="exception-percent"
                label={t("timetable.exceptions.percentLabel")}
                description={t("timetable.exceptions.percentHelp")}
              >
                <Input
                  id="exception-percent"
                  type="number"
                  min={1}
                  max={100}
                  disabled={draft.started}
                  value={draft.workPercent}
                  onChange={(e) => setDraft({ ...draft, workPercent: e.target.value })}
                  className="w-32"
                />
              </FormField>
              <div className="grid grid-cols-2 gap-3">
                <FormField name="exception-from" label={t("timetable.exceptions.from")} required>
                  <DateField
                    id="exception-from"
                    min={today}
                    disabled={draft.started}
                    value={draft.from}
                    onChange={(v) => setDraft({ ...draft, from: v })}
                  />
                </FormField>
                <FormField name="exception-to" label={t("timetable.exceptions.to")}>
                  <DateField
                    id="exception-to"
                    min={draft.started ? today : draft.from}
                    value={draft.to}
                    onChange={(v) => setDraft({ ...draft, to: v })}
                  />
                </FormField>
              </div>
              {!draft.started ? (
                <FormField
                  name="exception-reason"
                  label={t("timetable.exceptions.reason")}
                  description={t("timetable.exceptions.reasonHelp")}
                >
                  <Input
                    id="exception-reason"
                    value={draft.reason}
                    onChange={(e) => setDraft({ ...draft, reason: e.target.value })}
                  />
                </FormField>
              ) : null}
              {saveError ? (
                <p className={cn("text-xs", mfTone.danger.text)} role="alert">
                  {saveError}
                </p>
              ) : null}
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
            <Button className="h-11" onClick={() => void save()} disabled={saving || !draftValid}>
              {t("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
