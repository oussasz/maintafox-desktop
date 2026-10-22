/**
 * Maintenance tab: what maintaining the asset has cost and how often it failed (work orders),
 * with the open work, requests and preventive plans linked to their own screens.
 * Benchmark: Maximo asset "Work / Costs" tabs, SAP PM equipment cost and history reports.
 */

import { Activity, CalendarClock, ClipboardList, Wrench } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DetailSectionCard } from "@/components/detail";
import { diStatusToI18nKey } from "@/components/di/status-meta";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import {
  DI_STATUS_TONE,
  PM_PLAN_LIFECYCLE_TONE,
  WO_STATUS_TONE,
  mfTone,
  toneOf,
} from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { useCanReadList } from "@/lib/list-access";
import { useWoOpenRuleText } from "@/lib/wo-open-rule";
import { getAssetMaintenanceProfile } from "@/services/asset-registry-service";
import { listDis } from "@/services/di-service";
import { listWos } from "@/services/wo-service";
import { statusToI18nKey } from "@/utils/wo-status";
import type { Asset } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Most recent records listed per kind; the card titles show how many exist in total. */
const RECENT_ROWS = 10;

export function AssetMaintenanceTab({ asset }: { asset: Asset }) {
  const { t } = useTranslation(["equipment", "ot", "di", "pm"]);
  // The open count follows one named "open OT" rule ("Not yet closed"): the tile names it.
  const woOpenRuleText = useWoOpenRuleText();
  const { formatCurrency, formatDate, formatDecimal, formatPercent, formatNumber } =
    useFormatters();
  const navigate = useNavigate();
  const [rollUp, setRollUp] = useState(false);

  const profileLoad = useMemo(
    () => () => getAssetMaintenanceProfile(asset.id, rollUp),
    [asset.id, rollUp],
  );
  const profile = useLoad(profileLoad, `${asset.id}:${rollUp}:${asset.row_version}`);

  // Loaded separately: a failure in one list must not hide the other. Each list is asked only by
  // a user who may read it (ot.view, di.view); otherwise its card names the missing right.
  const canReadWos = useCanReadList("workOrders");
  const canReadDis = useCanReadList("requests");
  const woLoad = useMemo(
    () =>
      canReadWos ? () => listWos({ equipment_id: asset.id, limit: RECENT_ROWS, offset: 0 }) : null,
    [asset.id, canReadWos],
  );
  const diLoad = useMemo(
    () =>
      canReadDis ? () => listDis({ equipment_id: asset.id, limit: RECENT_ROWS, offset: 0 }) : null,
    [asset.id, canReadDis],
  );
  const wos = useLoad(woLoad, `${asset.id}:${canReadWos}`);
  const dis = useLoad(diLoad, `${asset.id}:${canReadDis}`);

  const p = profile.data;
  const maxYear = p ? Math.max(0, ...p.cost_by_year.map((y) => y.total)) : 0;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <label className="flex min-h-[44px] items-center gap-2 text-sm">
          <Switch checked={rollUp} onCheckedChange={setRollUp} />
          {t("maintenance.rollUp")}
        </label>
        <PermissionGate permission={P.RAM_VIEW}>
          <Button
            variant="outline"
            className="h-11 gap-1.5"
            onClick={() => void navigate(`/reliability?equipment=${asset.id}`)}
          >
            <Activity className="h-4 w-4" aria-hidden />
            {t("maintenance.openRams")}
          </Button>
        </PermissionGate>
      </div>

      {profile.loading && !p ? (
        <LoadingState />
      ) : profile.error || !p ? (
        <ErrorState
          error={profile.error}
          onRetry={() => void profile.reload()}
          retrying={profile.loading}
        />
      ) : (
        <>
          {rollUp ? (
            <p className="text-xs text-text-muted">
              {t("maintenance.rollUpScope", { count: p.asset_count })}
            </p>
          ) : null}
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
            <KpiTile
              label={t("maintenance.kpi.workOrders")}
              value={formatNumber(p.work_order_count)}
              hint={
                woOpenRuleText(p.open_work_order_rule)
                  ? `${formatNumber(p.open_work_order_count)} · ${woOpenRuleText(p.open_work_order_rule)?.label ?? ""}`
                  : t("maintenance.kpi.openHint", { count: p.open_work_order_count })
              }
            />
            <KpiTile
              label={t("maintenance.kpi.failures")}
              value={formatNumber(p.failure_work_order_count)}
              hint={
                p.last_failure_at
                  ? t("maintenance.kpi.lastFailure", { date: formatDate(p.last_failure_at) })
                  : t("maintenance.kpi.noFailure")
              }
            />
            <KpiTile
              label={t("maintenance.kpi.downtime")}
              value={formatDecimal(p.downtime_hours, 1)}
              unit={t("maintenance.kpi.hours")}
              hint={t("maintenance.kpi.downtimeHint")}
            />
            <KpiTile
              label={t("maintenance.kpi.costLifetime")}
              value={formatCurrency(p.cost_lifetime.total)}
              hint={t("maintenance.kpi.costSplit", {
                labor: formatCurrency(p.cost_lifetime.labor),
                parts: formatCurrency(p.cost_lifetime.parts),
                services: formatCurrency(p.cost_lifetime.services),
              })}
            />
            <KpiTile
              label={t("maintenance.kpi.costRecent", { months: p.recent_window_months })}
              value={formatCurrency(p.cost_recent.total)}
              hint={t("maintenance.kpi.costSplit", {
                labor: formatCurrency(p.cost_recent.labor),
                parts: formatCurrency(p.cost_recent.parts),
                services: formatCurrency(p.cost_recent.services),
              })}
            />
            <KpiTile
              label={t("maintenance.kpi.costVsReplacement")}
              value={
                p.cost_to_replacement_ratio != null
                  ? formatPercent(p.cost_to_replacement_ratio)
                  : "—"
              }
              hint={
                p.replacement_value != null
                  ? t("maintenance.kpi.replacementHint", {
                      value: formatCurrency(p.replacement_value),
                    })
                  : t("maintenance.kpi.noReplacementValue")
              }
            />
          </div>

          <DetailSectionCard title={t("maintenance.costByYear")} icon={CalendarClock}>
            {p.cost_by_year.length === 0 ? (
              <p className="text-sm text-text-muted">{t("maintenance.noCost")}</p>
            ) : (
              <ul className="space-y-1.5" aria-label={t("maintenance.costByYear")}>
                {p.cost_by_year.map((y) => (
                  <li key={y.year} className="grid grid-cols-[3.5rem_1fr_auto] items-center gap-3">
                    <span className="text-xs font-medium tabular-nums">{y.year}</span>
                    <span className="h-2.5 rounded-full bg-surface-2">
                      <span
                        className={`block h-2.5 rounded-full ${mfTone.info.bar}`}
                        style={{ width: maxYear > 0 ? `${(y.total / maxYear) * 100}%` : "0%" }}
                      />
                    </span>
                    <span className="text-xs tabular-nums">
                      {formatCurrency(y.total)}
                      <span className="ml-1 text-text-muted">
                        {t("maintenance.woCount", { count: y.work_order_count })}
                      </span>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </DetailSectionCard>

          <DetailSectionCard title={t("maintenance.pmPlans")} icon={CalendarClock}>
            {p.pm_plans.length === 0 ? (
              <p className="text-sm text-text-muted">{t("maintenance.noPmPlans")}</p>
            ) : (
              <ul className="divide-y divide-surface-border">
                {p.pm_plans.map((plan) => (
                  <li key={plan.id} className="flex flex-wrap items-center gap-3 py-2">
                    <Button
                      variant="link"
                      className="h-auto min-h-[36px] p-0 font-mono text-xs"
                      onClick={() => void navigate(`/pm?planId=${plan.id}`)}
                    >
                      {plan.code}
                    </Button>
                    <span className="min-w-0 flex-1 truncate text-sm">{plan.title}</span>
                    <StatusChip
                      label={t(`pm:lifecycle.${plan.lifecycle_status}`, {
                        defaultValue: plan.lifecycle_status,
                      })}
                      tone={toneOf(PM_PLAN_LIFECYCLE_TONE, plan.lifecycle_status)}
                    />
                    <span className="text-xs text-text-muted">
                      {plan.next_due_at
                        ? t("maintenance.nextDue", { date: formatDate(plan.next_due_at) })
                        : t("maintenance.noNextDue")}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </DetailSectionCard>
        </>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <DetailSectionCard
          title={
            wos.data
              ? t("maintenance.workOrders", {
                  shown: wos.data.items.length,
                  total: wos.data.total,
                })
              : t("maintenance.workOrdersTitle")
          }
          icon={Wrench}
        >
          {!canReadWos ? (
            <ListAccessNote list="workOrders" />
          ) : wos.loading && !wos.data ? (
            <LoadingState />
          ) : wos.error || !wos.data ? (
            <ErrorState
              error={wos.error}
              onRetry={() => void wos.reload()}
              retrying={wos.loading}
            />
          ) : wos.data.items.length === 0 ? (
            <p className="text-sm text-text-muted">{t("maintenance.noWorkOrders")}</p>
          ) : (
            <ul className="divide-y divide-surface-border">
              {wos.data.items.map((wo) => (
                <li key={wo.id} className="flex items-center gap-2 py-2">
                  <LinkedEntityBadge
                    entity="work_order"
                    code={wo.code}
                    entityId={wo.id}
                    title={wo.title}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm">{wo.title}</span>
                  <StatusChip
                    label={t(`ot:status.${statusToI18nKey(wo.status_code ?? "draft")}`)}
                    tone={toneOf(WO_STATUS_TONE, wo.status_code)}
                  />
                </li>
              ))}
            </ul>
          )}
        </DetailSectionCard>
        <DetailSectionCard
          title={
            dis.data
              ? t("maintenance.requests", { shown: dis.data.items.length, total: dis.data.total })
              : t("maintenance.requestsTitle")
          }
          icon={ClipboardList}
        >
          {!canReadDis ? (
            <ListAccessNote list="requests" />
          ) : dis.loading && !dis.data ? (
            <LoadingState />
          ) : dis.error || !dis.data ? (
            <ErrorState
              error={dis.error}
              onRetry={() => void dis.reload()}
              retrying={dis.loading}
            />
          ) : dis.data.items.length === 0 ? (
            <p className="text-sm text-text-muted">{t("maintenance.noRequests")}</p>
          ) : (
            <ul className="divide-y divide-surface-border">
              {dis.data.items.map((di) => (
                <li key={di.id} className="flex items-center gap-2 py-2">
                  <LinkedEntityBadge entity="di" code={di.code} entityId={di.id} title={di.title} />
                  <span className="min-w-0 flex-1 truncate text-sm">{di.title}</span>
                  <StatusChip
                    label={t(`di:status.${diStatusToI18nKey(di.status)}`)}
                    tone={toneOf(DI_STATUS_TONE, di.status)}
                  />
                </li>
              ))}
            </ul>
          )}
        </DetailSectionCard>
      </div>
    </div>
  );
}
