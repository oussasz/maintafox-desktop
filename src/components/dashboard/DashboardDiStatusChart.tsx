/**
 * Open requests by status (`get_dashboard_di_status_chart` → `di::stats::open_backlog_by_status`,
 * the DI page's open backlog: not closed, not turned into a work order). A status that has a work
 * queue opens it, when the user may see that queue.
 */
import { Inbox } from "lucide-react";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { diStatusLabel } from "@/components/analytics/AnalyticsDiStatusChart";
import { useVisibleDiQueues } from "@/components/di/DiQueueBar";
import { DI_WORKFLOW_STATUS_TONE, mfTone, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { getDashboardDiStatusChart } from "@/services/dashboard-service";
import type { DiQueue, DashboardDiStatusChart as Model } from "@shared/ipc-types";

import { HomeNote, HomePanel, HomeRecordList, HomeSection, usePanelLoad } from "./home/HomeParts";
import { homeLinks } from "./home/home-links";

/** The work queue that lists exactly the open requests of a status (`di::queries` queues). */
const QUEUE_OF_STATUS: Partial<Record<string, DiQueue>> = {
  submitted: "to_screen",
  in_review: "to_review",
  returned_for_clarification: "awaiting_info",
  awaiting_approval: "to_approve",
  approved: "approved_not_converted",
};

export function DashboardDiStatusChart({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const load = useCallback(() => getDashboardDiStatusChart(), []);
  const state = usePanelLoad<Model>(load, refreshSignal);
  return (
    <HomePanel
      icon={Inbox}
      title={t("widgets.diStatus.title")}
      state={state}
      testId="home-di-status"
    >
      {state.data ? <DiStatusBody model={state.data} /> : null}
    </HomePanel>
  );
}

function DiStatusBody({ model }: { model: Model }) {
  const { t } = useTranslation("dashboard");
  const { t: tr } = useTranslation("reports");
  const { formatNumber, formatDecimal } = useFormatters();
  const queues = useVisibleDiQueues();
  if (!model.available) return <HomeNote>{t("home.noPermission")}</HomeNote>;
  const max = Math.max(1, ...model.segments.map((s) => s.count));
  const linked = model.segments.filter((s) => {
    const q = QUEUE_OF_STATUS[s.status];
    return q != null && queues.includes(q);
  });

  return (
    <HomeSection
      title={t("widgets.diStatus.section")}
      definitions={[{ label: t("widgets.diStatus.section"), text: t("home.def.diOpenByStatus") }]}
      openLabel={t("home.open.requests")}
      openTo={homeLinks.requests}
    >
      {model.segments.length === 0 ? (
        <HomeNote>{t("widgets.diStatus.empty")}</HomeNote>
      ) : (
        <ul className="space-y-2" aria-label={t("widgets.diStatus.aria")}>
          {model.segments.map((s) => {
            const tone = toneOf(DI_WORKFLOW_STATUS_TONE, s.status);
            return (
              <li key={s.status} className="flex items-center gap-2 text-xs">
                <span className="w-44 truncate text-text-secondary">
                  {diStatusLabel(tr, s.status)}
                </span>
                <div className="h-2.5 flex-1 rounded bg-surface-2" aria-hidden>
                  <div
                    className={cn("h-2.5 rounded", mfTone[tone].bar)}
                    style={{ width: `${(s.count / max) * 100}%` }}
                  />
                </div>
                <span className="w-10 text-end font-mono tabular-nums text-text-primary">
                  {formatNumber(s.count)}
                </span>
                <span className="w-24 text-end text-text-muted">
                  {t("widgets.diStatus.avgAge", { days: formatDecimal(s.avg_age_days, 1) })}
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {linked.length > 0 ? (
        <HomeRecordList
          ariaLabel={t("widgets.diStatus.queuesAria")}
          emptyText=""
          rows={linked.map((s) => ({
            key: s.status,
            primary: t("widgets.diStatus.openQueue", { status: diStatusLabel(tr, s.status) }),
            meta: formatNumber(s.count),
            to: homeLinks.requestQueue(QUEUE_OF_STATUS[s.status] as DiQueue),
          }))}
        />
      ) : null}
    </HomeSection>
  );
}
