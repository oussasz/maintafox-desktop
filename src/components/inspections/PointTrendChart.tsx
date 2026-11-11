import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";
import {
  type InspectionPointReading,
  listInspectionPointTrend,
} from "@/services/inspection-service";

interface PointTrendChartProps {
  templateId: number;
  checkpointCode: string;
  /** Months shown (the readings of every round of the checklist, all versions). */
  months?: number;
}

const W = 520;
const H = 160;
const PAD = { left: 44, right: 12, top: 10, bottom: 22 };

/**
 * L1-19: the curve of one measurement point over the months, with its normal and warning limits,
 * so a drift shows before an alert. Readings come from every round of the checklist.
 */
export function PointTrendChart({ templateId, checkpointCode, months = 12 }: PointTrendChartProps) {
  const { t } = useTranslation("inspections");
  const { formatNumber, formatDate } = useFormatters();
  const [readings, setReadings] = useState<InspectionPointReading[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setReadings(null);
    setFailed(false);
    listInspectionPointTrend(templateId, checkpointCode, months * 31)
      .then((r) => {
        if (!cancelled) setReadings(r);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [templateId, checkpointCode, months]);

  const model = useMemo(() => {
    if (!readings || readings.length === 0) return null;
    const times = readings.map((r) => new Date(r.recorded_at).getTime());
    const last = readings[readings.length - 1];
    const limits = [
      last?.normal_min,
      last?.normal_max,
      last?.warning_min,
      last?.warning_max,
    ].filter((v): v is number => v != null);
    const values = [...readings.map((r) => r.numeric_value), ...limits];
    const minY = Math.min(...values);
    const maxY = Math.max(...values);
    const span = maxY - minY || 1;
    const minT = Math.min(...times);
    const spanT = Math.max(...times) - minT || 1;
    const x = (ms: number) => PAD.left + ((ms - minT) / spanT) * (W - PAD.left - PAD.right);
    const y = (v: number) => PAD.top + (1 - (v - minY) / span) * (H - PAD.top - PAD.bottom);
    return { times, last, minY, maxY, x, y };
  }, [readings]);

  if (failed) return <p className="text-xs text-tone-danger-fg">{t("trend.failed")}</p>;
  if (readings == null) return <p className="text-xs text-text-muted">{t("trend.loading")}</p>;
  if (!model || readings.length === 0) {
    return <p className="text-xs text-text-muted">{t("trend.none", { months })}</p>;
  }
  const { times, last, x, y, minY, maxY } = model;
  const unit = last?.measurement_unit ?? "";
  const limitLine = (v: number | null | undefined, cls: string, key: string) =>
    v == null ? null : (
      <line
        key={key}
        x1={PAD.left}
        x2={W - PAD.right}
        y1={y(v)}
        y2={y(v)}
        className={cls}
        strokeDasharray="4 3"
      />
    );
  return (
    <figure className="space-y-1" data-testid="point-trend">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="w-full max-w-xl"
        role="img"
        aria-label={t("trend.aria", { code: checkpointCode, count: readings.length, months })}
      >
        {limitLine(last?.warning_min, "stroke-tone-warning-fg", "wmin")}
        {limitLine(last?.warning_max, "stroke-tone-warning-fg", "wmax")}
        {limitLine(last?.normal_min, "stroke-tone-success-fg", "nmin")}
        {limitLine(last?.normal_max, "stroke-tone-success-fg", "nmax")}
        <polyline
          fill="none"
          className="stroke-primary"
          strokeWidth="2"
          points={readings
            .map((r, i) => `${x(times[i] ?? 0).toFixed(1)},${y(r.numeric_value).toFixed(1)}`)
            .join(" ")}
        />
        {readings.map((r, i) => (
          <circle
            key={`${r.round_id}-${r.recorded_at}`}
            cx={x(times[i] ?? 0)}
            cy={y(r.numeric_value)}
            r={3}
            className={
              r.result_status === "pass"
                ? "fill-primary"
                : r.result_status === "warning"
                  ? "fill-tone-warning-fg"
                  : "fill-tone-danger-fg"
            }
          >
            <title>
              {formatDate(r.recorded_at)} — {formatNumber(r.numeric_value)} {unit}
            </title>
          </circle>
        ))}
        <text x={4} y={y(maxY) + 4} className="fill-text-muted text-[10px]">
          {formatNumber(maxY)}
        </text>
        <text x={4} y={y(minY) + 4} className="fill-text-muted text-[10px]">
          {formatNumber(minY)}
        </text>
        <text x={PAD.left} y={H - 4} className="fill-text-muted text-[10px]">
          {formatDate(readings[0]?.recorded_at ?? "")}
        </text>
        <text x={W - PAD.right} y={H - 4} textAnchor="end" className="fill-text-muted text-[10px]">
          {formatDate(last?.recorded_at ?? "")}
        </text>
      </svg>
      <figcaption className="text-xs text-text-muted">{t("trend.legend", { unit })}</figcaption>
    </figure>
  );
}

/** The curve of a point, hidden until asked for (the field screen stays light). */
export function PointTrendToggle({
  templateId,
  checkpointCode,
  months = 12,
}: PointTrendChartProps) {
  const { t } = useTranslation("inspections");
  const [open, setOpen] = useState(false);
  return (
    <div className="space-y-2">
      <button
        type="button"
        className="text-sm text-primary underline underline-offset-2"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {open ? t("trend.hide") : t("trend.show")}
      </button>
      {open ? (
        <PointTrendChart templateId={templateId} checkpointCode={checkpointCode} months={months} />
      ) : null}
    </div>
  );
}
