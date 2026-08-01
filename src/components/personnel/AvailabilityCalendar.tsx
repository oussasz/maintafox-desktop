import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { createAvailabilityBlock, listAvailabilityCalendar } from "@/services/personnel-service";
import type { AvailabilityCalendarEntry } from "@shared/ipc-types";

interface AvailabilityCalendarProps {
  entityId?: number | null;
  teamId?: number | null;
}

function toIsoDate(value: Date): string {
  return value.toISOString().slice(0, 10);
}

export function AvailabilityCalendar({ entityId, teamId }: AvailabilityCalendarProps) {
  const { t } = useTranslation("personnel");
  const [rows, setRows] = useState<AvailabilityCalendarEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createError, setCreateError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);
  const [dateFrom, setDateFrom] = useState(() => toIsoDate(new Date()));
  const [dateTo, setDateTo] = useState(() => {
    const d = new Date();
    d.setDate(d.getDate() + 6);
    return toIsoDate(d);
  });
  const [newBlockPersonnelId, setNewBlockPersonnelId] = useState<number | null>(null);
  const [newBlockType, setNewBlockType] = useState("medical");
  const [newBlockStart, setNewBlockStart] = useState("");
  const [newBlockEnd, setNewBlockEnd] = useState("");
  const [newBlockReason, setNewBlockReason] = useState("");
  const [newBlockCritical, setNewBlockCritical] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listAvailabilityCalendar({
      date_from: dateFrom,
      date_to: dateTo,
      entity_id: entityId ?? null,
      team_id: teamId ?? null,
      include_inactive: false,
    })
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dateFrom, dateTo, entityId, teamId, reloadToken]);

  const grouped = useMemo(() => {
    return rows.reduce<Record<string, AvailabilityCalendarEntry[]>>((acc, row) => {
      const bucket = acc[row.work_date] ?? [];
      bucket.push(row);
      acc[row.work_date] = bucket;
      return acc;
    }, {});
  }, [rows]);

  const personnelOptions = useMemo(() => {
    const map = new Map<number, { id: number; label: string }>();
    for (const row of rows) {
      if (!map.has(row.personnel_id)) {
        map.set(row.personnel_id, {
          id: row.personnel_id,
          label: `${row.full_name} (${row.employee_code})`,
        });
      }
    }
    return [...map.values()].sort((a, b) => a.label.localeCompare(b.label));
  }, [rows]);

  useEffect(() => {
    if (newBlockPersonnelId == null && personnelOptions.length > 0) {
      const first = personnelOptions[0];
      if (first) setNewBlockPersonnelId(first.id);
    }
  }, [personnelOptions, newBlockPersonnelId]);

  const submitNewBlock = async () => {
    if (newBlockPersonnelId == null) {
      setCreateError(t("availability.form.errors.personnelRequired"));
      return;
    }
    if (!newBlockStart || !newBlockEnd) {
      setCreateError(t("availability.form.errors.timeRequired"));
      return;
    }
    const startIso = new Date(newBlockStart).toISOString();
    const endIso = new Date(newBlockEnd).toISOString();
    if (startIso >= endIso) {
      setCreateError(t("availability.form.errors.invalidRange"));
      return;
    }

    setCreating(true);
    setCreateError(null);
    try {
      await createAvailabilityBlock({
        personnel_id: newBlockPersonnelId,
        block_type: newBlockType,
        start_at: startIso,
        end_at: endIso,
        reason_note: newBlockReason.trim() || null,
        is_critical: newBlockCritical,
      });
      setNewBlockReason("");
      setReloadToken((v) => v + 1);
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : String(err));
    } finally {
      setCreating(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg">{t("tabs.availabilityCalendar")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap gap-3">
          <label className="flex items-center gap-2 text-sm">
            {t("availability.from")}
            <Input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="w-44" />
          </label>
          <label className="flex items-center gap-2 text-sm">
            {t("availability.to")}
            <Input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="w-44" />
          </label>
        </div>

        <div className="rounded border p-3">
          <div className="mb-2 text-sm font-medium">{t("availability.form.title")}</div>
          <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
            <label className="text-sm">
              <span className="mb-1 block text-text-muted">{t("availability.form.personnel")}</span>
              <select
                className="h-9 w-full rounded border bg-background px-2"
                value={newBlockPersonnelId ?? ""}
                onChange={(e) => setNewBlockPersonnelId(Number(e.target.value))}
              >
                {personnelOptions.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="text-sm">
              <span className="mb-1 block text-text-muted">{t("availability.form.blockType")}</span>
              <select
                className="h-9 w-full rounded border bg-background px-2"
                value={newBlockType}
                onChange={(e) => setNewBlockType(e.target.value)}
              >
                <option value="medical">{t("availability.form.types.medical")}</option>
                <option value="restriction">{t("availability.form.types.restriction")}</option>
                <option value="leave">{t("availability.form.types.leave")}</option>
                <option value="training">{t("availability.form.types.training")}</option>
              </select>
            </label>

            <label className="text-sm">
              <span className="mb-1 block text-text-muted">{t("availability.form.startAt")}</span>
              <Input
                type="datetime-local"
                value={newBlockStart}
                onChange={(e) => setNewBlockStart(e.target.value)}
              />
            </label>

            <label className="text-sm">
              <span className="mb-1 block text-text-muted">{t("availability.form.endAt")}</span>
              <Input
                type="datetime-local"
                value={newBlockEnd}
                onChange={(e) => setNewBlockEnd(e.target.value)}
              />
            </label>

            <label className="text-sm xl:col-span-2">
              <span className="mb-1 block text-text-muted">{t("availability.form.reason")}</span>
              <Input value={newBlockReason} onChange={(e) => setNewBlockReason(e.target.value)} />
            </label>
          </div>

          <label className="mt-3 flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={newBlockCritical}
              onChange={(e) => setNewBlockCritical(e.target.checked)}
            />
            {t("availability.form.isCritical")}
          </label>

          <div className="mt-3 flex items-center gap-2">
            <Button type="button" size="sm" onClick={() => void submitNewBlock()} disabled={creating}>
              {creating ? t("common.loading") : t("availability.form.submit")}
            </Button>
            {createError ? <span className="text-sm text-destructive">{createError}</span> : null}
          </div>
        </div>

        {error ? <div className="text-sm text-destructive">{error}</div> : null}
        {loading ? <div className="text-sm text-text-muted">{t("common.loading")}</div> : null}

        <div className="space-y-3">
          {Object.entries(grouped).length === 0 && !loading ? (
            <div className="text-sm text-text-muted">{t("common.noData")}</div>
          ) : null}
          {Object.entries(grouped).map(([day, dayRows]) => (
            <div key={day} className="rounded border p-3">
              <div className="mb-2 text-sm font-medium">{day}</div>
              <div className="space-y-1">
                {dayRows.map((row) => (
                  <div key={`${row.personnel_id}-${row.work_date}`} className="grid grid-cols-12 gap-2 text-sm">
                    <div className="col-span-4">
                      {row.full_name}
                      <span className="ml-2 text-xs text-text-muted">{row.employee_code}</span>
                    </div>
                    <div className="col-span-3 text-text-muted">{row.team_name ?? " "}</div>
                    <div className="col-span-2">{row.available_minutes}m</div>
                    <div className="col-span-2 text-text-muted">{row.blocked_minutes}m blocked</div>
                    <div className="col-span-1 text-right">
                      {row.has_critical_block ? (
                        <span className="rounded bg-destructive/10 px-2 py-0.5 text-xs text-destructive">!</span>
                      ) : null}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}


