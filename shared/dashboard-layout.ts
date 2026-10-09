import { P, type PermissionName } from "@shared/rbac/permissions.generated";
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
  /** Overview VisualMetricCard order/visibility. Persisted in `user_dashboard_layouts`. */
  metric_cards?: DashboardWidgetLayoutEntry[];
}

export const DASHBOARD_METRIC_CARD_IDS = {
  OPEN_DIS: "metric.open_dis",
  OPEN_WOS: "metric.open_wos",
  OVERDUE: "metric.overdue_items",
  TOTAL_ASSETS: "metric.total_assets",
} as const;

export type DashboardMetricCardId =
  (typeof DASHBOARD_METRIC_CARD_IDS)[keyof typeof DASHBOARD_METRIC_CARD_IDS];

export const DEFAULT_DASHBOARD_METRIC_CARDS: DashboardWidgetLayoutEntry[] = [
  { id: DASHBOARD_METRIC_CARD_IDS.OPEN_DIS, order: 0, visible: true },
  { id: DASHBOARD_METRIC_CARD_IDS.OPEN_WOS, order: 1, visible: true },
  { id: DASHBOARD_METRIC_CARD_IDS.OVERDUE, order: 2, visible: true },
  { id: DASHBOARD_METRIC_CARD_IDS.TOTAL_ASSETS, order: 3, visible: true },
];

/** Operational triage defaults — RAMS lives only under `/analytics` (not Home). */
export const DEFAULT_DASHBOARD_LAYOUT: DashboardLayoutV1 = {
  version: 1,
  widgets: [
    { id: DASHBOARD_WIDGET_IDS.KPIS, order: 0, visible: true },
    { id: DASHBOARD_WIDGET_IDS.WORKLOAD, order: 1, visible: true },
    { id: DASHBOARD_WIDGET_IDS.DI_STATUS, order: 2, visible: true },
    // Kept for layout merge/compat; never shown on Home (see canShowWidget).
    { id: DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT, order: 3, visible: false },
  ],
  metric_cards: DEFAULT_DASHBOARD_METRIC_CARDS.map((c) => ({ ...c })),
};

/** Optional permission required to load widget data (not stored in layout). */
export const DASHBOARD_WIDGET_PERMISSION: Partial<Record<string, PermissionName>> = {
  [DASHBOARD_WIDGET_IDS.DI_STATUS]: P.DI_VIEW,
  [DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT]: P.REP_VIEW,
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

function parseLayoutEntries(raw: unknown): DashboardWidgetLayoutEntry[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const seen = new Set<string>();
  const entries: DashboardWidgetLayoutEntry[] = [];
  for (const w of raw) {
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
        : entries.length;
    const visible = typeof e["visible"] === "boolean" ? e["visible"] : true;
    entries.push({ id, order, visible });
  }
  entries.sort((a, b) => a.order - b.order);
  return entries;
}

function mergeEntries(
  current: DashboardWidgetLayoutEntry[] | undefined,
  defaults: DashboardWidgetLayoutEntry[],
): DashboardWidgetLayoutEntry[] {
  const byId = new Map((current ?? []).map((w) => [w.id, w]));
  const merged: DashboardWidgetLayoutEntry[] = [];
  for (const d of defaults) {
    const cur = byId.get(d.id);
    merged.push(cur ? { id: d.id, order: cur.order, visible: cur.visible } : { ...d });
  }
  for (const w of current ?? []) {
    if (!defaults.some((x) => x.id === w.id)) {
      merged.push(w);
    }
  }
  merged.sort((a, b) => a.order - b.order);
  return merged;
}

export function normalizeDashboardLayout(input: unknown): DashboardLayoutV1 {
  if (!input || typeof input !== "object") {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  const o = input as Record<string, unknown>;
  if (o["version"] !== 1 || !Array.isArray(o["widgets"])) {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  const widgets = parseLayoutEntries(o["widgets"]);
  if (widgets.length === 0) {
    return DEFAULT_DASHBOARD_LAYOUT;
  }
  const metric_cards = parseLayoutEntries(o["metric_cards"]);
  return {
    version: 1,
    widgets,
    metric_cards:
      metric_cards.length > 0
        ? metric_cards
        : DEFAULT_DASHBOARD_METRIC_CARDS.map((c) => ({ ...c })),
  };
}

export function mergeWithDefaultLayout(normalized: DashboardLayoutV1): DashboardLayoutV1 {
  return {
    version: 1,
    widgets: mergeEntries(normalized.widgets, DEFAULT_DASHBOARD_LAYOUT.widgets),
    metric_cards: mergeEntries(normalized.metric_cards, DEFAULT_DASHBOARD_METRIC_CARDS),
  };
}
