//! DEMO-ONLY direct SQL RAMS simulation backfill (license-safe: no `rams_injector` source).
//!
//! **Production vs Demo separation**
//! - Never invoke from startup, equipment create, login, onboarding, or migrations.
//! - Only via explicit developer IPC (`seed_rams_sql_demo_data`) or
//!   `rams_presentation_seed::run_explicit_rams_demo_seed`.
//!
//! Populates schedule class, governed WO anchor, failure events, and runtime exposure logs
//! so `infer_exposure_hours` and Weibull/R(t) dashboards work without manual input.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use crate::errors::{AppError, AppResult};
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::queries as reliability_queries;

const DEMO_EXPOSURE_SOURCE: &str = "rams_demo_seed";
const DEMO_FE_PREFIX: &str = "RAMS-DEMO-FE-";
const DEMO_WO_TITLE_PREFIX: &str = "RAMS demo anchor WO for equipment ";
const TARGET_EXPOSURE_HOURS: f64 = 2_400.0;
const DEMO_ELIGIBLE_FLAGS: &str =
    r#"{"eligible_unplanned_mtbf":true,"eligible_for_strict_mtbf":true,"ot_linked":true,"rams_demo_seed":true}"#;
const MIN_ELIGIBLE_FAILURE_EVENTS: i64 = 6;

/// DEMO ONLY — backfill all active equipment that still lacks a working RAMS simulation profile.
/// Explicit developer entry only; never part of the production app flow.
pub async fn ensure_all_equipment_rams_simulation(db: &DatabaseConnection) -> AppResult<()> {
    sql_ensure_beta_benchmarks(db).await?;
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE deleted_at IS NULL ORDER BY id ASC".to_string(),
        ))
        .await?;
    for row in rows {
        let equipment_id: i64 = row.try_get("", "id").map_err(decode)?;
        if let Err(e) = ensure_equipment_rams_simulation(db, equipment_id).await {
            tracing::warn!(
                equipment_id,
                error = %e,
                "rams SQL demo seed failed for equipment"
            );
        }
    }
    Ok(())
}

/// DEMO ONLY — idempotent SQL backfill for one equipment, then refresh KPI + analytics pipeline.
/// Explicit developer entry only; never part of the production app flow.
pub async fn ensure_equipment_rams_simulation(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<()> {
    if !equipment_needs_simulation(db, equipment_id).await? {
        return Ok(());
    }

    tracing::info!(equipment_id, "rams SQL demo seed: backfilling simulation data");

    sql_ensure_default_schedule_class(db).await?;
    sql_assign_schedule_class(db, equipment_id).await?;
    let anchor_wo_id = sql_ensure_anchor_work_order(db, equipment_id).await?;
    sql_promote_failure_event_eligibility(db, equipment_id).await?;
    sql_ensure_demo_failure_events(db, equipment_id).await?;
    sql_ensure_demo_exposure_logs(db, equipment_id).await?;

    let actor_id = resolve_actor_id(db).await?;
    let now = Utc::now();
    let period_end = now.to_rfc3339();
    let period_start = (now - Duration::days(365)).to_rfc3339();

    reliability_queries::refresh_reliability_kpi_snapshot(
        db,
        RefreshReliabilityKpiSnapshotInput {
            equipment_id,
            period_start: period_start.clone(),
            period_end: period_end.clone(),
            min_sample_n: Some(5),
            repeat_lookback_days: Some(30),
        },
    )
    .await?;

    crate::db::rams_presentation_seed::refresh_rams_analytics_for_equipment(
        db,
        equipment_id,
        actor_id,
        12,
    )
    .await?;

    let ev = reliability_queries::evaluate_reliability_analysis_input(
        db,
        RefreshReliabilityKpiSnapshotInput {
            equipment_id,
            period_start,
            period_end,
            min_sample_n: Some(1),
            repeat_lookback_days: Some(30),
        },
    )
    .await?;

    tracing::info!(
        equipment_id,
        exposure_hours = ev.exposure_hours,
        eligible_events = ev.eligible_event_count,
        "rams SQL demo seed complete"
    );
    Ok(())
}

async fn equipment_needs_simulation(db: &DatabaseConnection, equipment_id: i64) -> AppResult<bool> {
    let now = Utc::now();
    let period_end = now.to_rfc3339();
    let period_start = (now - Duration::days(365)).to_rfc3339();
    let live_exp = reliability_queries::evaluate_reliability_analysis_input(
        db,
        RefreshReliabilityKpiSnapshotInput {
            equipment_id,
            period_start: period_start.clone(),
            period_end: period_end.clone(),
            min_sample_n: Some(1),
            repeat_lookback_days: Some(30),
        },
    )
    .await
    .map(|ev| ev.exposure_hours)
    .unwrap_or(0.0);
    if live_exp <= 0.0 {
        return Ok(true);
    }
    if count_kpi_eligible_failure_events(db, equipment_id, &period_start, &period_end).await?
        < MIN_ELIGIBLE_FAILURE_EVENTS
    {
        return Ok(true);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rams_schedule_reference_value_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let schedule: Option<i64> = row
        .try_get("", "rams_schedule_reference_value_id")
        .map_err(decode)?;

    let closed_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_orders
             WHERE equipment_id = ? AND closed_at IS NOT NULL
               AND closed_at <= strftime('%Y-%m-%dT%H:%M:%SZ','now')
               AND code NOT LIKE 'RAMS-INJECT%'
               AND COALESCE(description, '') NOT LIKE '%[rams_injector]%'",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("closed wo count missing".into()))?;
    let closed_cnt: i64 = closed_row.try_get("", "c").map_err(decode)?;

    let fe_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM failure_events WHERE equipment_id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("failure event count missing".into()))?;
    let fe_cnt: i64 = fe_row.try_get("", "c").map_err(decode)?;

    let year_ago = period_start;
    let exp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(value), 0) AS t FROM runtime_exposure_logs
             WHERE equipment_id = ? AND exposure_type = 'hours'
               AND source_type = ?
               AND recorded_at >= ? AND recorded_at <= ?",
            [
                equipment_id.into(),
                DEMO_EXPOSURE_SOURCE.into(),
                year_ago.into(),
                period_end.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("exposure sum missing".into()))?;
    let demo_exp: f64 = exp_row.try_get("", "t").map_err(decode)?;

    Ok(schedule.is_none()
        || closed_cnt == 0
        || fe_cnt < 6
        || demo_exp < 500.0)
}

