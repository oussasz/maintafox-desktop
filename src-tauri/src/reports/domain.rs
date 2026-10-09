use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportTemplate {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub description: String,
    pub default_format: String,
    pub spec_json: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSchedule {
    pub id: i64,
    pub user_id: i64,
    pub template_id: i64,
    pub cron_expr: String,
    pub export_format: String,
    pub enabled: bool,
    pub next_run_at: String,
    pub last_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRun {
    pub id: i64,
    pub schedule_id: Option<i64>,
    pub template_id: i64,
    pub user_id: i64,
    pub status: String,
    pub export_format: String,
    pub artifact_path: Option<String>,
    pub byte_size: Option<i64>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Present when a durable `report_artifacts` row exists for this run.
    pub has_downloadable_artifact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportArtifact {
    pub id: i64,
    pub run_id: i64,
    pub relative_path: String,
    pub file_name: String,
    pub mime_type: String,
    pub content_sha256: String,
    pub byte_size: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertReportScheduleInput {
    pub id: Option<i64>,
    pub template_id: i64,
    pub cron_expr: String,
    pub export_format: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExportReportInput {
    pub template_code: String,
    pub export_format: String,
}
