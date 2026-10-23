//! PM generation auto-pilot control row, run audit, and scoped generate helpers.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    GeneratePmOccurrencesInput, GeneratePmOccurrencesResult, PmGenerationControl, PmGenerationRun, PmGenerationStatus,
    UpdatePmGenerationSettingsInput,
};
use crate::pm::queries::generate_pm_occurrences;

fn now_rfc3339() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn next_cron_run_rfc3339(cron_expr: &str) -> String {
    use cron::Schedule;
    use std::str::FromStr;
    let schedule = Schedule::from_str(cron_expr.trim())
        .unwrap_or_else(|_| Schedule::from_str("0 0 0 * * *").expect("default pm cron"));
    schedule
        .upcoming(Utc)
        .next()
        .unwrap_or_else(|| Utc::now() + Duration::hours(1))
        .to_rfc3339()
}

async fn ensure_pm_generation_control(db: &DatabaseConnection) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO pm_generation_control (
            id, enabled, cron_expr, horizon_days, last_run_at, next_run_at,
            last_status, last_generated_count, last_skipped_count, last_error, updated_at
         ) VALUES (1, 1, '0 0 0 * * *', 30, NULL, ?, NULL, NULL, NULL, NULL, ?)",
        [now_rfc3339().into(), now_rfc3339().into()],
    ))
    .await?;
    Ok(())
}

fn decode_pm_generation_control(row: &sea_orm::QueryResult) -> AppResult<PmGenerationControl> {
    let enabled: i64 = row.try_get("", "enabled")?;
    Ok(PmGenerationControl {
        id: row.try_get("", "id")?,
        enabled: enabled != 0,
        cron_expr: row.try_get("", "cron_expr")?,
        horizon_days: row.try_get("", "horizon_days")?,
        last_run_at: row.try_get("", "last_run_at")?,
        next_run_at: row.try_get("", "next_run_at")?,
        last_status: row.try_get("", "last_status")?,
        last_generated_count: row.try_get("", "last_generated_count")?,
        last_skipped_count: row.try_get("", "last_skipped_count")?,
        last_error: row.try_get("", "last_error")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn get_pm_generation_control(db: &DatabaseConnection) -> AppResult<PmGenerationControl> {
    ensure_pm_generation_control(db).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, enabled, cron_expr, horizon_days, last_run_at, next_run_at, last_status,
                    last_generated_count, last_skipped_count, last_error, updated_at
             FROM pm_generation_control WHERE id = 1",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm_generation_control missing")))?;
    decode_pm_generation_control(&row)
}

fn decode_pm_generation_run(row: &sea_orm::QueryResult) -> AppResult<PmGenerationRun> {
    Ok(PmGenerationRun {
        id: row.try_get("", "id")?,
        started_at: row.try_get("", "started_at")?,
        finished_at: row.try_get("", "finished_at")?,
        trigger_source: row.try_get("", "trigger_source")?,
        status: row.try_get("", "status")?,
        generated_count: row.try_get("", "generated_count")?,
        skipped_count: row.try_get("", "skipped_count")?,
        trigger_events_recorded: row.try_get("", "trigger_events_recorded")?,
        error_message: row.try_get("", "error_message")?,
        scope_json: row.try_get("", "scope_json")?,
    })
}

pub async fn get_latest_pm_generation_run(db: &DatabaseConnection) -> AppResult<Option<PmGenerationRun>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, started_at, finished_at, trigger_source, status, generated_count, skipped_count,
                    trigger_events_recorded, error_message, scope_json
             FROM pm_generation_runs
             ORDER BY id DESC LIMIT 1",
            [],
        ))
        .await?;
    row.map(|r| decode_pm_generation_run(&r)).transpose()
}

pub async fn get_pm_generation_status(db: &DatabaseConnection) -> AppResult<PmGenerationStatus> {
    Ok(PmGenerationStatus {
        control: get_pm_generation_control(db).await?,
        latest_run: get_latest_pm_generation_run(db).await?,
    })
}

pub async fn update_pm_generation_settings(
    db: &DatabaseConnection,
    input: UpdatePmGenerationSettingsInput,
) -> AppResult<PmGenerationControl> {
    let current = get_pm_generation_control(db).await?;
    let enabled = input.enabled.unwrap_or(current.enabled);
    let cron_expr = input
        .cron_expr
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or(current.cron_expr);
    let horizon_days = input.horizon_days.unwrap_or(current.horizon_days).clamp(1, 365);
    let next = next_cron_run_rfc3339(&cron_expr);
    let now = now_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_generation_control
         SET enabled = ?, cron_expr = ?, horizon_days = ?, next_run_at = ?, updated_at = ?
         WHERE id = 1",
        [
            i64::from(enabled).into(),
            cron_expr.into(),
            horizon_days.into(),
            next.into(),
            now.into(),
        ],
    ))
    .await?;
    get_pm_generation_control(db).await
}

