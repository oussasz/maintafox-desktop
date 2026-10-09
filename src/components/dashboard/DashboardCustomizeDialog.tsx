import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { mfVisualMetric } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { saveDashboardLayout, saveOverviewKpiLayout } from "@/services/dashboard-service";
import {
  DASHBOARD_WIDGET_IDS,
  type DashboardLayoutV1,
  type DashboardWidgetLayoutEntry,
  DEFAULT_DASHBOARD_LAYOUT,
  mergeWithDefaultLayout,
} from "@shared/dashboard-layout";
import {
  OVERVIEW_KPI_ACCENT_OPTIONS,
  OVERVIEW_KPI_ACCENTS,
  OVERVIEW_KPI_REGISTRY,
  type OverviewKpiAccent,
  type OverviewKpiLayoutEntry,
} from "@shared/overview-kpi-registry";

function cloneDashboardLayout(layout: DashboardLayoutV1): DashboardLayoutV1 {
  const merged = mergeWithDefaultLayout(layout);
  return {
    version: 1,
    widgets: merged.widgets.map((w) => ({ ...w })),
  };
}

function reorderOrders<T extends { order: number }>(items: T[]): T[] {
  return items.map((w, i) => ({ ...w, order: i }));
}

function moveAtIndex<T>(items: T[], index: number, dir: -1 | 1): T[] {
  const j = index + dir;
  if (index < 0 || j < 0 || j >= items.length) return items;
  const next = [...items];
  const a = next[index];
  const b = next[j];
  if (a === undefined || b === undefined) return items;
  next[index] = b;
  next[j] = a;
  return next;
}

function buildFullKpiDraft(
  saved: OverviewKpiLayoutEntry[],
  allowedIds: string[],
): OverviewKpiLayoutEntry[] {
  const byId = new Map(saved.map((k) => [k.id, k]));
  const orderedSaved = [...saved].sort((a, b) => a.order - b.order);
  const result: OverviewKpiLayoutEntry[] = [];
  const seen = new Set<string>();

  for (const entry of orderedSaved) {
    if (!allowedIds.includes(entry.id) || seen.has(entry.id)) continue;
    seen.add(entry.id);
    result.push({
      id: entry.id,
      order: result.length,
      visible: entry.visible,
      ...(entry.accent ? { accent: entry.accent } : {}),
    });
  }
  for (const id of allowedIds) {
    if (seen.has(id)) continue;
    const cur = byId.get(id);
    result.push({
      id,
      order: result.length,
      visible: cur?.visible ?? false,
      ...(cur?.accent ? { accent: cur.accent } : {}),
    });
  }
  return result;
}

export interface DashboardCustomizeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  layout: DashboardLayoutV1;
  overviewKpiLayout: OverviewKpiLayoutEntry[];
  onSaved: (layout: DashboardLayoutV1) => void;
  onOverviewKpiSaved: (layout: OverviewKpiLayoutEntry[]) => void;
}

