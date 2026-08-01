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
import { saveDashboardLayout } from "@/services/dashboard-service";
import {
  DASHBOARD_WIDGET_IDS,
  type DashboardLayoutV1,
  type DashboardWidgetLayoutEntry,
  DEFAULT_DASHBOARD_LAYOUT,
  mergeWithDefaultLayout,
} from "@shared/dashboard-layout";

function cloneLayout(layout: DashboardLayoutV1): DashboardLayoutV1 {
  return {
    version: 1,
    widgets: layout.widgets.map((w) => ({ ...w })),
  };
}

function reorderOrders(widgets: DashboardWidgetLayoutEntry[]): DashboardWidgetLayoutEntry[] {
  const sorted = [...widgets].sort((a, b) => a.order - b.order);
  return sorted.map((w, i) => ({ ...w, order: i }));
}

function moveWidget(
  widgets: DashboardWidgetLayoutEntry[],
  id: string,
  dir: -1 | 1,
): DashboardWidgetLayoutEntry[] {
  const sorted = [...widgets].sort((a, b) => a.order - b.order);
  const idx = sorted.findIndex((w) => w.id === id);
  if (idx < 0) {
    return reorderOrders(widgets);
  }
  const j = idx + dir;
  if (j < 0 || j >= sorted.length) {
    return reorderOrders(widgets);
  }
  const next = [...sorted];
  const current = next[idx];
  const target = next[j];
  if (!current || !target) {
    return reorderOrders(widgets);
  }
  next[idx] = target;
  next[j] = current;
  return next.map((w, i) => ({ ...w, order: i }));
}

export interface DashboardCustomizeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  layout: DashboardLayoutV1;
  onSaved: (layout: DashboardLayoutV1) => void;
}

export function DashboardCustomizeDialog({
  open,
  onOpenChange,
  layout,
  onSaved,
}: DashboardCustomizeDialogProps) {
  const { t } = useTranslation("dashboard");
  const merged = useMemo(() => mergeWithDefaultLayout(layout), [layout]);
  const [draft, setDraft] = useState<DashboardLayoutV1>(merged);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) {
      setDraft(cloneLayout(mergeWithDefaultLayout(layout)));
    }
  }, [open, layout]);

  const ordered = useMemo(
    () => [...draft.widgets].sort((a, b) => a.order - b.order),
    [draft.widgets],
  );

  const onToggle = (id: string, visible: boolean) => {
    setDraft((d) => ({
      ...d,
      widgets: reorderOrders(d.widgets.map((w) => (w.id === id ? { ...w, visible } : w))),
    }));
  };

  const onMove = (id: string, dir: -1 | 1) => {
    setDraft((d) => ({ ...d, widgets: moveWidget(d.widgets, id, dir) }));
  };

  const onSave = async () => {
    setSaving(true);
    try {
      const json = JSON.stringify(draft);
      await saveDashboardLayout(json);
      onSaved(draft);
      onOpenChange(false);
    } catch {
      /* keep dialog open */
    } finally {
      setSaving(false);
    }
  };

  const labelFor = (id: string) => {
    switch (id) {
      case DASHBOARD_WIDGET_IDS.KPIS:
        return t("layout.labels.kpis");
      case DASHBOARD_WIDGET_IDS.WORKLOAD:
        return t("layout.labels.workload");
      case DASHBOARD_WIDGET_IDS.DI_STATUS:
        return t("layout.labels.diStatus");
      case DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT:
        return t("layout.labels.reliability");
      default:
        return id;
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("layout.customizeTitle")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 py-2">
          {ordered.map((w) => (
            <div key={w.id} className="flex items-center gap-2">
              <Checkbox
                checked={w.visible}
                onCheckedChange={(c) => onToggle(w.id, Boolean(c))}
                aria-label={labelFor(w.id)}
              />
              <span className="flex-1 text-sm">{labelFor(w.id)}</span>
              <Button type="button" variant="outline" size="sm" onClick={() => onMove(w.id, -1)}>
                ↑
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={() => onMove(w.id, 1)}>
                ↓
              </Button>
            </div>
          ))}
        </div>
        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            type="button"
            variant="ghost"
            onClick={() => setDraft({ ...DEFAULT_DASHBOARD_LAYOUT })}
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