async fn insert_generation_run_start(
    db: &DatabaseConnection,
    trigger_source: &str,
    scope_json: Option<&str>,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO pm_generation_runs
            (started_at, finished_at, trigger_source, status, generated_count, skipped_count,
             trigger_events_recorded, error_message, scope_json)
         VALUES (?, NULL, ?, 'running', 0, 0, 0, NULL, ?)",
        [
            now_rfc3339().into(),
            trigger_source.to_string().into(),
            scope_json.map(|s| s.to_string()).into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM pm_generation_runs WHERE rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm_generation_runs insert missing")))?;
    Ok(row.try_get("", "id")?)
}

async fn finish_generation_run(
    db: &DatabaseConnection,
    run_id: i64,
    status: &str,
    generated: i64,
    skipped: i64,
    trigger_events: i64,
    error_message: Option<&str>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_generation_runs
         SET finished_at = ?, status = ?, generated_count = ?, skipped_count = ?,
             trigger_events_recorded = ?, error_message = ?
         WHERE id = ?",
        [
            now_rfc3339().into(),
            status.to_string().into(),
            generated.into(),
            skipped.into(),
            trigger_events.into(),
            error_message.map(|s| s.to_string()).into(),
            run_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn update_control_after_run(
    db: &DatabaseConnection,
    status: &str,
    generated: i64,
    skipped: i64,
    error_message: Option<&str>,
    advance_schedule: bool,
) -> AppResult<()> {
    let control = get_pm_generation_control(db).await?;
    let next = if advance_schedule {
        Some(next_cron_run_rfc3339(&control.cron_expr))
    } else {
        control.next_run_at.clone()
    };
    let now = now_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_generation_control
         SET last_run_at = ?, next_run_at = COALESCE(?, next_run_at), last_status = ?,
             last_generated_count = ?, last_skipped_count = ?, last_error = ?, updated_at = ?
         WHERE id = 1",
        [
            now.clone().into(),
            next.into(),
            status.to_string().into(),
            generated.into(),
            skipped.into(),
            error_message.map(|s| s.to_string()).into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn run_generation_with_audit(
    db: &DatabaseConnection,
    input: GeneratePmOccurrencesInput,
    trigger_source: &str,
    advance_schedule: bool,
) -> AppResult<GeneratePmOccurrencesResult> {
    let scope_json = serde_json::json!({
        "pm_plan_id": input.pm_plan_id,
        "equipment_id": input.equipment_id,
        "horizon_days": input.horizon_days,
    })
    .to_string();
    let run_id = insert_generation_run_start(db, trigger_source, Some(&scope_json)).await?;
    match generate_pm_occurrences(db, input).await {
        Ok(result) => {
            let _ = finish_generation_run(
                db,
                run_id,
                "success",
                result.generated_count,
                result.skipped_count,
                result.trigger_events_recorded,
                None,
            )
            .await;
            let _ = update_control_after_run(
                db,
                "success",
                result.generated_count,
                result.skipped_count,
                None,
                advance_schedule,
            )
            .await;
            Ok(result)
        }
        Err(err) => {
            let msg = err.to_string();
            let _ = finish_generation_run(db, run_id, "failed", 0, 0, 0, Some(&msg)).await;
            let _ = update_control_after_run(db, "failed", 0, 0, Some(&msg), advance_schedule).await;
            Err(err)
        }
    }
}

pub async fn generate_pm_for_equipment(
    db: &DatabaseConnection,
    equipment_id: i64,
    horizon_days: Option<i64>,
    trigger_source: &str,
) -> AppResult<GeneratePmOccurrencesResult> {
    let horizon = match get_pm_generation_control(db).await {
        Ok(c) => horizon_days.unwrap_or(c.horizon_days).clamp(1, 365),
        Err(_) => horizon_days.unwrap_or(30).clamp(1, 365),
    };
    run_generation_with_audit(
        db,
        GeneratePmOccurrencesInput {
            as_of: None,
            horizon_days: Some(horizon),
            pm_plan_id: None,
            equipment_id: Some(equipment_id),
            event_codes: None,
            condition_codes: None,
        },
        trigger_source,
        false,
    )
    .await
}

pub async fn generate_pm_for_plan(
    db: &DatabaseConnection,
    pm_plan_id: i64,
    horizon_days: Option<i64>,
    trigger_source: &str,
) -> AppResult<GeneratePmOccurrencesResult> {
    let horizon = match get_pm_generation_control(db).await {
        Ok(c) => horizon_days.unwrap_or(c.horizon_days).clamp(1, 365),
        Err(_) => horizon_days.unwrap_or(30).clamp(1, 365),
    };
    run_generation_with_audit(
        db,
        GeneratePmOccurrencesInput {
            as_of: None,
            horizon_days: Some(horizon),
            pm_plan_id: Some(pm_plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
        trigger_source,
        false,
    )
    .await
}

pub async fn try_generate_pm_for_equipment(db: &DatabaseConnection, equipment_id: i64) {
    if let Err(err) = generate_pm_for_equipment(db, equipment_id, None, "meter").await {
        tracing::warn!(
            error = %err,
            equipment_id,
            "pm::generation meter hook failed"
        );
    }
}

fn is_terminal_occurrence_status(status: &str) -> bool {
    matches!(status, "completed" | "cancelled" | "missed")
}

fn next_status_toward_in_progress(current: &str) -> Option<&'static str> {
    match current {
        "forecasted" => Some("generated"),
        "generated" => Some("ready_for_scheduling"),
        "ready_for_scheduling" => Some("scheduled"),
        "scheduled" => Some("in_progress"),
        "deferred" => Some("ready_for_scheduling"),
        _ => None,
    }
}

async fn advance_occurrence_to_in_progress(
    db: &DatabaseConnection,
    mut occurrence: crate::pm::domain::PmOccurrence,
    actor_id: Option<i64>,
) -> AppResult<crate::pm::domain::PmOccurrence> {
    use crate::pm::domain::TransitionPmOccurrenceInput;
    use crate::pm::queries::transition_pm_occurrence;

    while occurrence.status != "in_progress" {
        let Some(next) = next_status_toward_in_progress(&occurrence.status) else {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_PM_OCCURRENCE_NOT_STARTABLE:status={1}: Cannot advance PM occurrence {} from status '{}' to in_progress.",
                occurrence.id, occurrence.status
            )]));
        };
        occurrence = transition_pm_occurrence(
            db,
            TransitionPmOccurrenceInput {
                occurrence_id: occurrence.id,
                expected_row_version: occurrence.row_version,
                next_status: next.to_string(),
                // Coded reason (shown translated by the UI); no fixed-language note.
                reason_code: Some(PM_REASON_WO_CLOSED.to_string()),
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id,
            },
        )
        .await?;
    }
    Ok(occurrence)
}

/// Reason code of the occurrence transitions made when the linked OT is closed.
pub const PM_REASON_WO_CLOSED: &str = "WO_CLOSED";
/// Finding recorded from a failure (mode / cause / effect) written on the closed OT.
pub const PM_FINDING_WO_FAILURE: &str = "WO_FAILURE";
/// Finding recorded from an OT checklist task closed as not OK.
pub const PM_FINDING_WO_TASK_NOK: &str = "WO_TASK_NOK";

/// What the closed OT says about the PM: its close-out text and what was found.
#[derive(Debug, Default)]
pub(crate) struct WorkOrderOutcome {
    pub close_out_note: Option<String>,
    pub findings: Vec<crate::pm::domain::PmExecutionFindingInput>,
}

impl WorkOrderOutcome {
    pub fn execution_result(&self) -> &'static str {
        if self.findings.is_empty() {
            "completed_no_findings"
        } else {
            "completed_with_findings"
        }
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn outcome_finding(finding_type: &str, description: String) -> crate::pm::domain::PmExecutionFindingInput {
    crate::pm::domain::PmExecutionFindingInput {
        finding_type: finding_type.to_string(),
        // The OT does not grade what it found; the planner rates it when following up.
        severity: None,
        description,
        create_follow_up_di: None,
        create_follow_up_work_order: None,
        follow_up_work_order_type_id: None,
    }
}

/// Reads the closed OT: corrective-action summary as the execution note, each recorded failure
/// and each checklist task closed "not OK" as a finding. An OT closed with neither is a PM
/// completed without findings — that is what the technician recorded.
pub(crate) async fn read_work_order_outcome(
    db: &DatabaseConnection,
    work_order_id: i64,
) -> AppResult<WorkOrderOutcome> {
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT corrective_action_summary FROM work_orders WHERE id = ?",
            [work_order_id.into()],
        ))
        .await?;
    let close_out_note = match wo_row {
        Some(row) => non_empty(row.try_get::<Option<String>>("", "corrective_action_summary")?),
        None => None,
    };

    let mut findings = Vec::new();
    for detail in crate::wo::closeout::get_failure_details(db, work_order_id).await? {
        let labels: Vec<String> = [
            detail.failure_mode_label,
            detail.failure_cause_label,
            detail.failure_effect_label,
        ]
        .into_iter()
        .filter_map(non_empty)
        .collect();
        let mut description = labels.join(" / ");
        if let Some(notes) = non_empty(detail.notes) {
            if description.is_empty() {
                description = notes;
            } else {
                description = format!("{description} — {notes}");
            }
        }
        if !description.is_empty() {
            findings.push(outcome_finding(PM_FINDING_WO_FAILURE, description));
        }
    }

    let nok_tasks = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT task_description, notes FROM work_order_tasks
             WHERE work_order_id = ? AND result_code = 'nok'
             ORDER BY sequence_order, id",
            [work_order_id.into()],
        ))
        .await?;
    for row in nok_tasks {
        let task: String = row.try_get("", "task_description")?;
        let notes = non_empty(row.try_get::<Option<String>>("", "notes")?);
        let description = match notes {
            Some(n) => format!("{} — {n}", task.trim()),
            None => task.trim().to_string(),
        };
        if !description.is_empty() {
            findings.push(outcome_finding(PM_FINDING_WO_TASK_NOK, description));
        }
    }

    Ok(WorkOrderOutcome {
        close_out_note,
        findings,
    })
}

