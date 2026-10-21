import {
  OVERVIEW_KPI_SEMANTIC_KEYS,
  isOverviewKpiAccent,
  type OverviewKpiLayoutEntry,
  type OverviewKpiSemanticKey,
} from "@shared/overview-kpi-registry";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

/** Executive dashboard widgets under `/analytics/dashboards` (device-local layout). */

export const ANALYTICS_DASHBOARD_WIDGET_IDS = {
  OPS_KPIS: "ops_kpis",
  PM_DUE: "pm_due",
  RELIABILITY_RAMS: "reliability_rams",
  MAINTENANCE_COST: "maintenance_cost",
  WORKLOAD: "workload",
  DI_STATUS: "di_status",
} as const;

export type AnalyticsDashboardWidgetId =
  (typeof ANALYTICS_DASHBOARD_WIDGET_IDS)[keyof typeof ANALYTICS_DASHBOARD_WIDGET_IDS];

export interface AnalyticsDashboardWidgetLayoutEntry {
  id: AnalyticsDashboardWidgetId | string;
  order: number;
  visible: boolean;
}

export interface AnalyticsDashboardLayoutV1 {
  version: 1;
  widgets: AnalyticsDashboardWidgetLayoutEntry[];
  overview_kpis?: OverviewKpiLayoutEntry[];
}

export type { OverviewKpiLayoutEntry };

export const DEFAULT_ANALYTICS_DASHBOARD_LAYOUT: AnalyticsDashboardLayoutV1 = {
  version: 1,
  widgets: [
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS, order: 0, visible: true },
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE, order: 1, visible: true },
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS, order: 2, visible: true },
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST, order: 3, visible: true },
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD, order: 4, visible: true },
    { id: ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS, order: 5, visible: true },
  ],
};

/** Permission required to show each widget (data is also gated server-side). */
export const ANALYTICS_DASHBOARD_WIDGET_PERMISSION: Partial<Record<string, PermissionName>> = {
  [ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS]: P.DI_VIEW,
  [ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE]: P.PM_VIEW,
  [ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS]: P.REP_VIEW,
  [ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST]: P.OT_VIEW,
  [ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD]: P.DI_VIEW,
  [ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS]: P.DI_VIEW,
};

export function parseAnalyticsDashboardLayoutJson(raw: string): AnalyticsDashboardLayoutV1 {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return DEFAULT_ANALYTICS_DASHBOARD_LAYOUT;
  }
  return normalizeAnalyticsDashboardLayout(parsed);
}

export function normalizeAnalyticsDashboardLayout(input: unknown): AnalyticsDashboardLayoutV1 {
  if (!input || typeof input !== "object") {
    return DEFAULT_ANALYTICS_DASHBOARD_LAYOUT;
  }
  const o = input as Record<string, unknown>;
  if (o["version"] !== 1 || !Array.isArray(o["widgets"])) {
    return DEFAULT_ANALYTICS_DASHBOARD_LAYOUT;
  }
  const seen = new Set<string>();
  const widgets: AnalyticsDashboardWidgetLayoutEntry[] = [];
  for (const w of o["widgets"] as unknown[]) {
    if (!w || typeof w !== "object") continue;
    const e = w as Record<string, unknown>;
    const id = typeof e["id"] === "string" ? e["id"] : "";
    if (!id || id.length > 64 || seen.has(id)) continue;
    seen.add(id);
    const order =
      typeof e["order"] === "number" && Number.isFinite(e["order"])
        ? (e["order"] as number)
        : widgets.length;
    const visible = typeof e["visible"] === "boolean" ? e["visible"] : true;
    widgets.push({ id, order, visible });
  }
  if (widgets.length === 0) {
    return DEFAULT_ANALYTICS_DASHBOARD_LAYOUT;
  }
  widgets.sort((a, b) => a.order - b.order);
  const overview_kpis = parseOverviewKpisArray(o["overview_kpis"]);
  const out: AnalyticsDashboardLayoutV1 = { version: 1, widgets };
  if (overview_kpis) {
    out.overview_kpis = overview_kpis;
  }
  return out;
}

function parseOverviewKpisArray(raw: unknown): OverviewKpiLayoutEntry[] | undefined {
  if (!Array.isArray(raw)) {
    return undefined;
  }
  const seen = new Set<string>();
  const entries: OverviewKpiLayoutEntry[] = [];
  for (const w of raw) {
    if (!w || typeof w !== "object") continue;
    const e = w as Record<string, unknown>;
    const id = typeof e["id"] === "string" ? e["id"] : "";
    if (!id || id.length > 64 || seen.has(id)) continue;
    seen.add(id);
    const order =
      typeof e["order"] === "number" && Number.isFinite(e["order"])
        ? (e["order"] as number)
        : entries.length;
    const visible = typeof e["visible"] === "boolean" ? e["visible"] : true;
    const entry: OverviewKpiLayoutEntry = { id, order, visible };
    if (isOverviewKpiAccent(e["accent"])) {
      entry.accent = e["accent"];
    }
    entries.push(entry);
  }
  if (entries.length === 0) {
    return undefined;
  }
  entries.sort((a, b) => a.order - b.order);
  return entries;
}

