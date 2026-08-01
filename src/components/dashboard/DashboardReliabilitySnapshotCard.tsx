import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { getDashboardReliabilitySnapshotSummary } from "@/services/dashboard-service";
import type { DashboardReliabilitySnapshotSummary as SummaryModel } from "@shared/ipc-types";
import { RELIABILITY_RAM_EVIDENCE_HASH } from "@shared/kpi-definitions";

export function DashboardReliabilitySnapshotCard() {
  const { t } = useTranslation("dashboard");
  const navigate = useNavigate();
  const [summary, setSummary] = useState<SummaryModel | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getDashboardReliabilitySnapshotSummary()
      .then((r) => {
        if (!cancelled) setSummary(r);
      })
      .catch(() => {
        if (!cancelled) setSummary(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (loading) {
    return (
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base font-medium">{t("widgets.reliability.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-text-muted">{t("chart.loading")}</p>
        </CardContent>
      </Card>
    );
  }

  if (!summary?.available) {
    return null;
  }

  const dq = summary.avg_data_quality_score;
  const mtbf = summary.avg_mtbf_hours;

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-base font-medium">{t("widgets.reliability.title")}</CardTitle>
        <button
          type="button"
          className="text-xs font-medium text-primary underline-offset-2 hover:underline"
          onClick={() => navigate(`/reliability/foundation${RELIABILITY_RAM_EVIDENCE_HASH}`)}
        >
          {t("widgets.reliability.openEvidence")}
        </button>
      </CardHeader>
      <CardContent>
        <p className="mb-3 text-xs text-text-muted">{t("widgets.reliability.subtitle")}</p>
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
          <div>
            <dt className="text-text-muted">{t("widgets.reliability.snapshots")}</dt>
            <dd className="font-semibold tabular-nums text-text-primary">
              {summary.snapshot_count}
            </dd>
          </div>
          <div>
            <dt className="text-text-muted">{t("widgets.reliability.dqScore")}</dt>
            <dd className="font-semibold tabular-nums text-text-primary">
              {dq != null && Number.isFinite(dq) ? dq.toFixed(2) : "—"}
            </dd>
          </div>
          <div>
            <dt className="text-text-muted">{t("widgets.reliability.mtbf")}</dt>
            <dd className="font-semibold tabular-nums text-text-primary">
              {mtbf != null && Number.isFinite(mtbf) ? mtbf.toFixed(1) : "—"}
            </dd>
          </div>
          <div>
            <dt className="text-text-muted">{t("widgets.reliability.events")}</dt>
            <dd className="font-semibold tabular-nums text-text-primary">
              {summary.total_failure_events}
            </dd>
          </div>
        </dl>
      </CardContent>
    </Card>
  );
}
