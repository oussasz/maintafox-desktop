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

export const RELIABILITY_RAM_EVIDENCE_HASH = "#ram-data-quality";
