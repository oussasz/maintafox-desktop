import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { listTeamCapacitySummary, scanSuccessionRisk } from "@/services/personnel-service";
import type { SuccessionRiskRow, TeamCapacitySummaryRow } from "@shared/ipc-types";

interface TeamCapacityBoardProps {
  entityId?: number | null;
}

function toIsoDate(value: Date): string {
  return value.toISOString().slice(0, 10);
}

export function TeamCapacityBoard({ entityId }: TeamCapacityBoardProps) {
  const { t } = useTranslation("personnel");
  const [rows, setRows] = useState<TeamCapacitySummaryRow[]>([]);
  const [riskRows, setRiskRows] = useState<SuccessionRiskRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dateFrom, setDateFrom] = useState(() => toIsoDate(new Date()));
  const [dateTo, setDateTo] = useState(() => {
    const d = new Date();
    d.setDate(d.getDate() + 6);
    return toIsoDate(d);
  });

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listTeamCapacitySummary({
      date_from: dateFrom,
      date_to: dateTo,
      entity_id: entityId ?? null,
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
    void scanSuccessionRisk(entityId ?? null, null).then((data) => {
      if (!cancelled) setRiskRows(data.slice(0, 8));
    });
    return () => {
      cancelled = true;
    };
  }, [dateFrom, dateTo, entityId]);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg">{t("tabs.teamCapacity")}</CardTitle>
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

        {error ? <div className="text-sm text-destructive">{error}</div> : null}
        {loading ? <div className="text-sm text-text-muted">{t("common.loading")}</div> : null}

        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {!loading && rows.length === 0 ? (
            <div className="text-sm text-text-muted">{t("common.noData")}</div>
          ) : null}
          {rows.map((row) => (
            <div key={row.team_id} className="rounded border p-3">
              <div className="text-sm font-semibold">
                {row.team_name} <span className="text-xs text-text-muted">({row.team_code})</span>
              </div>
              <div className="mt-2 space-y-1 text-sm">
                <div>{t("capacity.memberCount", { count: row.member_count })}</div>
                <div>{t("capacity.leadCount", { count: row.lead_count })}</div>
                <div>{t("capacity.availableMinutes", { count: row.total_available_minutes })}</div>
                <div>{t("capacity.blockedMinutes", { count: row.total_blocked_minutes })}</div>
                <div>{t("capacity.ratio", { value: Math.round(row.avg_availability_ratio * 100) })}%</div>
              </div>
            </div>
          ))}
        </div>

        <div className="space-y-2">
          <div className="text-sm font-medium">{t("capacity.successionRisk", "Succession risk")}</div>
          {riskRows.length === 0 ? (
            <div className="text-sm text-text-muted">{t("common.noData")}</div>
          ) : (
            <div className="grid gap-2 md:grid-cols-2">
              {riskRows.map((row) => (
                <div key={row.personnel_id} className="rounded border p-2 text-sm">
                  <div className="font-medium">{row.full_name}</div>
                  <div className="text-text-muted">{row.position_name ?? "—"} · {row.team_name ?? "—"}</div>
                  <div className="text-xs">{row.risk_level.toUpperCase()} · {row.reason}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}


