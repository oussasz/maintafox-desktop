use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::errors::{AppError, AppResult};
use crate::jobs::domain::{ComputationJobProgressEvent, JOB_KIND_RELIABILITY_KPI_REFRESH};
use crate::jobs::queries;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::queries as reliability_queries;

#[derive(Clone)]
pub struct ComputationJobRunner {
    cancel_tokens: Arc<Mutex<HashMap<i64, CancellationToken>>>,
}

impl std::fmt::Debug for ComputationJobRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputationJobRunner").finish_non_exhaustive()
    }
}

impl Default for ComputationJobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputationJobRunner {
    pub fn new() -> Self {
        Self {
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn cancel_job(&self, job_id: i64) {
        if let Some(t) = self.cancel_tokens.lock().await.remove(&job_id) {
            t.cancel();
        }
    }

    fn emit_progress(app: &AppHandle, job_id: i64, status: &str, pct: f64) {
        let _ = app.emit(
            "computation-job-progress",
            ComputationJobProgressEvent {
                job_id,
                status: status.to_string(),
                progress_pct: pct,
            },
        );
    }

    async fn finish_remove_token(&self, job_id: i64) {
        self.cancel_tokens.lock().await.remove(&job_id);
    }

    pub async fn spawn_reliability_kpi_refresh(
        &self,
        db: DatabaseConnection,
        app: AppHandle,
        input: RefreshReliabilityKpiSnapshotInput,
    ) -> AppResult<i64> {
        let input_json = serde_json::to_string(&input)
            .map_err(|e| AppError::ValidationFailed(vec![format!("job input json: {e}")]))?;
        let job_id = queries::insert_computation_job(&db, JOB_KIND_RELIABILITY_KPI_REFRESH, &input_json).await?;
        let token = CancellationToken::new();
        self.cancel_tokens.lock().await.insert(job_id, token.clone());
        let runner = self.clone();
        let db_c = db.clone();
        let app_c = app.clone();
        tokio::spawn(async move {
            run_reliability_kpi_job(db_c, app_c, job_id, token, input, runner).await;
        });
        Ok(job_id)
    }
}

async fn run_reliability_kpi_job(
    db: DatabaseConnection,
    app: AppHandle,
    job_id: i64,
    cancel: CancellationToken,
    input: RefreshReliabilityKpiSnapshotInput,
    runner: ComputationJobRunner,
) {
    if cancel.is_cancelled() {
        let _ = queries::complete_job_cancelled(&db, job_id).await;
        ComputationJobRunner::emit_progress(&app, job_id, "cancelled", 0.0);
        runner.finish_remove_token(job_id).await;
        return;
    }
    if let Err(e) = queries::update_job_running(&db, job_id).await {
        tracing::error!("computation job {job_id} running update: {e}");
        let _ = queries::complete_job_failed(&db, job_id, &e.to_string()).await;
        runner.finish_remove_token(job_id).await;
        return;
    }
    ComputationJobRunner::emit_progress(&app, job_id, "running", 5.0);

    if cancel.is_cancelled() {
        let _ = queries::complete_job_cancelled(&db, job_id).await;
        ComputationJobRunner::emit_progress(&app, job_id, "cancelled", 0.0);
        runner.finish_remove_token(job_id).await;
        return;
    }
    let _ = queries::update_job_progress(&db, job_id, 25.0).await;
    ComputationJobRunner::emit_progress(&app, job_id, "running", 25.0);

    if cancel.is_cancelled() {
        let _ = queries::complete_job_cancelled(&db, job_id).await;
        ComputationJobRunner::emit_progress(&app, job_id, "cancelled", 0.0);
        runner.finish_remove_token(job_id).await;
        return;
    }
    let _ = queries::update_job_progress(&db, job_id, 55.0).await;
    ComputationJobRunner::emit_progress(&app, job_id, "running", 55.0);

    match reliability_queries::refresh_reliability_kpi_snapshot(&db, input).await {
        Ok(snap) => {
            if cancel.is_cancelled() {
                let _ = queries::complete_job_cancelled(&db, job_id).await;
                ComputationJobRunner::emit_progress(&app, job_id, "cancelled", 0.0);
            } else {
                let result_json = match serde_json::to_string(&snap) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = queries::complete_job_failed(&db, job_id, &e.to_string()).await;
                        runner.finish_remove_token(job_id).await;
                        return;
                    }
                };
                if let Err(e) = queries::complete_job_success(&db, job_id, &result_json).await {
                    tracing::error!("computation job {job_id} complete: {e}");
                }
                ComputationJobRunner::emit_progress(&app, job_id, "completed", 100.0);
            }
        }
        Err(e) => {
            let _ = queries::complete_job_failed(&db, job_id, &e.to_string()).await;
            ComputationJobRunner::emit_progress(&app, job_id, "failed", 0.0);
        }
    }
    runner.finish_remove_token(job_id).await;
}