async fn sql_ensure_beta_benchmarks(db: &DatabaseConnection) -> AppResult<()> {
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reliability_beta_benchmarks (
            equipment_class_code, standard_code, beta_reference, source_document, revision_tag,
            is_active, created_at, updated_at
         ) VALUES (
            'RAMS_CLASS', 'IEC-60300', 3.0,
            'OREDA / IEC 60300-3-1 wear-out phase reference', 'demo',
            1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )".to_string(),
    ))
    .await?;
    Ok(())
}

/// Ensures at least one active ORG.SCHEDULE_CLASS value exists (catalog seed may be absent on some DBs).
async fn sql_ensure_default_schedule_class(db: &DatabaseConnection) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id AS id \
             FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.is_active = 1 \
             ORDER BY rv.id ASC LIMIT 1"
                .to_string(),
        ))
        .await?;
    if row.is_some() {
        return Ok(());
    }

    // Domain/set should already exist via system catalog; insert DAY_SHIFT baseline if missing.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
           (set_id, parent_id, code, label, description, sort_order, \
            color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, 'DAY_SHIFT', 'Journée normale', NULL, 1, \
                NULL, NULL, 'schedule_class', NULL, 1, \
                '{\"shift_pattern_code\":\"DAY_SHIFT\",\"is_continuous\":false,\"nominal_hours_per_day\":8.0}' \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
          ORDER BY rs.version_no DESC \
          LIMIT 1"
            .to_string(),
    ))
    .await?;

    let sc_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id AS id \
             FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
               AND UPPER(TRIM(rv.code)) = 'DAY_SHIFT' \
             ORDER BY rv.id ASC LIMIT 1"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "Failed to create ORG.SCHEDULE_CLASS DAY_SHIFT reference value.".into(),
            ])
        })?;
    let sc_id: i64 = sc_row.try_get("", "id").map_err(decode)?;

    for (day, rest) in [(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 1), (7, 1)] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day)
             VALUES (?, ?, '08:00', '16:00', ?)",
            [sc_id.into(), day.into(), rest.into()],
        ))
        .await?;
    }
    tracing::info!(
        schedule_reference_value_id = sc_id,
        "rams SQL demo seed: ensured default DAY_SHIFT ORG.SCHEDULE_CLASS value"
    );
    Ok(())
}

