//! Canonical KPI keys and quality badge codes (dashboard + reliability snapshots).

pub mod dashboard {
    pub const OPEN_DIS: &str = "open_dis";
    pub const OPEN_WOS: &str = "open_wos";
    pub const TOTAL_ASSETS: &str = "total_assets";
    pub const OVERDUE_ITEMS: &str = "overdue_items";
}

pub mod quality_badge {
    pub const INSUFFICIENT_BASELINE: &str = "insufficient_baseline";
    pub const SPARSE_WORKLOAD: &str = "sparse_workload";
}

pub mod reliability_snapshot {
    pub const MTBF: &str = "mtbf";
    pub const MTTR: &str = "mttr";
    /// KPI-035 operational availability A_o (alias of `availability_operational`).
    pub const AVAILABILITY: &str = "availability";
    pub const AVAILABILITY_OPERATIONAL: &str = "availability_operational";
    pub const AVAILABILITY_INHERENT: &str = "availability_inherent";
    /// Non-repairable equipment only (`equipment.rams_is_repairable = 0`).
    pub const MTTF: &str = "mttf";
    pub const FAILURE_RATE: &str = "failure_rate";
    pub const REPEAT_FAILURE_RATE: &str = "repeat_failure_rate";
    pub const EVENT_COUNT: &str = "event_count";
    pub const DATA_QUALITY_SCORE: &str = "data_quality_score";
}

/// Formal catalog ids for Analytics hub KPIs (`docs/engineering/KPI_CATALOG.md`).
pub mod catalog {
    pub const MTBF: &str = "KPI-033";
    pub const MTTR: &str = "KPI-034";
    pub const AVAILABILITY: &str = "KPI-035";
    pub const PM_ADHERENCE: &str = "KPI-036";
    pub const MAINTENANCE_COST: &str = "KPI-037";
    pub const DI_OPEN: &str = "KPI-038";
    pub const OT_IN_PROGRESS: &str = "KPI-039";
    pub const OT_OVERDUE: &str = "KPI-040";
    pub const PM_DUE_TODAY: &str = "KPI-041";
    pub const MAINT_BACKLOG: &str = "KPI-042";
    pub const CRITICAL_DOWN: &str = "KPI-043";
    pub const DATA_QUALITY: &str = "KPI-044";
    pub const STOCK_OUT: &str = "KPI-045";
    pub const REORDER_ALERT: &str = "KPI-046";
    pub const EQUIP_STATUS_SUMMARY: &str = "KPI-047";
    pub const EQUIP_CRITICAL_AT_RISK: &str = "KPI-048";
    pub const EQUIP_HEALTH_SCORE: &str = "KPI-049";
    pub const EQUIP_OPEN_WORK: &str = "KPI-050";
    pub const EQUIP_STALE_METERS: &str = "KPI-051";
}

/// Semantic keys for Vue d'ensemble Overview strip.
pub mod overview {
    pub const DI_OPEN: &str = "DI_OPEN";
    pub const OT_IN_PROGRESS: &str = "OT_IN_PROGRESS";
    pub const OT_OVERDUE: &str = "OT_OVERDUE";
    pub const MTTR: &str = "MTTR";
    pub const PM_COMPLIANCE: &str = "PM_COMPLIANCE";
    pub const PM_DUE_TODAY: &str = "PM_DUE_TODAY";
    pub const MAINT_BACKLOG: &str = "MAINT_BACKLOG";
    pub const AVAILABILITY: &str = "AVAILABILITY";
    pub const MTBF: &str = "MTBF";
    pub const CRITICAL_DOWN: &str = "CRITICAL_DOWN";
    pub const DATA_QUALITY: &str = "DATA_QUALITY";
    pub const STOCK_OUT: &str = "STOCK_OUT";
    pub const REORDER_ALERT: &str = "REORDER_ALERT";
    pub const TOTAL_COST: &str = "TOTAL_COST";
}

/// Semantic keys for Equipment registry strip.
pub mod equipment {
    pub const EQUIP_STATUS_SUMMARY: &str = "EQUIP_STATUS_SUMMARY";
    pub const EQUIP_CRITICAL_AT_RISK: &str = "EQUIP_CRITICAL_AT_RISK";
    pub const EQUIP_HEALTH_SCORE: &str = "EQUIP_HEALTH_SCORE";
    pub const EQUIP_OPEN_WORK: &str = "EQUIP_OPEN_WORK";
    pub const EQUIP_STALE_METERS: &str = "EQUIP_STALE_METERS";
}
