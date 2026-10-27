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
  /**
   * Change below which the trend reads "stable": percentage points for a `percent` KPI, percent of
   * the previous value for the others. Shown with the KPI definition; default `DEFAULT_KPI_STABLE_BELOW`.
   */
  stableBelow?: number;
}

/** Default "stable" threshold (see `OverviewKpiRegistryEntry.stableBelow`). */
export const DEFAULT_KPI_STABLE_BELOW = 0.5;

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
    stableBelow: 0.5,
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
    // D15: a rising load of PM due today is not bad news; only "en retard" is red.
    trendSense: "neutral",
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
    stableBelow: 0.1,
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
    stableBelow: 0.5,
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
];

export const OVERVIEW_KPI_REGISTRY_BY_KEY = new Map(
  OVERVIEW_KPI_REGISTRY.map((e) => [e.semanticKey, e]),
);

/**
 * SMRP indicators of the Analytics hub that move in percentage points. Each carries its own
 * "stable" band here, next to the Overview entries: this registry is the only place a band is
 * written (D14). PM compliance is the Overview `PM_COMPLIANCE` entry (same figure, same band).
 */
export const SMRP_KPI_KEYS = {
  SCHEDULE_COMPLIANCE: "SCHEDULE_COMPLIANCE",
  WORK_MIX_PLANNED: "WORK_MIX_PLANNED",
  WORK_MIX_REACTIVE: "WORK_MIX_REACTIVE",
  WORK_MIX_EMERGENCY: "WORK_MIX_EMERGENCY",
  COST_TO_RAV: "COST_TO_RAV",
} as const;

export type SmrpKpiKey = (typeof SMRP_KPI_KEYS)[keyof typeof SMRP_KPI_KEYS];

export interface SmrpKpiRegistryEntry {
  key: SmrpKpiKey;
  catalogId: string;
  trendSense: OverviewKpiTrendSense;
  /** Change, in percentage points, below which the trend reads "stable". */
  stableBelow: number;
}

export const SMRP_KPI_REGISTRY: SmrpKpiRegistryEntry[] = [
  {
    key: SMRP_KPI_KEYS.SCHEDULE_COMPLIANCE,
    catalogId: "KPI-052",
    trendSense: "up-good",
    stableBelow: 0.5,
  },
  {
    key: SMRP_KPI_KEYS.WORK_MIX_PLANNED,
    catalogId: "KPI-053",
    trendSense: "up-good",
    stableBelow: 0.5,
  },
  {
    key: SMRP_KPI_KEYS.WORK_MIX_REACTIVE,
    catalogId: "KPI-054",
    trendSense: "up-bad",
    stableBelow: 0.5,
  },
  {
    key: SMRP_KPI_KEYS.WORK_MIX_EMERGENCY,
    catalogId: "KPI-055",
    trendSense: "up-bad",
    stableBelow: 0.5,
  },
  {
    // A small ratio (a few percent a year): it moves by hundredths of a point.
    key: SMRP_KPI_KEYS.COST_TO_RAV,
    catalogId: "KPI-056",
    trendSense: "up-bad",
    stableBelow: 0.05,
  },
];

export const SMRP_KPI_REGISTRY_BY_KEY = new Map(SMRP_KPI_REGISTRY.map((e) => [e.key, e]));

/** The "stable" band of an SMRP indicator, in percentage points. */
export function smrpStableBelow(key: SmrpKpiKey): number {
  return SMRP_KPI_REGISTRY_BY_KEY.get(key)?.stableBelow ?? DEFAULT_KPI_STABLE_BELOW;
}

/** The trend sense of an SMRP indicator (registry only; screens never carry a copy). */
export function smrpTrendSense(key: SmrpKpiKey): OverviewKpiTrendSense {
  return SMRP_KPI_REGISTRY_BY_KEY.get(key)?.trendSense ?? "neutral";
}

export function isOverviewKpiSemanticKey(id: string): id is OverviewKpiSemanticKey {
  return Object.values(OVERVIEW_KPI_SEMANTIC_KEYS).includes(id as OverviewKpiSemanticKey);
}
