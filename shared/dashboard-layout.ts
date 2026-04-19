/** Mirrors `DEFAULT_DASHBOARD_LAYOUT_JSON` in `commands/dashboard.rs`. */

export const DASHBOARD_WIDGET_IDS = {
  KPIS: "kpis",
  WORKLOAD: "workload",
  DI_STATUS: "di_status",
  RELIABILITY_SNAPSHOT: "reliability_snapshot",
} as const;

export type DashboardWidgetId = (typeof DASHBOARD_WIDGET_IDS)[keyof typeof DASHBOARD_WIDGET_IDS];

export interface DashboardWidgetLayoutEntry {
  id: DashboardWidgetId | string;
  order: number;
  visible: boolean;
}

export interface DashboardLayoutV1 {
  version: 1;
  widgets: DashboardWidgetLayoutEntry[];
}

export const DEFAULT_DASHBOARD_LAYOUT: DashboardLayoutV1 = {
  version: 1,
  widgets: [
    { id: DASHBOARD_WIDGET_IDS.KPIS, order: 0, visible: true },
    { id: DASHBOARD_WIDGET_IDS.WORKLOAD, order: 1, visible: true },
    { id: DASHBOARD_WIDGET_IDS.DI_STATUS, order: 2, visible: true },
    { id: DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT, order: 3, visible: true },
  ],
};

/** Optional permission required to load widget data (not stored in layout). */
export const DASHBOARD_WIDGET_PERMISSION: Partial<Record<string, string>> = {
  [DASHBOARD_WIDGET_IDS.DI_STATUS]: "di.view",
  [DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT]: "rep.view",
};

export function parseDashboardLayoutJson(raw: string): DashboardLayoutV1 {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  return normalizeDashboardLayout(parsed);
}

export function normalizeDashboardLayout(input: unknown): DashboardLayoutV1 {
  if (!input || typeof input !== "object") {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  const o = input as Record<string, unknown>;
  if (o["version"] !== 1 || !Array.isArray(o["widgets"])) {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  const seen = new Set<string>();
  const widgets: DashboardWidgetLayoutEntry[] = [];
  for (const w of o["widgets"] as unknown[]) {
    if (!w || typeof w !== "object") {
      continue;
    }
    const e = w as Record<string, unknown>;
    const id = typeof e["id"] === "string" ? e["id"] : "";
    if (!id || id.length > 64 || seen.has(id)) {
      continue;
    }
    seen.add(id);
    const order =
      typeof e["order"] === "number" && Number.isFinite(e["order"])
        ? (e["order"] as number)
        : widgets.length;
    const visible = typeof e["visible"] === "boolean" ? e["visible"] : true;
    widgets.push({ id, order, visible });
  }
  if (widgets.length === 0) {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  widgets.sort((a, b) => a.order - b.order);
  return { version: 1, widgets };
}

export function mergeWithDefaultLayout(normalized: DashboardLayoutV1): DashboardLayoutV1 {
  const byId = new Map(normalized.widgets.map((w) => [w.id, w]));
  const merged: DashboardWidgetLayoutEntry[] = [];
  for (const d of DEFAULT_DASHBOARD_LAYOUT.widgets) {
    const cur = byId.get(d.id);
    merged.push(cur ? { id: d.id, order: cur.order, visible: cur.visible } : { ...d });
  }
  for (const w of normalized.widgets) {
    if (!DEFAULT_DASHBOARD_LAYOUT.widgets.some((x) => x.id === w.id)) {
      merged.push(w);
    }
  }
  merged.sort((a, b) => a.order - b.order);
  return { version: 1, widgets: merged };
}