export function DashboardCustomizeDialog({
  open,
  onOpenChange,
  layout,
  overviewKpiLayout,
  onSaved,
  onOverviewKpiSaved,
}: DashboardCustomizeDialogProps) {
  const { t } = useTranslation("dashboard");
  const { can } = usePermissions();
  const [draft, setDraft] = useState<DashboardLayoutV1>(() => cloneDashboardLayout(layout));
  const [kpiDraft, setKpiDraft] = useState<OverviewKpiLayoutEntry[]>([]);
  const [saving, setSaving] = useState(false);

  const allowedKpiIds = useMemo(
    () => OVERVIEW_KPI_REGISTRY.filter((e) => can(e.permission)).map((e) => e.semanticKey),
    [can],
  );

  useEffect(() => {
    if (open) {
      setDraft(cloneDashboardLayout(layout));
      setKpiDraft(buildFullKpiDraft(overviewKpiLayout, allowedKpiIds));
    }
  }, [open, layout, overviewKpiLayout, allowedKpiIds]);

  const orderedWidgets = useMemo(
    () =>
      [...draft.widgets]
        .filter((w) => w.id !== DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT)
        .sort((a, b) => a.order - b.order),
    [draft.widgets],
  );

  const orderedKpis = useMemo(() => [...kpiDraft].sort((a, b) => a.order - b.order), [kpiDraft]);

  const onToggleWidget = (id: string, visible: boolean) => {
    setDraft((d) => ({
      ...d,
      widgets: d.widgets.map((w) => (w.id === id ? { ...w, visible } : w)),
    }));
  };

  const onMoveWidget = (index: number, dir: -1 | 1) => {
    setDraft((d) => {
      const sorted = [...d.widgets]
        .filter((w) => w.id !== DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT)
        .sort((a, b) => a.order - b.order);
      const moved = reorderOrders(moveAtIndex(sorted, index, dir));
      const other = d.widgets.filter((w) => w.id === DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT);
      return { ...d, widgets: [...moved, ...other] as DashboardWidgetLayoutEntry[] };
    });
  };

  const onToggleKpi = (id: string, visible: boolean) => {
    setKpiDraft((prev) => prev.map((k) => (k.id === id ? { ...k, visible } : k)));
  };

  const onMoveKpi = (index: number, dir: -1 | 1) => {
    setKpiDraft((prev) => {
      const sorted = [...prev].sort((a, b) => a.order - b.order);
      return reorderOrders(moveAtIndex(sorted, index, dir));
    });
  };

  const onAccentKpi = (id: string, accent: OverviewKpiAccent) => {
    setKpiDraft((prev) =>
      prev.map((k) => {
        if (k.id !== id) return k;
        if (accent === OVERVIEW_KPI_ACCENTS.DEFAULT) {
          const withoutAccent = { ...k };
          delete withoutAccent.accent;
          return withoutAccent;
        }
        return { ...k, accent };
      }),
    );
  };

  const onSave = async () => {
    setSaving(true);
    try {
      const widgetsOrdered = reorderOrders(
        [...draft.widgets]
          .filter((w) => w.id !== DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT)
          .sort((a, b) => a.order - b.order),
      );
      const pageLayout: DashboardLayoutV1 = {
        version: 1,
        widgets: widgetsOrdered,
      };
      await saveDashboardLayout(JSON.stringify(pageLayout));
      onSaved(pageLayout);

      const kpiToSave = reorderOrders([...kpiDraft].sort((a, b) => a.order - b.order));
      await saveOverviewKpiLayout({ overview_kpis_json: JSON.stringify(kpiToSave) });
      onOverviewKpiSaved(kpiToSave);

      onOpenChange(false);
    } catch {
      /* keep dialog open */
    } finally {
      setSaving(false);
    }
  };

  const labelForWidget = (id: string) => {
    switch (id) {
      case DASHBOARD_WIDGET_IDS.KPIS:
        return t("layout.labels.kpis");
      case DASHBOARD_WIDGET_IDS.WORKLOAD:
        return t("layout.labels.workload");
      case DASHBOARD_WIDGET_IDS.DI_STATUS:
        return t("layout.labels.diStatus");
      default:
        return id;
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("layout.customizeTitle")}</DialogTitle>
        </DialogHeader>
        <div className="max-h-[60vh] space-y-4 overflow-y-auto py-2">
          <div className="space-y-3">
            {orderedWidgets.map((w, index) => (
              <div key={w.id} className="flex items-center gap-2">
                <Checkbox
                  checked={w.visible}
                  onCheckedChange={(c) => onToggleWidget(w.id, Boolean(c))}
                  aria-label={labelForWidget(w.id)}
                />
                <span className="flex-1 text-sm">{labelForWidget(w.id)}</span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={index === 0}
                  onClick={() => onMoveWidget(index, -1)}
                  aria-label={t("layout.moveUp")}
                >
                  ↑
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={index === orderedWidgets.length - 1}
                  onClick={() => onMoveWidget(index, 1)}
                  aria-label={t("layout.moveDown")}
                >
                  ↓
                </Button>
              </div>
            ))}
          </div>
          <div className="space-y-3 border-t border-surface-border pt-3">
            <p className="text-xs font-medium uppercase tracking-wide text-text-muted">
              {t("layout.metricCardsHeading")}
            </p>
            {orderedKpis.map((w, index) => {
              const reg = OVERVIEW_KPI_REGISTRY.find((r) => r.semanticKey === w.id);
              if (!reg) return null;
              const accent = w.accent ?? OVERVIEW_KPI_ACCENTS.DEFAULT;
              return (
                <div
                  key={w.id}
                  className="flex flex-col gap-1.5 sm:flex-row sm:items-center sm:gap-2"
                >
                  <div className="flex min-w-0 flex-1 items-center gap-2">
                    <Checkbox
                      checked={w.visible}
                      onCheckedChange={(c) => onToggleKpi(w.id, Boolean(c))}
                      aria-label={t(reg.labelKey as never)}
                    />
                    <span className="min-w-0 flex-1 truncate text-sm">
                      {t(reg.labelKey as never)}
                    </span>
                  </div>
                  <div
                    className="flex items-center gap-1 ps-7 sm:ps-0"
                    role="group"
                    aria-label={t("layout.accentPicker")}
                  >
                    {OVERVIEW_KPI_ACCENT_OPTIONS.map((opt) => (
                      <button
                        key={opt}
                        type="button"
                        title={t(`layout.accent.${opt}` as never)}
                        aria-label={t(`layout.accent.${opt}` as never)}
                        aria-pressed={accent === opt}
                        className={cn(
                          "h-4 w-4 rounded-full border border-surface-border transition-transform",
                          mfVisualMetric.accentDot[opt],
                          accent === opt
                            ? "ring-2 ring-ring ring-offset-1 ring-offset-background scale-110"
                            : "opacity-70 hover:opacity-100",
                        )}
                        onClick={() => onAccentKpi(w.id, opt)}
                      />
                    ))}
                  </div>
                  <div className="flex items-center gap-1 ps-7 sm:ps-0">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={index === 0}
                      onClick={() => onMoveKpi(index, -1)}
                      aria-label={t("layout.moveUp")}
                    >
                      ↑
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={index === orderedKpis.length - 1}
                      onClick={() => onMoveKpi(index, 1)}
                      aria-label={t("layout.moveDown")}
                    >
                      ↓
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              setDraft(cloneDashboardLayout(DEFAULT_DASHBOARD_LAYOUT));
              setKpiDraft(buildFullKpiDraft([], allowedKpiIds));
            }}
          >
            {t("layout.reset")}
          </Button>
          <Button type="button" onClick={() => void onSave()} disabled={saving}>
            {t("layout.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
