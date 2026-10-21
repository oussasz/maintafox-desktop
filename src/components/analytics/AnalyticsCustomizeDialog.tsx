import { ArrowDown, ArrowUp } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { EntityFormDialog, EntityFormFooter, EntityFormSection } from "@/components/entity-form";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { describeAppError } from "@/lib/app-error";
import { saveAnalyticsDashboardLayout } from "@/services/analytics-dashboard-service";
import {
  ANALYTICS_DASHBOARD_WIDGET_IDS,
  mergeWithDefaultAnalyticsLayout,
  type AnalyticsDashboardLayoutV1,
  type AnalyticsDashboardWidgetLayoutEntry,
} from "@shared/analytics-dashboard-layout";

function cloneLayout(layout: AnalyticsDashboardLayoutV1): AnalyticsDashboardLayoutV1 {
  const out: AnalyticsDashboardLayoutV1 = {
    version: 1,
    widgets: layout.widgets.map((w) => ({ ...w })),
  };
  if (layout.overview_kpis?.length) {
    out.overview_kpis = layout.overview_kpis.map((k) => ({ ...k }));
  }
  return out;
}

function reorderOrders(
  widgets: AnalyticsDashboardWidgetLayoutEntry[],
): AnalyticsDashboardWidgetLayoutEntry[] {
  const sorted = [...widgets].sort((a, b) => a.order - b.order);
  return sorted.map((w, i) => ({ ...w, order: i }));
}

function moveWidget(
  widgets: AnalyticsDashboardWidgetLayoutEntry[],
  id: string,
  dir: -1 | 1,
): AnalyticsDashboardWidgetLayoutEntry[] {
  const sorted = [...widgets].sort((a, b) => a.order - b.order);
  const idx = sorted.findIndex((w) => w.id === id);
  if (idx < 0) return widgets;
  const next = idx + dir;
  if (next < 0 || next >= sorted.length) return widgets;
  const copy = [...sorted];
  const current = copy[idx];
  const target = copy[next];
  if (current === undefined || target === undefined) return widgets;
  copy[idx] = target;
  copy[next] = current;
  return reorderOrders(copy);
}

const LABEL_KEYS: Record<string, string> = {
  [ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS]: "hub.dashboards.widgets.opsKpis",
  [ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE]: "hub.dashboards.widgets.pmDue",
  [ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS]: "hub.dashboards.widgets.reliability",
  [ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST]: "hub.dashboards.widgets.maintenanceCost",
  [ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD]: "hub.dashboards.widgets.workload",
  [ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS]: "hub.dashboards.widgets.diStatus",
};

export function AnalyticsCustomizeDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  layout: AnalyticsDashboardLayoutV1;
  onSaved: (layout: AnalyticsDashboardLayoutV1) => void;
}) {
  const { t } = useTranslation("reports");
  const [draft, setDraft] = useState(() => cloneLayout(props.layout));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (props.open) {
      setDraft(cloneLayout(mergeWithDefaultAnalyticsLayout(props.layout)));
      setError(null);
    }
  }, [props.open, props.layout]);

  const sorted = useMemo(
    () => [...draft.widgets].sort((a, b) => a.order - b.order),
    [draft.widgets],
  );

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const next = mergeWithDefaultAnalyticsLayout(draft);
      await saveAnalyticsDashboardLayout({ layout_json: JSON.stringify(next) });
      props.onSaved(next);
      props.onOpenChange(false);
    } catch (e: unknown) {
      setError(describeAppError(e).title);
    } finally {
      setSaving(false);
    }
  };

  return (
    <EntityFormDialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t("hub.dashboards.customizeTitle")}
      footer={
        <EntityFormFooter
          cancelLabel={t("hub.dashboards.cancel")}
          primaryLabel={t("hub.dashboards.saveLayout")}
          onCancel={() => props.onOpenChange(false)}
          primaryType="button"
          onPrimary={() => void onSave()}
          primaryLoading={saving}
        />
      }
    >
      <EntityFormSection title={t("hub.dashboards.widgetsSection")}>
        {sorted.map((w) => (
          <div
            key={w.id}
            className="flex items-center justify-between gap-2 rounded-md border border-surface-border px-3 py-2"
          >
            <label className="flex min-w-0 flex-1 items-center gap-2 text-sm">
              <Checkbox
                checked={w.visible}
                onCheckedChange={(c) => {
                  setDraft((prev) => ({
                    ...prev,
                    widgets: prev.widgets.map((x) =>
                      x.id === w.id ? { ...x, visible: c === true } : x,
                    ),
                  }));
                }}
              />
              <span className="truncate">
                {t(LABEL_KEYS[w.id] ?? w.id, { defaultValue: w.id })}
              </span>
            </label>
            <div className="flex shrink-0 gap-1">
              <Button
                type="button"
                variant="ghost"
                className="h-11 w-11 p-0"
                aria-label={t("hub.customize.moveUp", {
                  widget: t(LABEL_KEYS[w.id] ?? w.id, { defaultValue: w.id }),
                })}
                onClick={() =>
                  setDraft((prev) => ({
                    ...prev,
                    widgets: moveWidget(prev.widgets, w.id, -1),
                  }))
                }
              >
                <ArrowUp className="h-4 w-4" aria-hidden />
              </Button>
              <Button
                type="button"
                variant="ghost"
                className="h-11 w-11 p-0"
                aria-label={t("hub.customize.moveDown", {
                  widget: t(LABEL_KEYS[w.id] ?? w.id, { defaultValue: w.id }),
                })}
                onClick={() =>
                  setDraft((prev) => ({
                    ...prev,
                    widgets: moveWidget(prev.widgets, w.id, 1),
                  }))
                }
              >
                <ArrowDown className="h-4 w-4" aria-hidden />
              </Button>
            </div>
          </div>
        ))}
        {error ? <p className="text-sm text-text-danger">{error}</p> : null}
      </EntityFormSection>
    </EntityFormDialog>
  );
}