async fn sql_assign_schedule_class(db: &DatabaseConnection, equipment_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment
         SET rams_schedule_reference_value_id = (
               SELECT rv.id
                 FROM reference_values rv
                 JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published'
                 JOIN reference_domains d ON d.id = rs.domain_id
                WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.is_active = 1
                ORDER BY CASE WHEN UPPER(TRIM(rv.code)) = 'DAY_SHIFT' THEN 0 ELSE 1 END, rv.id ASC
                LIMIT 1
             ),
             rams_utilization_factor = 1.0,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND deleted_at IS NULL
           AND (rams_schedule_reference_value_id IS NULL OR rams_utilization_factor IS NULL OR rams_utilization_factor <= 0)",
        [equipment_id.into()],
    ))
    .await?;
    Ok(())
}

async fn sql_ensure_anchor_work_order(db: &DatabaseConnection, equipment_id: i64) -> AppResult<i64> {
    let meta = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: equipment_id.to_string(),
        })?;
    let node_id: Option<i64> = meta.try_get("", "installed_at_node_id").map_err(decode)?;
    let node_id = node_id.ok_or_else(|| {
        AppError::ValidationFailed(vec!["equipment missing installed_at_node_id".into()])
    })?;

    let actor_id = resolve_actor_id(db).await?;
    let closed_status_id = resolve_wo_status_id(db, "closed").await?;
    let corrective_type_id = resolve_wo_type_id(db, "corrective").await?;
    let urgency_id = resolve_urgency_id(db, 3).await?;

    let title = format!("{DEMO_WO_TITLE_PREFIX}{equipment_id}");
    let anchor_closed_at = (Utc::now() - Duration::days(330)).to_rfc3339();
    let now = Utc::now().to_rfc3339();

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE title = ? LIMIT 1",
            [title.clone().into()],
        ))
        .await?;

    let wo_id = if let Some(row) = existing {
        let id: i64 = row.try_get("", "id").map_err(decode)?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders
             SET status_id = ?, closed_at = ?, actual_start = ?, actual_end = ?,
                 equipment_id = ?, entity_id = ?, updated_at = ?
             WHERE id = ?",
            [
                closed_status_id.into(),
                anchor_closed_at.clone().into(),
                anchor_closed_at.clone().into(),
                anchor_closed_at.into(),
                equipment_id.into(),
                node_id.into(),
                now.clone().into(),
                id.into(),
            ],
        ))
        .await?;
        id
    } else {
        let code = crate::wo::domain::generate_wo_code(db).await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO work_orders (
                code, type_id, status_id, equipment_id, entity_id,
                requester_id, planner_id, urgency_id, title, description,
                actual_start, actual_end, closed_at,
                row_version, created_at, updated_at, closeout_validation_passed
              ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, 1)",
            [
                code.into(),
                corrective_type_id.into(),
                closed_status_id.into(),
                equipment_id.into(),
                node_id.into(),
                actor_id.into(),
                actor_id.into(),
                urgency_id.into(),
                title.into(),
                "SQL demo seed — governed exposure anchor.".into(),
                anchor_closed_at.clone().into(),
                anchor_closed_at.clone().into(),
                anchor_closed_at.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
        last_insert_id(db).await?
    };

    let _ = actor_id;
    Ok(wo_id)
}

async fn count_kpi_eligible_failure_events(
    db: &DatabaseConnection,
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM failure_events
             WHERE equipment_id = ?
               AND CAST(json_extract(eligible_flags_json, '$.eligible_unplanned_mtbf') AS INTEGER) = 1
               AND COALESCE(failed_at, detected_at, created_at) >= ?
               AND COALESCE(failed_at, detected_at, created_at) <= ?",
            [
                equipment_id.into(),
                period_start.into(),
                period_end.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("eligible fe count missing".into()))?;
    row.try_get("", "c").map_err(decode)
}

