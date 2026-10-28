/**
 * The site calendar's working week: the weekly rest days of every site (Friday–Saturday by
 * default, the Algerian week-end) and of the sites that set their own, from a date. Capacity
 * counts these days as non-working except for continuous Classes horaires (3×8, 24/7).
 */
import { CalendarRange } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type SiteWorkWeek,
  listSiteWorkWeeks,
  setSiteWorkWeek,
} from "@/services/workforce-service";
import { P } from "@shared/rbac/permissions.generated";

const ALL_SITES = "__all__";
const ISO_WEEK = [1, 2, 3, 4, 5, 6, 7];

function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

interface Draft {
  entity: string;
  from: string;
  rest: number[];
}

export function SiteWorkWeekPanel({ entities }: { entities: { id: number; name: string }[] }) {
  const { t, i18n } = useTranslation("personnel");
  const today = todayIso();
  const [rows, setRows] = useState<SiteWorkWeek[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const weekday = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { weekday: "long", timeZone: "UTC" }),
    [i18n.language],
  );
  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeZone: "UTC" }),
    [i18n.language],
  );
  // 2024-01-01 is a Monday: ISO weekday n is 2024-01-0n.
  const dayName = (n: number) => weekday.format(new Date(Date.UTC(2024, 0, n)));

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await listSiteWorkWeeks());
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  /** The row in force today for each site (and for all sites). */
  const inForce = useMemo(() => {
    const byScope = new Map<string, SiteWorkWeek>();
    for (const r of rows) {
      if (r.effective_from > today) continue;
      const key = r.entity_id == null ? ALL_SITES : String(r.entity_id);
      const prev = byScope.get(key);
      if (!prev || prev.effective_from <= r.effective_from) byScope.set(key, r);
    }
    return new Set([...byScope.values()].map((r) => r.id));
  }, [rows, today]);

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setSaveError(null);
    try {
      setRows(
        await setSiteWorkWeek({
          entity_id: draft.entity === ALL_SITES ? null : Number(draft.entity),
          effective_from: draft.from,
          weekend_days: draft.rest,
        }),
      );
      setDraft(null);
    } catch (e) {
      setSaveError(appErrorReason(e));
    } finally {
      setSaving(false);
    }
  };

  const draftValid =
    draft != null &&
    draft.from >= today &&
    draft.rest.length < 7 &&
    /^\d{4}-\d{2}-\d{2}$/.test(draft.from);

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("siteWeek.title")}</h3>
        <PermissionGate permission={P.PER_MANAGE}>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setSaveError(null);
              setDraft({ entity: ALL_SITES, from: today, rest: [5, 6] });
            }}
          >
            <CalendarRange className="h-4 w-4" aria-hidden />
            {t("siteWeek.change")}
          </Button>
        </PermissionGate>
      </div>
      <p className="text-sm text-text-muted">{t("siteWeek.intro")}</p>
      {error && rows.length === 0 ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : loading && rows.length === 0 ? (
        <LoadingState />
      ) : (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {rows.map((r) => (
            <li key={r.id} className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
              <span className="w-48 font-medium text-text-primary">
                {r.entity_name ?? t("holidays.allSites")}
              </span>
              <span className="w-40 text-text-secondary">
                {t("siteWeek.fromDate", {
                  date: dateFmt.format(new Date(`${r.effective_from}T00:00:00Z`)),
                })}
              </span>
              <span className="flex-1 capitalize">
                {r.weekend_days.length === 0
                  ? t("siteWeek.noRestDay")
                  : r.weekend_days.map(dayName).join(", ")}
              </span>
              <StatusChip
                label={t(
                  inForce.has(r.id)
                    ? "timetable.editor.inForce"
                    : r.effective_from > today
                      ? "timetable.editor.planned"
                      : "timetable.editor.past",
                )}
                tone={inForce.has(r.id) ? "success" : r.effective_from > today ? "info" : "neutral"}
                dot
              />
            </li>
          ))}
        </ul>
      )}

      <Dialog open={draft != null} onOpenChange={(o) => !o && !saving && setDraft(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("siteWeek.formTitle")}</DialogTitle>
            <DialogDescription>{t("siteWeek.formDescription")}</DialogDescription>
          </DialogHeader>
          {draft ? (
            <div className="grid gap-3">
              <FormField name="site-week-scope" label={t("holidays.scope")}>
                <Select
                  value={draft.entity}
                  onValueChange={(v) => setDraft({ ...draft, entity: v })}
                >
                  <SelectTrigger id="site-week-scope">
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
              <FormField name="site-week-from" label={t("timetable.editor.effectiveFrom")} required>
                <DateField
                  id="site-week-from"
                  min={today}
                  value={draft.from}
                  onChange={(v) => setDraft({ ...draft, from: v })}
                />
              </FormField>
              <fieldset className="space-y-1.5">
                <legend className="text-sm font-medium">{t("siteWeek.restDays")}</legend>
                <div className="grid grid-cols-2 gap-1.5">
                  {ISO_WEEK.map((n) => (
                    <label key={n} className="flex items-center gap-2 text-sm capitalize">
                      <Checkbox
                        checked={draft.rest.includes(n)}
                        onCheckedChange={(v) =>
                          setDraft({
                            ...draft,
                            rest:
                              v === true
                                ? [...draft.rest, n].sort((a, b) => a - b)
                                : draft.rest.filter((x) => x !== n),
                          })
                        }
                      />
                      {dayName(n)}
                    </label>
                  ))}
                </div>
              </fieldset>
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
    </section>
  );
}
