import { ANALYTICS_HUB_KPI_CATALOG_IDS, analyticsRamsPath } from "@shared/kpi-definitions";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

/** Semantic keys for Vue d'ensemble Overview strip (`user_analytics_dashboard_layouts.overview_kpis`). */
export const OVERVIEW_KPI_SEMANTIC_KEYS = {
  DI_OPEN: "DI_OPEN",
  OT_IN_PROGRESS: "OT_IN_PROGRESS",
  OT_OVERDUE: "OT_OVERDUE",
  MTTR: "MTTR",
  PM_COMPLIANCE: "PM_COMPLIANCE",
  PM_DUE_TODAY: "PM_DUE_TODAY",
  MAINT_BACKLOG: "MAINT_BACKLOG",
  AVAILABILITY: "AVAILABILITY",
  MTBF: "MTBF",
  CRITICAL_DOWN: "CRITICAL_DOWN",
  DATA_QUALITY: "DATA_QUALITY",
  STOCK_OUT: "STOCK_OUT",
  REORDER_ALERT: "REORDER_ALERT",
  TOTAL_COST: "TOTAL_COST",
} as const;

export type OverviewKpiSemanticKey =
  (typeof OVERVIEW_KPI_SEMANTIC_KEYS)[keyof typeof OVERVIEW_KPI_SEMANTIC_KEYS];

export type OverviewKpiTrendSense = "up-good" | "up-bad" | "neutral";
export type OverviewKpiValueKind = "count" | "percent" | "hours" | "currency";

/** Right-column micro-viz kind for VisualMetricCard. */
export type OverviewKpiVizKind = "areaWave" | "waterfall" | "gauge" | "histogram" | "alert";

export interface OverviewKpiLayoutEntry {
  id: OverviewKpiSemanticKey | string;
  order: number;
  visible: boolean;
  /** Optional accent for VisualMetricCard chrome / micro-viz. */
  accent?: OverviewKpiAccent | undefined;
}

/** User-selectable accent for Overview KPI cards (design-token mapped). */
export const OVERVIEW_KPI_ACCENTS = {
  DEFAULT: "default",
  BRAND: "brand",
  AMBER: "amber",
  DANGER: "danger",
  TEAL: "teal",
} as const;

export type OverviewKpiAccent = (typeof OVERVIEW_KPI_ACCENTS)[keyof typeof OVERVIEW_KPI_ACCENTS];

export const OVERVIEW_KPI_ACCENT_OPTIONS: OverviewKpiAccent[] = [
  OVERVIEW_KPI_ACCENTS.DEFAULT,
  OVERVIEW_KPI_ACCENTS.BRAND,
  OVERVIEW_KPI_ACCENTS.AMBER,
  OVERVIEW_KPI_ACCENTS.TEAL,
  OVERVIEW_KPI_ACCENTS.DANGER,
];

export function isOverviewKpiAccent(value: unknown): value is OverviewKpiAccent {
  return typeof value === "string" && (OVERVIEW_KPI_ACCENT_OPTIONS as string[]).includes(value);
}

export interface OverviewKpiRegistryEntry {
  semanticKey: OverviewKpiSemanticKey;
  catalogId: string;
  permission: PermissionName;
  trendSense: OverviewKpiTrendSense;
  valueKind: OverviewKpiValueKind;
  vizKind: OverviewKpiVizKind;
  drillPath: string;
  labelKey: string;
  contextKey: string;
  unitKey?: string;
}