export function mergeWithDefaultAnalyticsLayout(
  normalized: AnalyticsDashboardLayoutV1,
): AnalyticsDashboardLayoutV1 {
  const byId = new Map(normalized.widgets.map((w) => [w.id, w]));
  const merged: AnalyticsDashboardWidgetLayoutEntry[] = [];
  for (const d of DEFAULT_ANALYTICS_DASHBOARD_LAYOUT.widgets) {
    const cur = byId.get(d.id);
    merged.push(cur ? { id: d.id, order: cur.order, visible: cur.visible } : { ...d });
  }
  for (const w of normalized.widgets) {
    if (!DEFAULT_ANALYTICS_DASHBOARD_LAYOUT.widgets.some((x) => x.id === w.id)) {
      merged.push(w);
    }
  }
  merged.sort((a, b) => a.order - b.order);
  const out: AnalyticsDashboardLayoutV1 = { version: 1, widgets: merged };
  if (normalized.overview_kpis?.length) {
    out.overview_kpis = normalized.overview_kpis.map((k) => ({ ...k }));
  }
  return out;
}

/**
 * Permission-based role defaults when the user has never saved a layout.
 * Prefer capabilities over role name strings.
 */
export function buildRoleDefaultAnalyticsLayout(opts: {
  canDi: boolean;
  canOt: boolean;
  canPm: boolean;
  canRep: boolean;
  isPlannerHeavy: boolean;
  isSupervisorHeavy: boolean;
}): AnalyticsDashboardLayoutV1 {
  const vis = (id: string, fallback: boolean) => {
    switch (id) {
      case ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS:
        return opts.canDi || opts.canOt;
      case ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE:
        return opts.canPm;
      case ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS:
        return opts.canRep;
      case ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST:
        return opts.canOt;
      case ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD:
        return opts.canDi || opts.canOt || opts.canPm;
      case ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS:
        return opts.canDi;
      default:
        return fallback;
    }
  };

  let order = [
    ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS,
    ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE,
    ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS,
    ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST,
    ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD,
    ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS,
  ];

  if (opts.isPlannerHeavy) {
    order = [
      ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE,
      ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD,
      ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS,
      ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST,
      ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS,
      ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS,
    ];
  } else if (opts.isSupervisorHeavy) {
    order = [
      ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS,
      ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS,
      ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD,
      ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE,
      ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS,
      ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST,
    ];
  }

  return {
    version: 1,
    widgets: order.map((id, i) => ({
      id,
      order: i,
      visible: vis(id, true),
    })),
  };
}

function overviewEntry(
  id: OverviewKpiSemanticKey,
  order: number,
  visible = true,
): OverviewKpiLayoutEntry {
  return { id, order, visible };
}

/**
 * Capability-based Overview KPI defaults when `overview_kpis` is absent from saved layout.
 * Prefer capabilities over role name strings; first matching profile wins.
 */
export function buildRoleDefaultOverviewKpis(opts: {
  canDi: boolean;
  canOt: boolean;
  canPm: boolean;
  canInv: boolean;
  canRam: boolean;
  canRep: boolean;
  isSupervisorHeavy: boolean;
}): OverviewKpiLayoutEntry[] {
  const K = OVERVIEW_KPI_SEMANTIC_KEYS;
  const isReliabilityHeavy = opts.canRam || opts.canRep;
  const isStorekeeperHeavy = opts.canInv && !opts.isSupervisorHeavy && !isReliabilityHeavy;

  let ids: OverviewKpiSemanticKey[];

  if (isReliabilityHeavy) {
    ids = [K.MTBF, K.AVAILABILITY, K.DATA_QUALITY];
  } else if (isStorekeeperHeavy) {
    ids = [K.STOCK_OUT, K.REORDER_ALERT, K.OT_IN_PROGRESS];
  } else if (opts.isSupervisorHeavy) {
    ids = [K.DI_OPEN, K.OT_OVERDUE, K.PM_COMPLIANCE, K.AVAILABILITY];
  } else {
    ids = [K.DI_OPEN, K.OT_IN_PROGRESS, K.OT_OVERDUE, K.PM_DUE_TODAY];
  }

  const permissionFor: Partial<Record<OverviewKpiSemanticKey, PermissionName>> = {
    [K.DI_OPEN]: P.DI_VIEW,
    [K.OT_IN_PROGRESS]: P.OT_VIEW,
    [K.OT_OVERDUE]: P.OT_VIEW,
    [K.PM_COMPLIANCE]: P.PM_VIEW,
    [K.PM_DUE_TODAY]: P.PM_VIEW,
    [K.AVAILABILITY]: P.REP_VIEW,
    [K.MTBF]: P.REP_VIEW,
    [K.DATA_QUALITY]: P.REP_VIEW,
    [K.STOCK_OUT]: P.INV_VIEW,
    [K.REORDER_ALERT]: P.INV_VIEW,
  };

  const filtered = ids.filter((id) => {
    const perm = permissionFor[id];
    if (!perm) return true;
    switch (perm) {
      case P.DI_VIEW:
        return opts.canDi;
      case P.OT_VIEW:
        return opts.canOt;
      case P.PM_VIEW:
        return opts.canPm;
      case P.INV_VIEW:
        return opts.canInv;
      case P.REP_VIEW:
        return opts.canRep;
      default:
        return true;
    }
  });

  return filtered.map((id, order) => overviewEntry(id, order));
}

export function resolveOverviewKpiLayout(
  layout: AnalyticsDashboardLayoutV1,
  roleDefaults: OverviewKpiLayoutEntry[],
): OverviewKpiLayoutEntry[] {
  if (layout.overview_kpis?.length) {
    return [...layout.overview_kpis].sort((a, b) => a.order - b.order);
  }
  return roleDefaults;
}
