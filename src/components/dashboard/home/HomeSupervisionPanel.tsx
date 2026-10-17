/**
 * Supervisor / manager panel (`get_home_supervision`): open requests against their SLA, the OT
 * backlog by nature and urgency, PM, stock to reorder, permits and lockouts, critical assets
 * down. Each section is the module's own figure; a module the user may not open has none.
 */
import { ShieldCheck } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { useVisibleDiQueues } from "@/components/di/DiQueueBar";
import { useWoTypeLabels, woNatureLabel } from "@/components/wo/use-wo-type-labels";
import { useFormatters } from "@/hooks/use-formatters";
import { listLinks } from "@/lib/list-links";
import { type HomePeriod, getHomeSupervision } from "@/services/home-service";
import type { HomeSupervision, HomeSupervisionWorkOrders } from "@shared/ipc-types";

import {
  HomeFigure,
  HomeNote,
  HomePanel,
  HomeRecordList,
  HomeSection,
  usePanelLoad,
} from "./HomeParts";
import { homeLinks } from "./home-links";

export function HomeSupervisionPanel({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const [period, setPeriod] = useState<HomePeriod>(30);
  const load = useCallback(() => getHomeSupervision(period), [period]);
  const state = usePanelLoad<HomeSupervision>(load, refreshSignal);
  return (
    <HomePanel
      icon={ShieldCheck}
      title={t("home.supervision.title")}
      subtitle={t("home.supervision.subtitle")}
      period={period}
      onPeriodChange={setPeriod}
      state={state}
      testId="home-supervision"
    >
      {state.data ? <SupervisionBody data={state.data} /> : null}
    </HomePanel>
  );
}

function SupervisionBody({ data }: { data: HomeSupervision }) {
  const { t } = useTranslation("dashboard");
  const { formatNumber, formatPercent, formatDecimal, formatDateTime } = useFormatters();
  const queues = useVisibleDiQueues();
  const n = (v: number) => formatNumber(v);
  const { requests: di, work_orders: wo, pm, stock, safety, assets } = data;
  const slaQueue = queues.includes("sla_at_risk")
    ? homeLinks.requestQueue("sla_at_risk")
    : undefined;
  const queueLink = (q: "to_screen" | "to_review" | "to_approve") =>
    queues.includes(q) ? homeLinks.requestQueue(q) : undefined;
  const closingPct =
    di && di.sla_closing.total > 0 ? (di.sla_closing.met / di.sla_closing.total) * 100 : null;

  const hasAny = di || wo || pm || stock || safety || assets;
  if (!hasAny) return <HomeNote>{t("home.supervision.noModule")}</HomeNote>;

  return (
    <div className="grid gap-6 xl:grid-cols-2">
      {di ? (
        <HomeSection
          title={t("home.supervision.di.title")}
          definitions={[
            { label: t("home.supervision.di.breached"), text: t("home.def.slaBreached") },
            { label: t("home.supervision.di.atRisk"), text: t("home.def.slaAtRisk") },
            { label: t("home.supervision.di.paused"), text: t("home.def.slaPaused") },
            { label: t("home.supervision.di.closing"), text: t("home.def.slaClosing") },
          ]}
          openLabel={t("home.open.requests")}
          openTo={homeLinks.requests}
        >
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <HomeFigure
              label={t("home.supervision.di.breached")}
              value={n(di.sla_open.breached)}
              tone={di.sla_open.breached > 0 ? "danger" : undefined}
              to={slaQueue}
            />
            <HomeFigure
              label={t("home.supervision.di.atRisk")}
              value={n(di.sla_open.at_risk)}
              tone={di.sla_open.at_risk > 0 ? "warning" : undefined}
              to={slaQueue}
            />
            <HomeFigure label={t("home.supervision.di.onTrack")} value={n(di.sla_open.on_track)} />
            <HomeFigure label={t("home.supervision.di.paused")} value={n(di.sla_open.paused)} />
          </div>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <HomeFigure
              label={t("home.supervision.di.closing")}
              value={closingPct == null ? t("home.none") : formatPercent(closingPct)}
              hint={t("home.supervision.di.closingHint", {
                met: n(di.sla_closing.met),
                total: n(di.sla_closing.total),
                count: di.period_days,
              })}
            />
            <HomeFigure
              label={t("home.supervision.di.toScreen")}
              value={n(di.queues.to_screen)}
              to={queueLink("to_screen")}
            />
            <HomeFigure
              label={t("home.supervision.di.toReview")}
              value={n(di.queues.to_review)}
              to={queueLink("to_review")}
            />
            <HomeFigure
              label={t("home.supervision.di.toApprove")}
              value={n(di.queues.to_approve)}
              tone={di.queues.to_approve > 0 ? "attention" : undefined}
              to={queueLink("to_approve")}
            />
          </div>
          {di.sla_open.no_target > 0 ? (
            <HomeNote>
              {t("home.supervision.di.noTarget", { count: di.sla_open.no_target })}
            </HomeNote>
          ) : null}
        </HomeSection>
      ) : null}

      {wo ? <WoBacklogSection wo={wo} /> : null}

      {pm ? (
        <HomeSection
          title={t("home.supervision.pm.title")}
          definitions={[
            { label: t("home.supervision.pm.overdue"), text: t("home.def.pmOverdue") },
            { label: t("home.supervision.pm.today"), text: t("home.def.pmToday") },
            { label: t("home.supervision.pm.next7"), text: t("home.def.pmNext7") },
            { label: t("home.supervision.pm.compliance"), text: t("home.def.pmCompliance") },
          ]}
          openLabel={t("home.open.pm")}
          openTo={homeLinks.pm}
        >
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <HomeFigure
              label={t("home.supervision.pm.overdue")}
              value={n(pm.overdue)}
              tone={pm.overdue > 0 ? "danger" : undefined}
              to={listLinks.pmDue("overdue")}
            />
            <HomeFigure
              label={t("home.supervision.pm.today")}
              value={n(pm.due_today)}
              to={listLinks.pmDue("today")}
            />
            <HomeFigure
              label={t("home.supervision.pm.next7")}
              value={n(pm.due_next_7_days)}
              to={listLinks.pmDue("next7")}
            />
            <HomeFigure
              label={t("home.supervision.pm.compliance")}
              value={pm.compliance_pct == null ? t("home.none") : formatPercent(pm.compliance_pct)}
              hint={t("home.supervision.pm.complianceHint", {
                done: formatDecimal(pm.compliance_done_on_time, 0),
                due: formatDecimal(pm.compliance_due, 0),
                count: pm.period_days,
              })}
            />
          </div>
        </HomeSection>
      ) : null}

      {stock ? (
        <HomeSection
          title={t("home.supervision.stock.title")}
          definitions={[
            { label: t("home.supervision.stock.toReorder"), text: t("home.def.stockReorder") },
            { label: t("home.supervision.stock.out"), text: t("home.def.stockOut") },
          ]}
          openLabel={t("home.open.inventory")}
          openTo={homeLinks.inventory}
        >
          <div className="grid grid-cols-2 gap-2">
            <HomeFigure
              label={t("home.supervision.stock.toReorder")}
              value={n(stock.articles_to_reorder)}
              hint={t("home.supervision.stock.lines", { count: stock.lines_to_reorder })}
              tone={stock.articles_to_reorder > 0 ? "warning" : undefined}
              to={listLinks.inventory({ belowReorder: true })}
            />
            <HomeFigure
              label={t("home.supervision.stock.out")}
              value={n(stock.lines_out_of_stock)}
              tone={stock.lines_out_of_stock > 0 ? "danger" : undefined}
            />
          </div>
          <HomeRecordList
            ariaLabel={t("home.supervision.stock.listAria")}
            emptyText={t("home.supervision.stock.empty")}
            total={stock.lines_to_reorder}
            rows={stock.items.map((s) => ({
              key: `${s.article_id}-${s.warehouse_code}`,
              primary: `${s.article_code} — ${s.article_name}`,
              secondary: t("home.supervision.stock.line", {
                warehouse: s.warehouse_code,
                available: formatDecimal(s.available_qty, 2),
                threshold: formatDecimal(
                  s.trigger_type === "min_stock" ? s.min_stock : s.reorder_point,
                  2,
                ),
              }),
              badge: s.available_qty <= 0 ? t("home.supervision.stock.outBadge") : undefined,
              tone: s.available_qty <= 0 ? "danger" : undefined,
              to: homeLinks.article(s.article_id),
            }))}
          />
        </HomeSection>
      ) : null}

      {safety ? (
        <HomeSection
          title={t("home.supervision.safety.title")}
          definitions={[
            { label: t("home.supervision.safety.open"), text: t("home.def.permitsOpen") },
            { label: t("home.supervision.safety.expiring"), text: t("home.def.permitsExpiring") },
            { label: t("home.supervision.safety.expired"), text: t("home.def.permitsExpired") },
            { label: t("home.supervision.safety.locks"), text: t("home.def.locks") },
          ]}
          openLabel={t("home.open.permits")}
          openTo={homeLinks.permits}
        >
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <HomeFigure label={t("home.supervision.safety.open")} value={n(safety.open_permits)} />
            <HomeFigure
              label={t("home.supervision.safety.expiring")}
              value={safety.expiring_rule === "active" ? n(safety.expiring.total) : t("home.none")}
              hint={
                safety.expiring_rule === "active" && safety.expiring_lead_minutes != null
                  ? t("home.supervision.safety.within", {
                      hours: formatDecimal(safety.expiring_lead_minutes / 60, 1),
                    })
                  : t(`home.supervision.safety.rule.${safety.expiring_rule}`)
              }
              tone={safety.expiring.total > 0 ? "warning" : undefined}
            />
            <HomeFigure
              label={t("home.supervision.safety.expired")}
              value={n(safety.expired_open.total)}
              tone={safety.expired_open.total > 0 ? "danger" : undefined}
            />
            <HomeFigure
              label={t("home.supervision.safety.locks")}
              value={n(safety.locks_in_place)}
              hint={t("home.supervision.safety.locksOn", { count: safety.permits_with_locks })}
            />
          </div>
          {[
            { list: safety.expired_open, expired: true },
            { list: safety.expiring, expired: false },
          ].map(({ list, expired }) =>
            list.total > 0 ? (
              <div key={expired ? "expired" : "expiring"} className="space-y-1.5">
                <HomeRecordList
                  ariaLabel={
                    expired
                      ? t("home.supervision.safety.expiredListAria")
                      : t("home.supervision.safety.expiringListAria")
                  }
                  emptyText=""
                  rows={list.items.map((p) => ({
                    key: p.permit_id,
                    primary: `${p.code} — ${p.permit_type}`,
                    secondary: p.asset_code || undefined,
                    meta: formatDateTime(p.expires_at),
                    badge: expired
                      ? t("home.supervision.safety.expiredBadge")
                      : t("home.supervision.safety.expiringBadge"),
                    tone: expired ? "danger" : "warning",
                    to: homeLinks.permit(p.permit_id),
                  }))}
                />
                {list.total - list.without_record > list.items.length ? (
                  <HomeNote>
                    {t("home.more", {
                      count: list.total - list.without_record - list.items.length,
                    })}
                  </HomeNote>
                ) : null}
                {list.without_record > 0 ? (
                  <HomeNote tone="warning">
                    {t("home.supervision.safety.withoutRecord", { count: list.without_record })}
                  </HomeNote>
                ) : null}
              </div>
            ) : null,
          )}
        </HomeSection>
      ) : null}

      {assets ? (
        <HomeSection
          title={t("home.supervision.assets.title")}
          definitions={[
            {
              label: t("home.supervision.assets.down"),
              text: t("home.def.criticalDown", {
                level: assets.criticality_label ?? t("home.supervision.assets.noLevel"),
                statuses:
                  assets.down_status_labels.length > 0
                    ? assets.down_status_labels.join(", ")
                    : t("home.supervision.assets.noDownStatus"),
              }),
            },
          ]}
          openLabel={t("home.open.equipment")}
          openTo={homeLinks.equipment}
        >
          <HomeFigure
            label={t("home.supervision.assets.down")}
            value={n(assets.critical_down)}
            tone={assets.critical_down > 0 ? "danger" : undefined}
          />
          <HomeRecordList
            ariaLabel={t("home.supervision.assets.listAria")}
            emptyText={t("home.supervision.assets.empty")}
            total={assets.critical_down}
            rows={assets.items.map((a) => ({
              key: a.id,
              primary: `${a.asset_code} — ${a.name}`,
              badge: a.status_label ?? undefined,
              tone: "danger",
              to: homeLinks.asset(a.id),
            }))}
          />
        </HomeSection>
      ) : null}
    </div>
  );
}

function WoBacklogSection({ wo }: { wo: HomeSupervisionWorkOrders }) {
  const { t } = useTranslation("dashboard");
  const { formatNumber } = useFormatters();
  const { levels, labelOf: urgencyLabel } = useUrgencyScale();
  const { types, language } = useWoTypeLabels();

  // Nature labels come from the SYSTEM list WORK.MAINTENANCE_NATURES (through the OT types).
  const natureLabel = useMemo(() => {
    const map = new Map<string, string>();
    for (const type of types) {
      if (type.nature && !map.has(type.nature)) {
        const label = woNatureLabel(type, language);
        if (label) map.set(type.nature, label);
      }
    }
    return (code: string | null) =>
      code ? (map.get(code) ?? code) : t("home.supervision.wo.noNature");
  }, [types, language, t]);

  const natures = useMemo(() => {
    const seen: (string | null)[] = [];
    for (const c of wo.cells) if (!seen.includes(c.nature_code)) seen.push(c.nature_code);
    return seen;
  }, [wo.cells]);
  const urgencies = useMemo(() => {
    const codes: (string | null)[] = levels.map((l) => l.code);
    if (wo.cells.some((c) => c.urgency_code == null)) codes.push(null);
    for (const c of wo.cells) if (!codes.includes(c.urgency_code)) codes.push(c.urgency_code);
    return codes;
  }, [levels, wo.cells]);
  const count = (nature: string | null, urgency: string | null) =>
    wo.cells.find((c) => c.nature_code === nature && c.urgency_code === urgency)?.count ?? 0;

  return (
    <HomeSection
      title={t("home.supervision.wo.title")}
      definitions={[
        { label: t("home.supervision.wo.open"), text: t("home.def.backlogOpen") },
        { label: t("home.supervision.wo.overdue"), text: t("home.def.woOverdueAll") },
        { label: t("home.supervision.wo.planned"), text: t("home.def.planned") },
        { label: t("home.supervision.wo.reactive"), text: t("home.def.reactive") },
        { label: t("home.supervision.wo.emergency"), text: t("home.def.emergency") },
      ]}
      openLabel={t("home.open.workOrders")}
      openTo={homeLinks.workOrders}
    >
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
        <HomeFigure
          label={t("home.supervision.wo.open")}
          value={formatNumber(wo.open)}
          to={listLinks.workOrders({ scope: "open" })}
        />
        <HomeFigure
          label={t("home.supervision.wo.overdue")}
          value={formatNumber(wo.overdue)}
          tone={wo.overdue > 0 ? "danger" : undefined}
        />
        <HomeFigure
          label={t("home.supervision.wo.planned")}
          value={formatNumber(wo.planned)}
          to={listLinks.workOrders({ scope: "open", mix: "planned" })}
        />
        <HomeFigure
          label={t("home.supervision.wo.reactive")}
          value={formatNumber(wo.reactive)}
          to={listLinks.workOrders({ scope: "open", mix: "reactive" })}
        />
        <HomeFigure
          label={t("home.supervision.wo.emergency")}
          value={formatNumber(wo.emergency)}
          tone={wo.emergency > 0 ? "attention" : undefined}
          to={listLinks.workOrders({ scope: "open", mix: "emergency" })}
        />
      </div>
      {wo.cells.length === 0 ? (
        <HomeNote>{t("home.supervision.wo.empty")}</HomeNote>
      ) : (
        <div className="overflow-x-auto rounded-md border border-surface-border">
          <table className="w-full text-xs">
            <caption className="sr-only">{t("home.supervision.wo.matrixCaption")}</caption>
            <thead className="bg-surface-2 text-text-secondary">
              <tr>
                <th scope="col" className="px-2 py-1.5 text-start font-medium">
                  {t("home.supervision.wo.nature")}
                </th>
                {urgencies.map((u) => (
                  <th key={u ?? "none"} scope="col" className="px-2 py-1.5 text-end font-medium">
                    {u ? urgencyLabel(u) : t("home.noUrgency")}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-surface-border">
              {natures.map((nat) => (
                <tr key={nat ?? "none"}>
                  <th scope="row" className="px-2 py-1.5 text-start font-normal text-text-primary">
                    {natureLabel(nat)}
                  </th>
                  {urgencies.map((u) => {
                    const c = count(nat, u);
                    return (
                      <td
                        key={u ?? "none"}
                        className="px-2 py-1.5 text-end font-mono tabular-nums text-text-primary"
                      >
                        {c > 0 ? formatNumber(c) : <span className="text-text-muted">·</span>}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </HomeSection>
  );
}