/// Best-effort: when a WO linked to a PM occurrence is closed, complete the occurrence
/// (if still open) with the OT outcome (see `read_work_order_outcome`) and ensure plan-scoped
/// regeneration is audited.
pub async fn on_work_order_closed_for_pm(db: &DatabaseConnection, work_order_id: i64, actor_id: Option<i64>) {
    let result = on_work_order_closed_for_pm_inner(db, work_order_id, actor_id).await;
    if let Err(err) = result {
        tracing::warn!(
            error = %err,
            work_order_id,
            "pm::generation wo_close hook failed"
        );
    }
}

async fn on_work_order_closed_for_pm_inner(
    db: &DatabaseConnection,
    work_order_id: i64,
    actor_id: Option<i64>,
) -> AppResult<()> {
    use crate::pm::domain::ExecutePmOccurrenceInput;
    use crate::pm::queries::execute_pm_occurrence;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, pm_plan_id, status, row_version
             FROM pm_occurrences
             WHERE linked_work_order_id = ?
             ORDER BY id DESC LIMIT 1",
            [work_order_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };

    let occurrence_id: i64 = row.try_get("", "id")?;
    let pm_plan_id: i64 = row.try_get("", "pm_plan_id")?;
    let status: String = row.try_get("", "status")?;
    let row_version: i64 = row.try_get("", "row_version")?;

    if is_terminal_occurrence_status(&status) {
        if status == "completed" {
            let _ = generate_pm_for_plan(db, pm_plan_id, None, "wo_close").await?;
        }
        return Ok(());
    }

    let occurrence = crate::pm::domain::PmOccurrence {
        id: occurrence_id,
        pm_plan_id,
        plan_version_id: 0,
        due_basis: String::new(),
        due_at: None,
        due_meter_value: None,
        generated_at: String::new(),
        status,
        linked_work_order_id: Some(work_order_id),
        linked_work_order_code: None,
        deferral_reason: None,
        missed_reason: None,
        row_version,
        created_at: String::new(),
        updated_at: String::new(),
        plan_code: None,
        plan_title: None,
        strategy_type: None,
        asset_code: None,
        asset_name: None,
        equipment_id: None,
        window_start_at: None,
        window_end_at: None,
        window_start_meter: None,
        window_end_meter: None,
        timing_status: None,
    };

    let occurrence = advance_occurrence_to_in_progress(db, occurrence, actor_id).await?;
    let outcome = read_work_order_outcome(db, work_order_id).await?;
    let _ = execute_pm_occurrence(
        db,
        ExecutePmOccurrenceInput {
            occurrence_id: occurrence.id,
            expected_occurrence_row_version: occurrence.row_version,
            execution_result: outcome.execution_result().to_string(),
            note: outcome.close_out_note,
            actor_id,
            work_order_id: Some(work_order_id),
            defer_reason_code: None,
            miss_reason_code: None,
            findings: if outcome.findings.is_empty() {
                None
            } else {
                Some(outcome.findings)
            },
        },
    )
    .await?;

    // Execute already regenerates; record an explicit wo_close audit run for status chrome.
    let _ = generate_pm_for_plan(db, pm_plan_id, None, "wo_close").await;
    Ok(())
}
