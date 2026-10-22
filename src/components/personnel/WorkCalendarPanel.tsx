/**
 * Site calendar: the weekly rest days of the sites (SiteWorkWeekPanel), then public holidays and
 * shutdown days entered year by year with French and English names, for every site or for one
 * organisation node and everything under it. Capacity counts them as non-working days, except
 * for continuous Classes horaires.
 */
import { CalendarOff, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
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
import { Switch } from "@/components/ui/switch";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type WorkCalendarHoliday,
  listWorkCalendarHolidays,
  upsertWorkCalendarHoliday,
} from "@/services/workforce-service";
import { P } from "@shared/rbac/permissions.generated";

import { SiteWorkWeekPanel } from "./SiteWorkWeekPanel";

const ALL_SITES = "__all__";

interface Draft {
  id: number | null;
  row_version: number | null;
  date: string;
  label: string;
  labelEn: string;
  entity: string;
  active: boolean;
}

export interface WorkCalendarPanelProps {
  /** Organisation nodes a holiday can be limited to. */
  entities: { id: number; name: string }[];
}

export function WorkCalendarPanel({ entities }: WorkCalendarPanelProps) {
  const { t, i18n } = useTranslation("personnel");
  const english = i18n.language.startsWith("en");
  const { can } = usePermissions();
  const canManage = can(P.PER_MANAGE);
  const thisYear = new Date().getFullYear();
  const [year, setYear] = useState(thisYear);
  const [rows, setRows] = useState<WorkCalendarHoliday[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const fmt = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.language, {
        weekday: "long",
        day: "numeric",
        month: "long",
        timeZone: "UTC",
      }),
    [i18n.language],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(
        await listWorkCalendarHolidays({
          date_from: `${year}-01-01`,
          date_to: `${year}-12-31`,
          include_inactive: true,
        }),
      );
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [year]);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!draft || draft.date === "" || draft.label.trim() === "") return;
    setSaving(true);
    try {
      await upsertWorkCalendarHoliday({
        id: draft.id,
        expected_row_version: draft.row_version,
        holiday_date: draft.date,
        label: draft.label.trim(),
        label_en: draft.labelEn.trim() || null,
        entity_id: draft.entity === ALL_SITES ? null : Number(draft.entity),
        is_active: draft.active,
      });
      setDraft(null);
      await load();
    } catch {
      // Reported by the global toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-3">
      <SiteWorkWeekPanel entities={entities} />
      <h3 className="pt-2 text-sm font-semibold text-text-primary">{t("holidays.title")}</h3>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <label htmlFor="holiday-year" className="text-sm text-text-muted">
            {t("holidays.year")}
          </label>
          <Select value={String(year)} onValueChange={(v) => setYear(Number(v))}>
            <SelectTrigger id="holiday-year" className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {[thisYear - 1, thisYear, thisYear + 1, thisYear + 2].map((y) => (
                <SelectItem key={y} value={String(y)}>
                  {y}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <PermissionGate permission={P.PER_MANAGE}>
          <Button
            className={mfLayout.primaryAction}
            onClick={() =>
              setDraft({
                id: null,
                row_version: null,
                date: `${year}-01-01`,
                label: "",
                labelEn: "",
                entity: ALL_SITES,
                active: true,
              })
            }
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("holidays.add")}
          </Button>
        </PermissionGate>
      </div>
      <p className="text-sm text-text-muted">{t("holidays.intro")}</p>

      {error && rows.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : loading && rows.length === 0 ? (
        <LoadingState />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={<CalendarOff className="h-8 w-8" aria-hidden />}
          title={t("holidays.empty", { year })}
          description={t("holidays.emptyHint")}
        />
      ) : (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {rows.map((h) => (
            <li key={h.id} className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
              <span className="w-56 tabular-nums text-text-secondary">
                {fmt.format(new Date(`${h.holiday_date}T00:00:00Z`))}
              </span>
              <span className="min-w-[160px] flex-1">
                <span className="font-medium text-text-primary">
                  {english && h.label_en ? h.label_en : h.label}
                </span>
                {h.label_en ? (
                  <span className="ml-2 text-xs text-text-muted">
                    {english ? h.label : h.label_en}
                  </span>
                ) : null}
              </span>
              <span className="text-text-muted">{h.entity_name ?? t("holidays.allSites")}</span>
              {!h.is_active ? <StatusChip label={t("holidays.cancelled")} tone="neutral" /> : null}
              {canManage ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    setDraft({
                      id: h.id,
                      row_version: h.row_version,
                      date: h.holiday_date,
                      label: h.label,
                      labelEn: h.label_en ?? "",
                      entity: h.entity_id != null ? String(h.entity_id) : ALL_SITES,
                      active: h.is_active,
                    })
                  }
                >
                  {t("holidays.edit")}
                </Button>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      <Dialog open={draft != null} onOpenChange={(o) => !o && !saving && setDraft(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {draft?.id ? t("holidays.editTitle") : t("holidays.addTitle")}
            </DialogTitle>
            <DialogDescription>{t("holidays.formDescription")}</DialogDescription>
          </DialogHeader>
          {draft ? (
            <div className="grid gap-3">
              <FormField name="holiday-date" label={t("holidays.date")} required>
                <DateField
                  id="holiday-date"
                  value={draft.date}
                  onChange={(v) => setDraft({ ...draft, date: v })}
                />
              </FormField>
              <FormField name="holiday-label" label={t("holidays.labelFr")} required>
                <Input
                  id="holiday-label"
                  lang="fr"
                  value={draft.label}
                  onChange={(e) => setDraft({ ...draft, label: e.target.value })}
                />
              </FormField>
              <FormField name="holiday-label-en" label={t("holidays.labelEn")}>
                <Input
                  id="holiday-label-en"
                  lang="en"
                  value={draft.labelEn}
                  onChange={(e) => setDraft({ ...draft, labelEn: e.target.value })}
                />
              </FormField>
              <FormField name="holiday-scope" label={t("holidays.scope")}>
                <Select
                  value={draft.entity}
                  onValueChange={(v) => setDraft({ ...draft, entity: v })}
                >
                  <SelectTrigger id="holiday-scope">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={ALL_SITES}>{t("holidays.allSites")}</SelectItem>
                    {entities.map((e) => (
                      <SelectItem key={e.id} value={String(e.id)}>
                        {e.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>
              {draft.id != null ? (
                <label className="flex items-center gap-2 text-sm">
                  <Switch
                    checked={draft.active}
                    onCheckedChange={(v) => setDraft({ ...draft, active: v })}
                    aria-label={t("holidays.active")}
                  />
                  {t("holidays.active")}
                </label>
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
            <Button
              className="h-11"
              onClick={() => void save()}
              disabled={saving || !draft || draft.date === "" || draft.label.trim() === ""}
            >
              {t("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