export const OVERVIEW_KPI_REGISTRY: OverviewKpiRegistryEntry[] = [
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.DI_OPEN,
    catalogId: "KPI-038",
    permission: P.DI_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "histogram",
    drillPath: "/requests",
    labelKey: "overviewKpi.diOpen",
    contextKey: "overviewKpi.contextOpenQueue",
    unitKey: "overviewKpi.unit.di",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.OT_IN_PROGRESS,
    catalogId: "KPI-039",
    permission: P.OT_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "histogram",
    drillPath: "/work-orders",
    labelKey: "overviewKpi.otInProgress",
    contextKey: "overviewKpi.contextExecution",
    unitKey: "overviewKpi.unit.wo",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.OT_OVERDUE,
    catalogId: "KPI-040",
    permission: P.OT_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    drillPath: "/work-orders",
    labelKey: "overviewKpi.otOverdue",
    contextKey: "overviewKpi.contextWoOverdue",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.MTTR,
    catalogId: ANALYTICS_HUB_KPI_CATALOG_IDS.MTTR,
    permission: P.REP_VIEW,
    trendSense: "up-bad",
    valueKind: "hours",
    vizKind: "areaWave",
    drillPath: "/analytics/dashboards",
    labelKey: "overviewKpi.mttr",
    contextKey: "overviewKpi.contextFleet",
    unitKey: "overviewKpi.unit.hours",
  },
  {
    // SMRP PM compliance (%): occurrences done within their tolerance window ÷ occurrences due
    // whose outcome is known, last 30 days — `pm::compliance`, the same figure as the PM page and
    // the Analytics SMRP tile. Higher is better.
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.PM_COMPLIANCE,
    catalogId: ANALYTICS_HUB_KPI_CATALOG_IDS.PM_ADHERENCE,
    permission: P.PM_VIEW,
    trendSense: "up-good",
    valueKind: "percent",
    vizKind: "gauge",
    drillPath: "/pm",
    labelKey: "overviewKpi.pmCompliance",
    contextKey: "overviewKpi.contextPmCompliance",
    unitKey: "overviewKpi.unit.percent",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.PM_DUE_TODAY,
    catalogId: "KPI-041",
    permission: P.PM_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "histogram",
    drillPath: "/pm",
    labelKey: "overviewKpi.pmDueToday",
    contextKey: "overviewKpi.contextPmDue",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.MAINT_BACKLOG,
    catalogId: "KPI-042",
    permission: P.OT_VIEW,
    trendSense: "up-bad",
    valueKind: "hours",
    vizKind: "histogram",
    drillPath: "/planning",
    labelKey: "overviewKpi.maintBacklog",
    contextKey: "overviewKpi.contextBacklog",
    unitKey: "overviewKpi.unit.hours",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.AVAILABILITY,
    catalogId: ANALYTICS_HUB_KPI_CATALOG_IDS.AVAILABILITY,
    permission: P.REP_VIEW,
    trendSense: "up-good",
    valueKind: "percent",
    vizKind: "areaWave",
    drillPath: "/analytics/dashboards",
    labelKey: "overviewKpi.availability",
    contextKey: "overviewKpi.contextFleet",
    unitKey: "overviewKpi.unit.percent",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.MTBF,
    catalogId: ANALYTICS_HUB_KPI_CATALOG_IDS.MTBF,
    permission: P.REP_VIEW,
    trendSense: "up-good",
    valueKind: "hours",
    vizKind: "areaWave",
    drillPath: "/analytics/dashboards",
    labelKey: "overviewKpi.mtbf",
    contextKey: "overviewKpi.contextFleet",
    unitKey: "overviewKpi.unit.hours",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.CRITICAL_DOWN,
    catalogId: "KPI-043",
    permission: P.EQ_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    drillPath: "/equipment",
    labelKey: "overviewKpi.criticalDown",
    contextKey: "overviewKpi.contextCriticalAssets",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.DATA_QUALITY,
    catalogId: "KPI-044",
    permission: P.REP_VIEW,
    trendSense: "up-good",
    valueKind: "percent",
    vizKind: "gauge",
    drillPath: analyticsRamsPath("data-quality"),
    labelKey: "overviewKpi.dataQuality",
    contextKey: "overviewKpi.contextFleet",
    unitKey: "overviewKpi.unit.percent",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.STOCK_OUT,
    catalogId: "KPI-045",
    permission: P.INV_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    drillPath: "/inventory",
    labelKey: "overviewKpi.stockOut",
    contextKey: "overviewKpi.contextInventory",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.REORDER_ALERT,
    catalogId: "KPI-046",
    permission: P.INV_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    drillPath: "/inventory",
    labelKey: "overviewKpi.reorderAlert",
    contextKey: "overviewKpi.contextInventory",
  },
  {
    semanticKey: OVERVIEW_KPI_SEMANTIC_KEYS.TOTAL_COST,
    catalogId: ANALYTICS_HUB_KPI_CATALOG_IDS.MAINTENANCE_COST,
    permission: P.FIN_VIEW,
    trendSense: "up-bad",
    valueKind: "currency",
    vizKind: "waterfall",
    drillPath: "/analytics/dashboards",
    labelKey: "overviewKpi.totalCost",
    contextKey: "overviewKpi.contextCost",
  },
];

export const OVERVIEW_KPI_REGISTRY_BY_KEY = new Map(
  OVERVIEW_KPI_REGISTRY.map((e) => [e.semanticKey, e]),
);

export function isOverviewKpiSemanticKey(id: string): id is OverviewKpiSemanticKey {
  return Object.values(OVERVIEW_KPI_SEMANTIC_KEYS).includes(id as OverviewKpiSemanticKey);
}