async fn sql_promote_failure_event_eligibility(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE failure_events
         SET eligible_flags_json = ?, verification_status = 'verified',
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE equipment_id = ?
           AND source_type = 'work_order'
           AND CAST(COALESCE(json_extract(eligible_flags_json, '$.eligible_unplanned_mtbf'), 0) AS INTEGER) = 0",
        [DEMO_ELIGIBLE_FLAGS.into(), equipment_id.into()],
    ))
    .await?;
    Ok(())
}

async fn sql_ensure_demo_failure_events(db: &DatabaseConnection, equipment_id: i64) -> AppResult<()> {
    let now = Utc::now();
    let period_end = now.to_rfc3339();
    let period_start = (now - Duration::days(365)).to_rfc3339();
    let eligible = count_kpi_eligible_failure_events(db, equipment_id, &period_start, &period_end).await?;
    if eligible >= MIN_ELIGIBLE_FAILURE_EVENTS {
        return Ok(());
    }

    let now_s = now.to_rfc3339();
    for i in 0..8_i64 {
        let entity_sync_id = format!("{DEMO_FE_PREFIX}{equipment_id}-{i}");
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM failure_events WHERE entity_sync_id = ?",
                [entity_sync_id.clone().into()],
            ))
            .await?;
        if exists.is_some() {
            continue;
        }
        let months_ago = 8 - i;
        let failed_at = (now - Duration::days(months_ago * 30 + 5)).to_rfc3339();
        let restored_at = (now - Duration::days(months_ago * 30 + 4)).to_rfc3339();
        let source_id = equipment_id * 100 + i;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO failure_events (
                entity_sync_id, source_type, source_id, equipment_id,
                detected_at, failed_at, restored_at,
                downtime_duration_hours, active_repair_hours, waiting_hours,
                is_planned, cause_not_determined, verification_status,
                eligible_flags_json, row_version, created_at, updated_at
              ) VALUES (?, 'rams_demo_seed', ?, ?, ?, ?, ?, 4.0, 3.0, 1.0, 0, 0, 'verified', ?, 1, ?, ?)",
            [
                entity_sync_id.into(),
                source_id.into(),
                equipment_id.into(),
                failed_at.clone().into(),
                failed_at.into(),
                restored_at.into(),
                DEMO_ELIGIBLE_FLAGS.into(),
                now_s.clone().into(),
                now_s.clone().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

async fn sql_ensure_demo_exposure_logs(db: &DatabaseConnection, equipment_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM runtime_exposure_logs
             WHERE equipment_id = ? AND source_type = ?",
            [equipment_id.into(), DEMO_EXPOSURE_SOURCE.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("exposure log count missing".into()))?;
    let existing: i64 = row.try_get("", "c").map_err(decode)?;
    if existing >= 12 {
        return Ok(());
    }

    let now = Utc::now();
    let hours_per_month = TARGET_EXPOSURE_HOURS / 12.0;
    for month in 0..12_i64 {
        let entity_sync_id = format!("rams-demo-exp-{equipment_id}-{month}");
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM runtime_exposure_logs WHERE entity_sync_id = ?",
                [entity_sync_id.clone().into()],
            ))
            .await?;
        if exists.is_some() {
            continue;
        }
        let recorded_at = (now - Duration::days((11 - month) * 28)).to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO runtime_exposure_logs (
                entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version
              ) VALUES (?, ?, 'hours', ?, ?, ?, 1)",
            [
                entity_sync_id.into(),
                equipment_id.into(),
                hours_per_month.into(),
                recorded_at.into(),
                DEMO_EXPOSURE_SOURCE.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

async fn resolve_actor_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE is_active = 1 ORDER BY is_admin DESC, id ASC LIMIT 1"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["No active user account.".into()]))?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_wo_status_id(db: &DatabaseConnection, code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("WO status '{code}' missing")]))?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_wo_type_id(db: &DatabaseConnection, code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_types WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("WO type '{code}' missing")]))?;
    row.try_get("", "id").map_err(decode)
}

async fn resolve_urgency_id(db: &DatabaseConnection, level: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM urgency_levels WHERE level = ? LIMIT 1",
            [level.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("urgency level {level} missing")]))?;
    row.try_get("", "id").map_err(decode)
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("last_insert_rowid missing".into()))?;
    row.try_get("", "id").map_err(decode)
}

fn decode(err: sea_orm::DbErr) -> AppError {
    AppError::SyncError(format!("rams_sql_demo_seed decode: {err}"))
}
