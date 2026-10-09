/** Mirrors `src-tauri/src/kpi_definitions.rs` — canonical keys for dashboard + reliability KPIs. */

export const DASHBOARD_KPI_KEYS = {
  OPEN_DIS: "open_dis",
  OPEN_WOS: "open_wos",
  TOTAL_ASSETS: "total_assets",
  OVERDUE_ITEMS: "overdue_items",
} as const;

export type DashboardKpiKey = (typeof DASHBOARD_KPI_KEYS)[keyof typeof DASHBOARD_KPI_KEYS];

export const WORKLOAD_CHART_ID = "workload_chart";

export const QUALITY_BADGE_CODES = {
  INSUFFICIENT_BASELINE: "insufficient_baseline",
  SPARSE_WORKLOAD: "sparse_workload",
} as const;

export type QualityBadgeCode = (typeof QUALITY_BADGE_CODES)[keyof typeof QUALITY_BADGE_CODES];

/** Default drill targets for dashboard KPI evidence (paths under app router). */
export const DASHBOARD_KPI_DRILL_PATH: Record<DashboardKpiKey, string> = {
  [DASHBOARD_KPI_KEYS.OPEN_DIS]: "/requests",
  [DASHBOARD_KPI_KEYS.OPEN_WOS]: "/work-orders",
  [DASHBOARD_KPI_KEYS.TOTAL_ASSETS]: "/equipment",
  [DASHBOARD_KPI_KEYS.OVERDUE_ITEMS]: "/planning",
};

export const RELIABILITY_KPI_SNAPSHOT_FIELDS = {
  MTBF: "mtbf",
  MTTR: "mttr",
  AVAILABILITY: "availability",
  FAILURE_RATE: "failure_rate",
  REPEAT_FAILURE_RATE: "repeat_failure_rate",
  EVENT_COUNT: "event_count",
  DATA_QUALITY_SCORE: "data_quality_score",
} as const;

/** Formal catalog ids for Analytics hub KPIs (`docs/engineering/KPI_CATALOG.md`). */
export const ANALYTICS_HUB_KPI_CATALOG_IDS = {
  MTBF: "KPI-033",
  MTTR: "KPI-034",
  AVAILABILITY: "KPI-035",
  PM_ADHERENCE: "KPI-036",
  MAINTENANCE_COST: "KPI-037",
  DI_OPEN: "KPI-038",
  OT_IN_PROGRESS: "KPI-039",
  OT_OVERDUE: "KPI-040",
  PM_DUE_TODAY: "KPI-041",
  MAINT_BACKLOG: "KPI-042",
  CRITICAL_DOWN: "KPI-043",
  DATA_QUALITY: "KPI-044",
  STOCK_OUT: "KPI-045",
  REORDER_ALERT: "KPI-046",
  EQUIP_STATUS_SUMMARY: "KPI-047",
  EQUIP_CRITICAL_AT_RISK: "KPI-048",
  EQUIP_HEALTH_SCORE: "KPI-049",
  EQUIP_OPEN_WORK: "KPI-050",
  EQUIP_STALE_METERS: "KPI-051",
} as const;

/** Re-export Overview semantic keys — SSOT in `shared/overview-kpi-registry.ts`. */
export { OVERVIEW_KPI_SEMANTIC_KEYS } from "@shared/overview-kpi-registry";

/** Re-export Equipment registry semantic keys — SSOT in `shared/equipment-kpi-registry.ts`. */
export {
  EQUIPMENT_KPI_SEMANTIC_KEYS,
  EQUIPMENT_KPI_CATALOG_IDS,
} from "@shared/equipment-kpi-registry";

export const RELIABILITY_RAM_EVIDENCE_HASH = "#ram-data-quality";

/** Canonical Deep RAMS routes under the unified Analytics hub (M0+). */
export const ANALYTICS_RAMS_BASE_PATH = "/analytics/rams";

export const analyticsRamsPath = (segment: string, hash = "") =>
  `${ANALYTICS_RAMS_BASE_PATH}/${segment.replace(/^\//, "")}${hash}`;
