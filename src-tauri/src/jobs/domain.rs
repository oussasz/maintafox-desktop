use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationJob {
    pub id: i64,
    pub entity_sync_id: String,
    pub job_kind: String,
    pub status: String,
    pub progress_pct: f64,
    pub input_json: String,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationJobProgressEvent {
    pub job_id: i64,
    pub status: String,
    pub progress_pct: f64,
}

pub const JOB_KIND_RELIABILITY_KPI_REFRESH: &str = "reliability_kpi_refresh";
