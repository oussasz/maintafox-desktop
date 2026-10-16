/**
 * "My work" — the signed-in person's own work orders, requests, inspection rounds, PM and
 * notifications to acknowledge (`get_home_my_work`). A module the person may not open has no
 * section (the backend sends none).
 */
import { UserRound } from "lucide-react";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { useVisibleDiQueues } from "@/components/di/DiQueueBar";
import { useFormatters } from "@/hooks/use-formatters";
import { getHomeMyWork } from "@/services/home-service";
import { statusToI18nKey } from "@/utils/wo-status";
import type { HomeMyWork } from "@shared/ipc-types";

import {
  HomeFigure,
  HomeNote,
  HomePanel,
  HomeRecordList,
  HomeSection,
  usePanelLoad,
} from "./HomeParts";
import { homeLinks } from "./home-links";

export function HomeMyWorkPanel({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const load = useCallback(() => getHomeMyWork(), []);
  const state = usePanelLoad<HomeMyWork>(load, refreshSignal);
  return (
    <HomePanel
      icon={UserRound}
      title={t("home.myWork.title")}
      subtitle={t("home.myWork.subtitle")}
      state={state}
      testId="home-my-work"
    >
      {state.data ? <MyWorkBody data={state.data} /> : null}
    </HomePanel>
  );
}

function MyWorkBody({ data }: { data: HomeMyWork }) {
  const { t } = useTranslation("dashboard");
  const { t: tOt } = useTranslation("ot");
  const { t: tIns } = useTranslation("inspections");
  const { formatNumber, formatDateTime, formatDate } = useFormatters();
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const queues = useVisibleDiQueues();
  const n = (v: number) => formatNumber(v);
  const wo = data.work_orders;
  const di = data.requests;
  const rounds = data.rounds;
  const pm = data.pm;

  return (
    <div className="grid gap-6 xl:grid-cols-2">
      {wo ? (
        <HomeSection
          title={t("home.myWork.wo.title")}
          definitions={[
            { label: t("home.myWork.wo.open"), text: t("home.def.myWoOpen") },
            { label: t("home.myWork.wo.overdue"), text: t("home.def.woOverdue") },
            { label: t("home.myWork.wo.today"), text: t("home.def.myWoToday") },
          ]}
          openLabel={t("home.open.workOrders")}
          openTo={homeLinks.workOrders}
        >
          <div className="grid grid-cols-3 gap-2">
            <HomeFigure label={t("home.myWork.wo.open")} value={n(wo.open)} />
            <HomeFigure
              label={t("home.myWork.wo.overdue")}
              value={n(wo.overdue)}
              tone={wo.overdue > 0 ? "danger" : undefined}
            />
            <HomeFigure label={t("home.myWork.wo.today")} value={n(wo.today)} />
          </div>
          {wo.by_urgency.length > 0 ? (
            <ul className="flex flex-wrap gap-1.5" aria-label={t("home.myWork.wo.byUrgency")}>
              {wo.by_urgency.map((u) => (
                <li
                  key={u.urgency_code ?? "none"}
                  className="rounded-full border border-surface-border px-2 py-0.5 text-xs text-text-secondary"
                >
                  {u.urgency_code ? urgencyLabel(u.urgency_code) : t("home.noUrgency")} ·{" "}
                  {n(u.count)}
                </li>
              ))}
            </ul>
          ) : null}
          <HomeRecordList
            ariaLabel={t("home.myWork.wo.listAria")}
            emptyText={t("home.myWork.wo.empty")}
            total={wo.open}
            rows={wo.items.map((i) => ({
              key: i.id,
              primary: `${i.code} — ${i.title}`,
              secondary: [tOt(`status.${statusToI18nKey(i.status_code)}` as never), i.asset_label]
                .filter(Boolean)
                .join(" · "),
              meta: i.planned_end ? formatDateTime(i.planned_end) : undefined,
              badge: i.overdue
                ? t("home.overdueBadge")
                : i.urgency_code
                  ? urgencyLabel(i.urgency_code)
                  : undefined,
              tone: i.overdue ? "danger" : urgencyTone(i.urgency_code),
              to: homeLinks.workOrder(i.id),
            }))}
          />
        </HomeSection>
      ) : null}

      <div className="space-y-6">
        {di ? (
          <HomeSection
            title={t("home.myWork.di.title")}
            definitions={[
              { label: t("home.myWork.di.open"), text: t("home.def.myDiOpen") },
              { label: t("home.myWork.di.awaiting"), text: t("home.def.myDiAwaiting") },
            ]}
          >
            <div className="grid grid-cols-2 gap-2">
              <HomeFigure
                label={t("home.myWork.di.open")}
                value={n(di.open)}
                to={queues.includes("mine") ? homeLinks.requestQueue("mine") : undefined}
              />
              <HomeFigure
                label={t("home.myWork.di.awaiting")}
                value={n(di.awaiting_my_answer)}
                tone={di.awaiting_my_answer > 0 ? "attention" : undefined}
                to={queues.includes("mine") ? homeLinks.requestQueue("mine") : undefined}
              />
            </div>
          </HomeSection>
        ) : null}

        {rounds ? (
          <HomeSection
            title={t("home.myWork.rounds.title")}
            definitions={[
              { label: t("home.myWork.rounds.overdue"), text: t("home.def.roundOverdue") },
              { label: t("home.myWork.rounds.today"), text: t("home.def.roundToday") },
              { label: t("home.myWork.rounds.next7"), text: t("home.def.roundNext7") },
            ]}
            openLabel={t("home.open.inspections")}
            openTo={homeLinks.inspections}
          >
            {rounds.personnel_linked ? (
              <>
                <div className="grid grid-cols-3 gap-2">
                  <HomeFigure
                    label={t("home.myWork.rounds.overdue")}
                    value={n(rounds.overdue)}
                    tone={rounds.overdue > 0 ? "danger" : undefined}
                  />
                  <HomeFigure label={t("home.myWork.rounds.today")} value={n(rounds.today)} />
                  <HomeFigure label={t("home.myWork.rounds.next7")} value={n(rounds.next_7_days)} />
                </div>
                <HomeRecordList
                  ariaLabel={t("home.myWork.rounds.listAria")}
                  emptyText={t("home.myWork.rounds.empty")}
                  total={rounds.todo}
                  rows={rounds.items.map((r) => ({
                    key: r.id,
                    primary: `${r.template_code} — ${r.template_name}`,
                    secondary: tIns(`status.${r.status}` as never),
                    meta: r.scheduled_at ? formatDateTime(r.scheduled_at) : undefined,
                    badge: r.overdue ? t("home.overdueBadge") : undefined,
                    tone: r.overdue ? "danger" : undefined,
                    to: homeLinks.round(r.id),
                  }))}
                />
              </>
            ) : (
              <HomeNote>{t("home.myWork.rounds.notLinked")}</HomeNote>
            )}
          </HomeSection>
        ) : null}

        {pm ? (
          <HomeSection
            title={t("home.myWork.pm.title")}
            definitions={[
              { label: t("home.myWork.pm.overdue"), text: t("home.def.pmOverdue") },
              { label: t("home.myWork.pm.next7"), text: t("home.def.pmNext7") },
            ]}
            openLabel={t("home.open.pm")}
            openTo={homeLinks.pm}
          >
            <div className="grid grid-cols-2 gap-2">
              <HomeFigure
                label={t("home.myWork.pm.overdue")}
                value={n(pm.overdue)}
                tone={pm.overdue > 0 ? "danger" : undefined}
              />
              <HomeFigure label={t("home.myWork.pm.next7")} value={n(pm.due_next_7_days)} />
            </div>
            <HomeRecordList
              ariaLabel={t("home.myWork.pm.listAria")}
              emptyText={t("home.myWork.pm.empty")}
              total={pm.pending}
              rows={pm.items.map((o) => ({
                key: o.id,
                primary: `${o.plan_code} — ${o.plan_title}`,
                secondary: o.work_order_code,
                meta: o.due_at ? formatDate(o.due_at) : undefined,
                badge: o.overdue ? t("home.overdueBadge") : undefined,
                tone: o.overdue ? "danger" : undefined,
                to: homeLinks.pmOccurrence(o.id),
              }))}
            />
          </HomeSection>
        ) : null}

        <HomeSection
          title={t("home.myWork.notifications.title")}
          definitions={[
            { label: t("home.myWork.notifications.toAck"), text: t("home.def.toAck") },
            { label: t("home.myWork.notifications.unread"), text: t("home.def.unread") },
          ]}
        >
          <div className="grid grid-cols-2 gap-2">
            <HomeFigure
              label={t("home.myWork.notifications.toAck")}
              value={n(data.notifications.to_acknowledge)}
              tone={data.notifications.to_acknowledge > 0 ? "attention" : undefined}
              to={homeLinks.notifications}
            />
            <HomeFigure
              label={t("home.myWork.notifications.unread")}
              value={n(data.notifications.unread)}
              to={homeLinks.notifications}
            />
          </div>
        </HomeSection>
      </div>
    </div>
  );
}
