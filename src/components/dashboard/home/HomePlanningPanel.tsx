/**
 * Planner panel (`get_home_planning`): the schedule backlog by readiness, this week's committed
 * load against the crews' capacity, and SMRP schedule compliance — the planning module's figures.
 */
import { CalendarRange } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";
import { type HomePeriod, getHomePlanning } from "@/services/home-service";
import type { HomePlanning } from "@shared/ipc-types";

import { HomeFigure, HomeNote, HomePanel, HomeSection, usePanelLoad } from "./HomeParts";
import { homeLinks } from "./home-links";

const BLOCKER_DIMENSIONS = ["parts", "skills", "permits", "windows", "prerequisites"] as const;

export function HomePlanningPanel({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const [period, setPeriod] = useState<HomePeriod>(7);
  const load = useCallback(() => getHomePlanning(period), [period]);
  const state = usePanelLoad<HomePlanning>(load, refreshSignal);
  return (
    <HomePanel
      icon={CalendarRange}
      title={t("home.planning.title")}
      subtitle={t("home.planning.subtitle")}
      period={period}
      onPeriodChange={setPeriod}
      state={state}
      testId="home-planning"
    >
      {state.data ? <PlanningBody data={state.data} /> : null}
    </HomePanel>
  );
}

function PlanningBody({ data }: { data: HomePlanning }) {
  const { t } = useTranslation("dashboard");
  const { formatNumber, formatDecimal, formatPercent, formatDate } = useFormatters();
  const { backlog, week, schedule_compliance: sc } = data;
  const hours = (v: number) => t("home.hours", { value: formatDecimal(v, 1) });

  return (
    <div className="grid gap-6 xl:grid-cols-3">
      <HomeSection
        title={t("home.planning.backlog.title")}
        definitions={[
          { label: t("home.planning.backlog.ready"), text: t("home.def.backlogReady") },
          { label: t("home.planning.backlog.blocked"), text: t("home.def.backlogBlocked") },
        ]}
        openLabel={t("home.open.planning")}
        openTo={homeLinks.planning}
      >
        <div className="grid grid-cols-2 gap-2">
          <HomeFigure
            label={t("home.planning.backlog.ready")}
            value={formatNumber(backlog.ready)}
            to={homeLinks.planning}
          />
          <HomeFigure
            label={t("home.planning.backlog.blocked")}
            value={formatNumber(backlog.blocked)}
            tone={backlog.blocked > 0 ? "warning" : undefined}
            to={homeLinks.planning}
          />
        </div>
        {backlog.blocked_by.length > 0 ? (
          <ul className="flex flex-wrap gap-1.5" aria-label={t("home.planning.backlog.blockedBy")}>
            {BLOCKER_DIMENSIONS.map((dim) => {
              const c = backlog.blocked_by.find((b) => b.dimension === dim);
              if (!c) return null;
              return (
                <li
                  key={dim}
                  className="rounded-full border border-surface-border px-2 py-0.5 text-xs text-text-secondary"
                >
                  {t(`home.planning.blocker.${dim}`)} · {formatNumber(c.count)}
                </li>
              );
            })}
          </ul>
        ) : null}
        {backlog.truncated ? (
          <HomeNote tone="info">
            {t("home.planning.backlog.truncated", { count: backlog.candidates })}
          </HomeNote>
        ) : null}
      </HomeSection>

      <HomeSection
        title={t("home.planning.week.title", {
          from: formatDate(week.week_start),
          to: formatDate(week.week_end),
        })}
        definitions={[
          { label: t("home.planning.week.load"), text: t("home.def.weekLoad") },
          { label: t("home.planning.week.overloaded"), text: t("home.def.weekOverloaded") },
        ]}
        openLabel={t("home.open.planning")}
        openTo={homeLinks.planning}
      >
        {week.teams === 0 ? (
          <HomeNote>{t("home.planning.week.noTeams")}</HomeNote>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-2">
              <HomeFigure
                label={t("home.planning.week.load")}
                value={
                  week.load_pct == null
                    ? t("home.planning.week.noCapacity")
                    : formatPercent(week.load_pct)
                }
                hint={t("home.planning.week.hoursOf", {
                  committed: hours(week.committed_hours),
                  capacity: hours(week.capacity_hours),
                })}
                tone={week.load_pct != null && week.load_pct > 100 ? "danger" : undefined}
              />
              <HomeFigure
                label={t("home.planning.week.overloaded")}
                value={formatNumber(week.overloaded_team_days)}
                tone={week.overloaded_team_days > 0 ? "warning" : undefined}
              />
            </div>
          </>
        )}
      </HomeSection>

      <HomeSection
        title={t("home.planning.compliance.title")}
        definitions={[
          { label: t("home.planning.compliance.title"), text: t("home.def.scheduleCompliance") },
        ]}
      >
        {sc.value_pct == null ? (
          <HomeNote>{t("home.planning.compliance.none", { count: sc.period_days })}</HomeNote>
        ) : (
          <HomeFigure
            label={t("home.planning.compliance.value")}
            value={formatPercent(sc.value_pct)}
            hint={t("home.planning.compliance.hint", {
              done: hours(sc.done_hours),
              scheduled: hours(sc.scheduled_hours),
              count: sc.period_days,
            })}
          />
        )}
      </HomeSection>
    </div>
  );
}
