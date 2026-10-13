//! Computation jobs: persisted lifecycle (pending -> running -> progress -> completed / failed /
//! cancelled), listing, and the reliability KPI refresh payload the runner stores.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::jobs::domain::JOB_KIND_RELIABILITY_KPI_REFRESH;
use crate::jobs::queries::{
    complete_job_cancelled, complete_job_failed, complete_job_success, get_computation_job, insert_computation_job,
    list_computation_jobs, update_job_progress, update_job_running,
};
use crate::jobs::ComputationJob;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::connect_in_memory().await.expect("in-memory SQLite");
    db.execute_unprepared("PRAGMA foreign_keys = ON")
        .await
        .expect("foreign keys");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seeder");
    db
}

async fn job(db: &DatabaseConnection, id: i64) -> ComputationJob {
    get_computation_job(db, id)
        .await
        .expect("get job")
        .unwrap_or_else(|| panic!("job {id} missing"))
}

fn is_utc_seconds(ts: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(ts).is_ok() && ts.ends_with('Z') && !ts.contains('.')
}

#[tokio::test]
async fn inserted_job_is_pending_with_identity_and_input() {
    let db = setup().await;
    let id = insert_computation_job(&db, "custom_kind", r#"{"a":1}"#)
        .await
        .expect("insert");
    let j = job(&db, id).await;
    assert_eq!(j.id, id);
    assert_eq!(j.job_kind, "custom_kind");
    assert_eq!(j.status, "pending");
    assert_eq!(j.progress_pct, 0.0);
    assert_eq!(j.input_json, r#"{"a":1}"#);
    assert_eq!(j.row_version, 1);
    assert_eq!((j.result_json, j.error_message), (None, None));
    assert_eq!((j.started_at, j.finished_at), (None, None));
    assert!(is_utc_seconds(&j.created_at), "{}", j.created_at);
    let uuid = j
        .entity_sync_id
        .strip_prefix("computation_job:")
        .expect("computation_job: prefix");
    uuid::Uuid::parse_str(uuid).expect("uuid suffix");

    let other = insert_computation_job(&db, "custom_kind", "{}")
        .await
        .expect("insert 2");
    assert_ne!(other, id);
    assert_ne!(job(&db, other).await.entity_sync_id, j.entity_sync_id);
}

#[tokio::test]
async fn job_runs_reports_progress_and_completes() {
    let db = setup().await;
    let id = insert_computation_job(&db, JOB_KIND_RELIABILITY_KPI_REFRESH, "{}")
        .await
        .expect("insert");

    update_job_running(&db, id).await.expect("running");
    let j = job(&db, id).await;
    assert_eq!((j.status.as_str(), j.progress_pct, j.row_version), ("running", 5.0, 2));
    assert!(is_utc_seconds(j.started_at.as_deref().expect("started_at")));
    assert_eq!(j.finished_at, None);

    update_job_progress(&db, id, 42.5).await.expect("progress");
    let j = job(&db, id).await;
    assert_eq!((j.progress_pct, j.row_version), (42.5, 3));

    update_job_progress(&db, id, 250.0).await.expect("progress above range");
    assert_eq!(job(&db, id).await.progress_pct, 100.0, "clamped to 100");
    update_job_progress(&db, id, -10.0).await.expect("progress below range");
    assert_eq!(job(&db, id).await.progress_pct, 0.0, "clamped to 0");

    complete_job_success(&db, id, r#"{"ok":true}"#).await.expect("complete");
    let j = job(&db, id).await;
    assert_eq!(j.status, "completed");
    assert_eq!(j.progress_pct, 100.0);
    assert_eq!(j.result_json.as_deref(), Some(r#"{"ok":true}"#));
    assert_eq!(j.error_message, None);
    assert!(is_utc_seconds(j.finished_at.as_deref().expect("finished_at")));
    assert_eq!(j.row_version, 6);
    assert!(j.started_at.is_some(), "started_at is kept");
}

#[tokio::test]
async fn job_failure_keeps_progress_and_records_message() {
    let db = setup().await;
    let id = insert_computation_job(&db, "k", "{}").await.expect("insert");
    update_job_running(&db, id).await.expect("running");
    update_job_progress(&db, id, 55.0).await.expect("progress");
    complete_job_failed(&db, id, "equipment 9 not found")
        .await
        .expect("fail");
    let j = job(&db, id).await;
    assert_eq!(j.status, "failed");
    assert_eq!(j.error_message.as_deref(), Some("equipment 9 not found"));
    assert_eq!(j.progress_pct, 55.0, "progress at the time of failure");
    assert_eq!(j.result_json, None);
    assert!(j.finished_at.is_some());
    assert_eq!(j.row_version, 4);
}

#[tokio::test]
async fn job_cancellation_is_recorded() {
    let db = setup().await;
    let id = insert_computation_job(&db, "k", "{}").await.expect("insert");
    complete_job_cancelled(&db, id).await.expect("cancel");
    let j = job(&db, id).await;
    assert_eq!(j.status, "cancelled");
    assert_eq!(j.error_message.as_deref(), Some("cancelled"));
    assert_eq!(j.started_at, None, "cancelled before it started");
    assert!(j.finished_at.is_some());
    assert_eq!(j.row_version, 2);
}

#[tokio::test]
async fn success_clears_a_previous_error_message() {
    let db = setup().await;
    let id = insert_computation_job(&db, "k", "{}").await.expect("insert");
    complete_job_failed(&db, id, "transient").await.expect("fail");
    complete_job_success(&db, id, "{}").await.expect("retry succeeds");
    let j = job(&db, id).await;
    assert_eq!((j.status.as_str(), j.error_message), ("completed", None));
}

#[tokio::test]
async fn get_missing_job_is_none_and_updates_on_it_touch_nothing() {
    let db = setup().await;
    assert!(get_computation_job(&db, 9_999).await.expect("get").is_none());
    let id = insert_computation_job(&db, "k", "{}").await.expect("insert");
    update_job_running(&db, id + 1).await.expect("no row is not an error");
    complete_job_failed(&db, id + 1, "x")
        .await
        .expect("no row is not an error");
    let j = job(&db, id).await;
    assert_eq!((j.status.as_str(), j.row_version), ("pending", 1));
    assert!(get_computation_job(&db, id + 1).await.expect("get").is_none());
}

#[tokio::test]
async fn list_returns_newest_first_and_clamps_limit() {
    let db = setup().await;
    assert!(list_computation_jobs(&db, 10).await.expect("list").is_empty());
    let mut ids = Vec::new();
    for n in 0..5 {
        ids.push(
            insert_computation_job(&db, &format!("kind_{n}"), "{}")
                .await
                .expect("insert"),
        );
    }
    complete_job_failed(&db, ids[1], "boom").await.expect("fail");

    let all = list_computation_jobs(&db, 50).await.expect("list");
    let listed: Vec<i64> = all.iter().map(|j| j.id).collect();
    let mut expected = ids.clone();
    expected.reverse();
    assert_eq!(listed, expected, "ORDER BY id DESC");
    assert_eq!(all[3].status, "failed");
    assert_eq!(all[3].job_kind, "kind_1");

    let two = list_computation_jobs(&db, 2).await.expect("list 2");
    assert_eq!(two.iter().map(|j| j.id).collect::<Vec<_>>(), [ids[4], ids[3]]);
    let clamped_low = list_computation_jobs(&db, 0).await.expect("list 0");
    assert_eq!(clamped_low.len(), 1, "limit below 1 is raised to 1");
    assert_eq!(clamped_low[0].id, ids[4]);
    let clamped_neg = list_computation_jobs(&db, -5).await.expect("list -5");
    assert_eq!(clamped_neg.len(), 1);
}

#[tokio::test]
async fn list_limit_is_capped_at_200() {
    let db = setup().await;
    for _ in 0..205 {
        insert_computation_job(&db, "bulk", "{}").await.expect("insert");
    }
    assert_eq!(list_computation_jobs(&db, 1_000).await.expect("list").len(), 200);
}

#[tokio::test]
async fn runner_default_cancel_of_unknown_job_leaves_rows_untouched() {
    let db = setup().await;
    let id = insert_computation_job(&db, "k", "{}").await.expect("insert");
    let runner = crate::jobs::ComputationJobRunner::default();
    runner.cancel_job(id).await;
    runner.cancel_job(id + 100).await;
    // Cancelling only signals a running task; the stored job is changed by that task, not here.
    assert_eq!(job(&db, id).await.status, "pending");
}

/// The reliability KPI refresh job, step for step as `runner::run_reliability_kpi_job` performs it
/// (the runner itself needs a Tauri `AppHandle` for its progress events, which tests cannot build
/// without the `tauri/test` feature): the stored input is the request and the stored result is the
/// snapshot actually written.
#[tokio::test]
async fn reliability_kpi_refresh_job_stores_input_and_snapshot_result() {
    let db = setup().await;
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, maintainable_boundary, \
         rams_utilization_factor, created_at, updated_at, row_version) \
         VALUES ('jobs-eq', 'JOB-EQ', 'Job asset', 'IN_SERVICE', 1, 1.0, ?, ?, 1)",
        [now.clone().into(), now.into()],
    ))
    .await
    .expect("equipment");
    let eq: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = 'JOB-EQ'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "id")
        .expect("id");

    let end = chrono::Utc::now();
    let input = RefreshReliabilityKpiSnapshotInput {
        equipment_id: eq,
        period_start: (end - chrono::Duration::days(30)).to_rfc3339(),
        period_end: end.to_rfc3339(),
        min_sample_n: Some(1),
        repeat_lookback_days: Some(30),
    };
    let input_json = serde_json::to_string(&input).expect("input json");
    let id = insert_computation_job(&db, JOB_KIND_RELIABILITY_KPI_REFRESH, &input_json)
        .await
        .expect("insert");
    update_job_running(&db, id).await.expect("running");
    update_job_progress(&db, id, 25.0).await.expect("25");
    update_job_progress(&db, id, 55.0).await.expect("55");
    let stored: RefreshReliabilityKpiSnapshotInput =
        serde_json::from_str(&job(&db, id).await.input_json).expect("stored input parses");
    assert_eq!(stored.equipment_id, eq);
    assert_eq!(stored.period_end, input.period_end);

    let snap = crate::reliability::queries::refresh_reliability_kpi_snapshot(&db, stored)
        .await
        .expect("refresh");
    complete_job_success(&db, id, &serde_json::to_string(&snap).expect("snap json"))
        .await
        .expect("complete");

    let j = job(&db, id).await;
    assert_eq!(j.job_kind, JOB_KIND_RELIABILITY_KPI_REFRESH);
    assert_eq!(
        (j.status.as_str(), j.progress_pct, j.row_version),
        ("completed", 100.0, 5)
    );
    let result: serde_json::Value = serde_json::from_str(j.result_json.as_deref().expect("result")).expect("json");
    assert_eq!(result["id"].as_i64(), Some(snap.id));
    assert_eq!(result["equipment_id"].as_i64(), Some(eq));
    let rows: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM reliability_kpi_snapshots WHERE id = ? AND equipment_id = ?",
            [snap.id.into(), eq.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert_eq!(rows, 1, "the job result points at the persisted snapshot");
}
