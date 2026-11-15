/**
 * Supervisor home: equipment flagged "pannes répétitives" (at least N unplanned failures in the
 * last D days, both from Paramètres > Fiabilité). Shown with the assets section (equipment view
 * right); a failed read says so, never shows zero.
 */
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";
import { appErrorText } from "@/lib/app-error";
import { listRepeatFailureAssets } from "@/services/asset-follow-up-service";

import { HomeFigure, HomeNote, HomeRecordList, HomeSection, usePanelLoad } from "./HomeParts";
import { homeLinks } from "./home-links";

export function HomeRepeatFailuresSection({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const { formatNumber } = useFormatters();
  const load = useCallback(() => listRepeatFailureAssets(), []);
  const state = usePanelLoad(load, refreshSignal);
  const data = state.data;
  return (
    <HomeSection
      title={t("home.supervision.repeatFailures.title")}
      definitions={[
        {
          label: t("home.supervision.repeatFailures.count"),
          text: data
            ? t("home.def.repeatFailures", {
                min: data.rule.min_failures,
                days: data.rule.window_days,
              })
            : t("home.def.repeatFailuresRule"),
        },
      ]}
      openLabel={t("home.open.equipment")}
      openTo={homeLinks.equipment}
    >
      {state.error != null ? (
        <HomeNote tone="danger">
          {t("home.supervision.repeatFailures.error", { reason: appErrorText(state.error) })}
        </HomeNote>
      ) : data ? (
        <>
          <HomeFigure
            label={t("home.supervision.repeatFailures.count")}
            value={formatNumber(data.assets.length)}
            tone={data.assets.length > 0 ? "warning" : undefined}
          />
          <HomeRecordList
            ariaLabel={t("home.supervision.repeatFailures.listAria")}
            emptyText={t("home.supervision.repeatFailures.empty")}
            total={data.assets.length}
            rows={data.assets.map((a) => ({
              key: a.asset_id,
              primary: `${a.asset_code} — ${a.asset_name}`,
              badge: t("home.supervision.repeatFailures.failures", { count: a.failure_count }),
              tone: "warning",
              to: homeLinks.asset(a.asset_id),
            }))}
          />
        </>
      ) : null}
    </HomeSection>
  );
}
