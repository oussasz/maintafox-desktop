/**
 * "Indicateurs SMRP" — maintenance management indicators computed in Rust (`analytics::smrp`).
 * Every tile states its formula, period and unit (info button), shows the figures it was computed
 * from, and opens the module behind it. Missing inputs are said plainly, never estimated.
 */
import { ChevronRight, Info } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { type Tone, mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { listLinks } from "@/lib/list-links";
import { cn } from "@/lib/utils";
import { useWoOpenRuleText } from "@/lib/wo-open-rule";
import type { SmrpKpis, SmrpMetric } from "@/services/analytics-dashboard-service";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

/** Change below this many percentage points reads "stable". */
const STABLE_POINTS = 0.5;

export type SmrpDirection = "up-good" | "up-bad" | "neutral";

export interface PointsTrend {
  points: number;
  tone: Tone | null;
}

/** Change between two percentages, in points; `null` without both values. */
export function pointsTrend(
  current: number | null | undefined,
  previous: number | null | undefined,
  direction: SmrpDirection,
): PointsTrend | null {
  if (current == null || previous == null) return null;
  const points = current - previous;
  if (Math.abs(points) < STABLE_POINTS) return { points: 0, tone: null };
  if (direction === "neutral") return { points, tone: null };
  const good = direction === "up-good" ? points > 0 : points < 0;
  return { points, tone: good ? "success" : "danger" };
}

interface TileProps {
  id: string;
  title: string;
  value: string;
  available: boolean;
  /** Plain-language reason when not available. */
  unavailableText?: string;
  detail?: ReactNode;
  trend?: { label: string; tone: Tone | null } | null;
  formula: string;
  unit: string;
  period: string;
  onOpen?: () => void;
  openLabel?: string;
}

function SmrpTile({
  id,
  title,
  value,
  available,
  unavailableText,
  detail,
  trend,
  formula,
  unit,
  period,
  onOpen,
  openLabel,
}: TileProps) {
  const { t } = useTranslation("reports");
  return (
    <div
      className="flex flex-col rounded-lg border border-surface-border bg-surface-1 p-3 shadow-sm"
      data-testid={`smrp-${id}`}
    >
      <div className="flex items-start justify-between gap-2">
        <h4 className="text-xs font-medium uppercase tracking-wide text-text-muted">{title}</h4>
        <Popover>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="-mr-1 -mt-1 h-9 w-9 shrink-0 text-text-muted"
              aria-label={t("smrp.definitionAria", { title })}
            >
              <Info className="h-4 w-4" aria-hidden />
            </Button>
          </PopoverTrigger>
          <PopoverContent align="end" className="w-80 text-sm">
            <p className="font-medium text-text-primary">{title}</p>
            <dl className="mt-2 space-y-1.5 text-xs">
              <div>
                <dt className="font-medium text-text-secondary">{t("smrp.formula")}</dt>
                <dd className="text-text-muted">{formula}</dd>
              </div>
              <div>
                <dt className="font-medium text-text-secondary">{t("smrp.period")}</dt>
                <dd className="text-text-muted">{period}</dd>
              </div>
              <div>
                <dt className="font-medium text-text-secondary">{t("smrp.unit")}</dt>
                <dd className="text-text-muted">{unit}</dd>
              </div>
            </dl>
          </PopoverContent>
        </Popover>
      </div>
      {available ? (
        <div className="mt-1 flex flex-wrap items-baseline gap-2">
          <span className="font-mono text-2xl font-semibold tabular-nums text-text-primary">
            {value}
          </span>
          {trend ? (
            <span
              className={cn(
                "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                trend.tone ? mfTone[trend.tone].chip : mfTone.neutral.chip,
              )}
            >
              {trend.label}
            </span>
          ) : null}
        </div>
      ) : (
        <p className="mt-1 text-sm text-text-muted">{unavailableText ?? "—"}</p>
      )}
      {detail ? <div className="mt-1 text-xs text-text-muted">{detail}</div> : null}
      {onOpen ? (
        <Button
          type="button"
          variant="link"
          className="mt-auto h-9 justify-start px-0 text-xs"
          onClick={onOpen}
        >
          {openLabel ?? t("smrp.open")}
          <ChevronRight className="ml-0.5 h-3.5 w-3.5" aria-hidden />
        </Button>
      ) : null}
    </div>
  );
}

export function SmrpIndicatorsSection({
  smrp,
  can = () => true,
}: {
  smrp: SmrpKpis | null;
  /** The user's rights: a tile opens its module only when the user may enter it. */
  can?: (permission: PermissionName) => boolean;
}) {
  const { t } = useTranslation("reports");
  const navigate = useNavigate();
  const woOpenRuleText = useWoOpenRuleText();
  const { formatPercent, formatDecimal, formatNumber, formatDate, formatCurrency } =
    useFormatters();

  if (!smrp) return null;

  const period = t("smrp.periodValue", {
    days: Math.round(smrp.days),
    from: formatDate(smrp.from),
    to: formatDate(smrp.to),
  });
  const pct = (v: number | null | undefined) => (v == null ? "—" : formatPercent(v));
  const hours = (v: number) => t("smrp.hoursValue", { value: formatDecimal(v, 1) });
  const trendOf = (m: SmrpMetric, direction: SmrpDirection) => {
    const tr = pointsTrend(m.current.value_pct, m.previous.value_pct, direction);
    if (!tr) return null;
    if (tr.points === 0) return { label: t("smrp.trendStable"), tone: null };
    return {
      label: t("smrp.trendPoints", {
        change: `${tr.points > 0 ? "+" : ""}${formatDecimal(tr.points, 1)}`,
      }),
      tone: tr.tone,
    };
  };

  const tiles: ReactNode[] = [];

  if (smrp.schedule_compliance) {
    const m = smrp.schedule_compliance;
    tiles.push(
      <SmrpTile
        key="schedule"
        id="schedule"
        title={t("smrp.schedule.title")}
        value={pct(m.current.value_pct)}
        available={m.current.value_pct != null}
        unavailableText={t("smrp.schedule.none")}
        detail={t("smrp.schedule.detail", {
          done: hours(m.current.numerator),
          total: hours(m.current.denominator),
        })}
        trend={trendOf(m, "up-good")}
        formula={t("smrp.schedule.formula")}
        unit={t("smrp.unitPercent")}
        period={period}
        {...(can(P.PLAN_VIEW) ? { onOpen: () => navigate("/planning") } : {})}
      />,
    );
  }

  if (smrp.work_mix) {
    const mix = smrp.work_mix;
    const basis = mix.basis === "hours" ? t("smrp.mix.basisHours") : t("smrp.mix.basisCount");
    const detail = (m: SmrpMetric) =>
      mix.basis === "hours"
        ? t("smrp.mix.detailHours", {
            part: hours(m.current.numerator),
            total: hours(m.current.denominator),
          })
        : t("smrp.mix.detailCount", {
            part: formatNumber(m.current.numerator),
            total: formatNumber(m.current.denominator),
          });
    for (const [key, m, direction] of [
      ["planned", mix.planned, "up-good"],
      ["reactive", mix.reactive, "up-bad"],
      ["emergency", mix.emergency, "up-bad"],
    ] as const) {
      tiles.push(
        <SmrpTile
          key={key}
          id={key}
          title={t(`smrp.mix.${key}.title`)}
          value={pct(m.current.value_pct)}
          available={m.current.value_pct != null}
          unavailableText={t("smrp.mix.none")}
          detail={
            <>
              {detail(m)}
              <br />
              {basis}
            </>
          }
          trend={trendOf(m, direction)}
          formula={t(`smrp.mix.${key}.formula`)}
          unit={t("smrp.unitPercent")}
          period={period}
          {...(can(P.OT_VIEW)
            ? { onOpen: () => navigate(listLinks.workOrders({ mix: key })) }
            : {})}
        />,
      );
    }
  }

  if (smrp.pm_compliance) {
    const m = smrp.pm_compliance;
    tiles.push(
      <SmrpTile
        key="pm"
        id="pm"
        title={t("smrp.pm.title")}
        value={pct(m.current.value_pct)}
        available={m.current.value_pct != null}
        unavailableText={t("smrp.pm.none")}
        detail={t("smrp.pm.detail", {
          done: formatNumber(m.current.numerator),
          total: formatNumber(m.current.denominator),
        })}
        trend={trendOf(m, "up-good")}
        formula={t("smrp.pm.formula")}
        unit={t("smrp.unitPercent")}
        period={period}
        {...(can(P.PM_VIEW) ? { onOpen: () => navigate("/pm") } : {})}
      />,
    );
  }

  if (smrp.backlog) {
    const b = smrp.backlog;
    const weeks = (v: number | null) =>
      v == null ? "—" : t("smrp.backlog.weeksValue", { value: formatDecimal(v, 1) });
    const change = b.weeks != null && b.previous_weeks != null ? b.weeks - b.previous_weeks : null;
    // The backlog counts one named "open OT" rule: the tile says which and opens the same list.
    const backlogRule = woOpenRuleText(b.open_work_orders_rule);
    tiles.push(
      <SmrpTile
        key="backlog"
        id="backlog"
        title={t("smrp.backlog.title")}
        value={weeks(b.weeks)}
        available={b.weeks != null}
        unavailableText={t("smrp.backlog.noCapacity", { hours: hours(b.backlog_hours) })}
        detail={
          <>
            {t("smrp.backlog.detail", {
              hours: hours(b.backlog_hours),
              count: b.open_work_orders,
              ready: weeks(b.ready_weeks),
            })}
            {backlogRule ? (
              <>
                <br />
                <span title={backlogRule.hint}>{backlogRule.figure}</span>
              </>
            ) : null}
            <br />
            {t("smrp.backlog.capacity", {
              hours: hours(b.weekly_capacity_hours),
              people: b.people_counted,
            })}
            {b.people_without_schedule > 0 ? (
              <>
                <br />
                {t("smrp.backlog.withoutSchedule", { count: b.people_without_schedule })}
              </>
            ) : null}
            {b.work_orders_without_estimate > 0 ? (
              <>
                <br />
                {t("smrp.backlog.withoutEstimate", { count: b.work_orders_without_estimate })}
              </>
            ) : null}
          </>
        }
        trend={
          change == null
            ? null
            : Math.abs(change) < 0.05
              ? { label: t("smrp.trendStable"), tone: null }
              : {
                  label: t("smrp.backlog.trend", {
                    change: `${change > 0 ? "+" : ""}${formatDecimal(change, 1)}`,
                  }),
                  tone: null,
                }
        }
        formula={t("smrp.backlog.formula")}
        unit={t("smrp.backlog.unit")}
        period={t("smrp.backlog.period")}
        {...(backlogRule && can(P.OT_VIEW)
          ? { onOpen: () => navigate(listLinks.workOrders({ scope: backlogRule.scope })) }
          : {})}
      />,
    );
  }

  if (smrp.cost_to_rav) {
    const c = smrp.cost_to_rav;
    const tr = pointsTrend(c.value_pct, c.previous_value_pct, "up-bad");
    tiles.push(
      <SmrpTile
        key="rav"
        id="rav"
        title={t("smrp.rav.title")}
        value={c.value_pct == null ? "—" : formatPercent(c.value_pct)}
        available={c.value_pct != null}
        unavailableText={t("smrp.rav.needsValue", {
          valued: c.assets_with_value,
          total: c.assets_total,
        })}
        detail={
          c.value_pct == null
            ? undefined
            : t("smrp.rav.detail", {
                cost: formatCurrency(c.annualized_cost),
                rav: formatCurrency(c.replacement_value_total),
                valued: c.assets_with_value,
                total: c.assets_total,
              })
        }
        trend={
          tr == null
            ? null
            : tr.points === 0
              ? { label: t("smrp.trendStable"), tone: null }
              : {
                  label: t("smrp.trendPoints", {
                    change: `${tr.points > 0 ? "+" : ""}${formatDecimal(tr.points, 2)}`,
                  }),
                  tone: tr.tone,
                }
        }
        formula={t("smrp.rav.formula")}
        unit={t("smrp.unitPercentYear")}
        period={period}
        {...(can(P.EQ_VIEW) ? { onOpen: () => navigate("/equipment") } : {})}
        openLabel={c.value_pct == null ? t("smrp.rav.openAssets") : t("smrp.open")}
      />,
    );
  }

  if (tiles.length === 0) return null;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-base font-medium">{t("smrp.title")}</CardTitle>
        <p className="text-xs text-text-muted">{t("smrp.subtitle", { period })}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">{tiles}</div>
        <p className="text-xs text-text-muted">{t("smrp.wrenchTime")}</p>
      </CardContent>
    </Card>
  );
}
