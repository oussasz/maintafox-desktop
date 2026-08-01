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
    pub const AVAILABILITY: &str = "availability";
    pub const FAILURE_RATE: &str = "failure_rate";
    pub const REPEAT_FAILURE_RATE: &str = "repeat_failure_rate";
    pub const EVENT_COUNT: &str = "event_count";
    pub const DATA_QUALITY_SCORE: &str = "data_quality_score";
}
