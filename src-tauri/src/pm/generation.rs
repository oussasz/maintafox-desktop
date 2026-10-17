//! PM occurrence generation (forecast): which occurrences are due, for which asset, by when.
//!
//! For every active plan with a published current version:
//!
//! 1. **Targets.** A plan scoped to one equipment has that asset as its only target. A plan
//!    scoped to an equipment family (`equipment.equipment_family_ref_id`), an organisation
//!    location (assets installed in that node or below it) or a criticality group
//!    (`equipment.criticality_value_id`) fans out: one occurrence stream per matching asset in
//!    service, each with its own due dates and meter state. Fanned-out due bases are prefixed with
//!    the asset's sync identity, so every device derives the same occurrence identity.
//! 2. **Strategy** per target:
//!    - `fixed`: calendar dates from `effective_from` every interval, inside the horizon;
//!    - `floating`: one occurrence an interval after the last completion of that asset (the
//!      execution date), else after `effective_from`;
//!    - `meter`: a threshold on the asset's meter, in **lifetime** units (rollover and meter
//!      replacement keep counting, see `assets::meters`): either an absolute `threshold_value`, or
//!      every `interval_value` from `start_reading` (when given) or from the lifetime reading at
//!      `effective_from`;
//!    - `fixed`/`floating` with a `meter` block = **whichever comes first**: the calendar
//!      occurrence is generated as above, and when the meter reaches `anchor + interval` first (the
//!      anchor is the reading recorded at the last completion of that asset), the next open
//!      occurrence is brought forward to the date the meter was read, or created if none is open;
//!    - `event` / `condition`: on demand, for the codes passed to the run.
//! 3. **Window.** Each occurrence stores its tolerance window (`pm::schedule`).
//!
//! The meter is the plan's chosen `asset_meter_id` (one-equipment plans) or the asset's active
//! meter of `meter_type` (primary first), e.g. `HOURS`; legacy `meter_source` maps to a type.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{GeneratePmOccurrencesInput, GeneratePmOccurrencesResult};
use crate::pm::queries::{
    emit_pm_due_event, insert_trigger_event, is_occurrence_idempotency_conflict, next_meter_threshold,
    occurrence_exists, occurrence_sync_id, parse_as_of_or_now,
};
use crate::pm::schedule::{
    meter_window, nth_date, parse_recurrence, parse_tolerance, period_length, rfc3339, time_window, OccurrenceWindow,
    Recurrence, Tolerance,
};

/// Occurrence statuses that still wait for the work (an occurrence in progress is being done).
const OPEN_STATUSES: &str = "'forecasted','generated','ready_for_scheduling','scheduled','deferred'";
/// Asset lifecycle states that take no new preventive work.
const RETIRED_ASSET_STATES: &str = "'DECOMMISSIONED','SCRAPPED','DISPOSED'";

/// One asset (or none, for a plan without an asset) an occurrence stream is generated for.
#[derive(Debug, Clone)]
pub struct Target {
    pub equipment_id: Option<i64>,
    /// Prefix of the due basis: empty for a one-equipment plan (unchanged identities), the asset's
    /// identity for a fanned-out plan.
    pub basis_prefix: String,
}

#[derive(Default)]
struct RunCounters {
    generated: i64,
    skipped: i64,
    events: i64,
    ids: Vec<i64>,
}

struct PlanRow {
    plan_id: i64,
    version_id: i64,
    strategy: String,
    effective_from: String,
}

/// Assets in service matching a plan scope (see module docs), optionally restricted to one asset.
pub async fn resolve_targets(
    db: &impl ConnectionTrait,
    scope_type: &str,
    scope_id: Option<i64>,
    only_equipment: Option<i64>,
) -> AppResult<Vec<Target>> {
    if scope_type == "equipment" {
        return Ok(match (scope_id, only_equipment) {
            (Some(id), Some(only)) if id != only => Vec::new(),
            (Some(id), _) => {
                // A plan pointing to an asset that is not on this device (deleted, not synced yet)
                // still forecasts, without linking the occurrence to the missing asset.
                let exists = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT id FROM equipment WHERE id = ?",
                        [id.into()],
                    ))
                    .await?
                    .is_some();
                vec![Target {
                    equipment_id: exists.then_some(id),
                    basis_prefix: String::new(),
                }]
            }
            (None, Some(_)) => Vec::new(),
            (None, None) => vec![Target {
                equipment_id: None,
                basis_prefix: String::new(),
            }],
        });
    }
    let Some(scope_id) = scope_id else {
        return Ok(Vec::new());
    };
    let condition = match scope_type {
        // Same family code in any set of the family domain (a newer published set keeps it).
        "family" => "e.equipment_family_ref_id IN (SELECT r2.id FROM reference_values r1              JOIN reference_sets s1 ON s1.id = r1.set_id              JOIN reference_sets s2 ON s2.domain_id = s1.domain_id              JOIN reference_values r2 ON r2.set_id = s2.id AND r2.code = r1.code              WHERE r1.id = ?)"
            .to_string(),
        "criticality_group" => "e.criticality_value_id = ?".to_string(),
        "location" => "e.installed_at_node_id IN (SELECT n.id FROM org_nodes n \
             WHERE n.deleted_at IS NULL AND n.ancestor_path LIKE '%/' || ? || '/%')"
            .to_string(),
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Unsupported PM asset scope type '{other}'."
            )]))
        }
    };
    let mut sql = format!(
        "SELECT e.id, e.entity_sync_id FROM equipment e
         WHERE {condition} AND e.deleted_at IS NULL
           AND UPPER(COALESCE(e.lifecycle_status, '')) NOT IN ({RETIRED_ASSET_STATES})"
    );
    let mut binds: Vec<sea_orm::Value> = vec![scope_id.into()];
    if let Some(only) = only_equipment {
        sql.push_str(" AND e.id = ?");
        binds.push(only.into());
    }
    sql.push_str(" ORDER BY e.asset_id_code, e.id");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    rows.iter()
        .map(|r| {
            let id: i64 = r.try_get("", "id")?;
            let sync: Option<String> = r.try_get("", "entity_sync_id")?;
            let key = sync.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| id.to_string());
            Ok(Target {
                equipment_id: Some(id),
                basis_prefix: format!("asset:{key}|"),
            })
        })
        .collect()
}

/// A meter an occurrence stream counts on, in lifetime units.
#[derive(Debug, Clone)]
pub struct MeterState {
    pub meter_id: i64,
    /// Sync identity (local id when none), used in due bases.
    pub reference: String,
    pub lifetime: f64,
    pub last_read_at: Option<String>,
}

fn meter_type_of(spec: &Map<String, Value>) -> Option<String> {
    if let Some(t) = spec
        .get("meter_type")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    {
        return Some(t.trim().to_uppercase());
    }
    match spec
        .get("meter_source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase()
        .as_str()
    {
        "odometer" => Some("DISTANCE".to_string()),
        "operating_hours" => Some("HOURS".to_string()),
        _ => None,
    }
}

/// The meter of `spec` on the target asset (`None` when the asset has no such active meter).
pub async fn resolve_meter(
    db: &impl ConnectionTrait,
    spec: &Map<String, Value>,
    equipment_id: Option<i64>,
) -> AppResult<Option<MeterState>> {
    let explicit = spec.get("asset_meter_id").and_then(Value::as_i64).filter(|id| *id > 0);
    let row = if let Some(meter_id) = explicit {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, current_reading + reading_offset AS lifetime, last_read_at
             FROM equipment_meters WHERE id = ? AND is_active = 1",
            [meter_id.into()],
        ))
        .await?
    } else {
        let (Some(equipment_id), Some(meter_type)) = (equipment_id, meter_type_of(spec)) else {
            return Ok(None);
        };
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, current_reading + reading_offset AS lifetime, last_read_at
             FROM equipment_meters
             WHERE equipment_id = ? AND is_active = 1 AND UPPER(meter_type) = ?
             ORDER BY is_primary DESC, id ASC LIMIT 1",
            [equipment_id.into(), meter_type.into()],
        ))
        .await?
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let meter_id: i64 = row.try_get("", "id")?;
    let sync: Option<String> = row.try_get("", "entity_sync_id")?;
    Ok(Some(MeterState {
        meter_id,
        reference: sync
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| meter_id.to_string()),
        lifetime: row.try_get("", "lifetime")?,
        last_read_at: row.try_get("", "last_read_at")?,
    }))
}

/// Lifetime reading of a meter at `at` (latest accepted reading at or before it; the first
/// reading when none is older; the current value when the meter has no reading).
pub async fn lifetime_at(db: &impl ConnectionTrait, meter: &MeterState, at: &str) -> AppResult<f64> {
    for sql in [
        "SELECT COALESCE(lifetime_value, reading_value) AS v FROM asset_meter_readings
         WHERE meter_id = ? AND quality_flag != 'corrected' AND reading_at <= ?
         ORDER BY reading_at DESC LIMIT 1",
        "SELECT COALESCE(lifetime_value, reading_value) AS v FROM asset_meter_readings
         WHERE meter_id = ? AND quality_flag != 'corrected' AND ? IS NOT NULL
         ORDER BY reading_at ASC LIMIT 1",
    ] {
        if let Some(row) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                [meter.meter_id.into(), at.to_string().into()],
            ))
            .await?
        {
            return Ok(row.try_get("", "v")?);
        }
    }
    Ok(meter.lifetime)
}

/// Lifetime reading recorded at the last completion of this plan on this asset, if any.
async fn reading_at_last_completion(
    db: &impl ConnectionTrait,
    plan_id: i64,
    target: &Target,
) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pe.meter_reading FROM pm_executions pe
             JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
             WHERE po.pm_plan_id = ? AND po.equipment_id IS ?
               AND pe.execution_result IN ('completed_no_findings','completed_with_findings')
               AND pe.meter_reading IS NOT NULL
             ORDER BY pe.executed_at DESC, pe.id DESC LIMIT 1",
            [plan_id.into(), target.equipment_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => r.try_get("", "meter_reading")?,
        None => None,
    })
}

/// Date of the last completion of this plan on this asset (execution date, else the completed
/// occurrence's due date).
async fn last_completion_at(db: &impl ConnectionTrait, plan_id: i64, target: &Target) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(
                 (SELECT MAX(pe.executed_at) FROM pm_executions pe
                  WHERE pe.pm_occurrence_id = po.id
                    AND pe.execution_result IN ('completed_no_findings','completed_with_findings')),
                 po.due_at, po.updated_at) AS done_at
             FROM pm_occurrences po
             WHERE po.pm_plan_id = ? AND po.equipment_id IS ? AND po.status = 'completed'
             ORDER BY done_at DESC LIMIT 1",
            [plan_id.into(), target.equipment_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => r.try_get("", "done_at")?,
        None => None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn insert_occurrence(
    db: &impl ConnectionTrait,
    plan: &PlanRow,
    target: &Target,
    due_basis: &str,
    due_at: Option<String>,
    due_meter_value: Option<f64>,
    window: &OccurrenceWindow,
) -> AppResult<Option<i64>> {
    let entity_sync_id = occurrence_sync_id(db, plan.plan_id, plan.version_id, due_basis).await?;
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO pm_occurrences
                (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, due_meter_value, status,
                 equipment_id, window_start_at, window_end_at, window_start_meter, window_end_meter)
             VALUES (?, ?, ?, ?, ?, ?, 'generated', ?, ?, ?, ?, ?)",
            [
                entity_sync_id.into(),
                plan.plan_id.into(),
                plan.version_id.into(),
                due_basis.to_string().into(),
                due_at.into(),
                due_meter_value.into(),
                target.equipment_id.into(),
                window.start_at.clone().into(),
                window.end_at.clone().into(),
                window.start_meter.into(),
                window.end_meter.into(),
            ],
        ))
        .await;
    if let Err(db_err) = inserted {
        let err = AppError::Database(db_err);
        if is_occurrence_idempotency_conflict(&err) {
            return Ok(None);
        }
        return Err(err);
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM pm_occurrences WHERE rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read inserted PM occurrence")))?;
    Ok(Some(row.try_get("", "id")?))
}

/// Creates the occurrence unless it exists; records the trigger event either way.
#[allow(clippy::too_many_arguments)]
async fn emit_occurrence(
    db: &impl ConnectionTrait,
    counters: &mut RunCounters,
    plan: &PlanRow,
    target: &Target,
    trigger_type: &str,
    due_basis: String,
    due_at: Option<String>,
    due_meter_value: Option<f64>,
    measured: Option<f64>,
    window: &OccurrenceWindow,
    source_reference: Option<String>,
) -> AppResult<()> {
    let exists = occurrence_exists(
        db,
        plan.plan_id,
        plan.version_id,
        &due_basis,
        due_at.as_deref(),
        due_meter_value,
    )
    .await?;
    let created = if exists {
        None
    } else {
        insert_occurrence(db, plan, target, &due_basis, due_at, due_meter_value, window).await?
    };
    match created {
        Some(id) => {
            counters.generated += 1;
            counters.ids.push(id);
        }
        None => counters.skipped += 1,
    }
    insert_trigger_event(
        db,
        plan.plan_id,
        plan.version_id,
        trigger_type,
        Some(source_reference.unwrap_or(due_basis)),
        measured,
        due_meter_value,
        created.is_some(),
        created,
    )
    .await?;
    counters.events += 1;
    Ok(())
}

fn parse_ts(raw: &str, field: &str) -> AppResult<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(raw)
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be RFC3339.")]))?
        .with_timezone(&Utc))
}

fn calendar_window(due: DateTime<Utc>, recurrence: Recurrence, tolerance: Option<&Tolerance>) -> OccurrenceWindow {
    let (start, end) = time_window(due, Some(period_length(due, recurrence)), tolerance);
    OccurrenceWindow {
        start_at: start.map(rfc3339),
        end_at: Some(rfc3339(end)),
        start_meter: None,
        end_meter: None,
    }
}

fn meter_occurrence_window(
    due: DateTime<Utc>,
    threshold: f64,
    interval: Option<f64>,
    tolerance: Option<&Tolerance>,
) -> OccurrenceWindow {
    let (start, end) = time_window(due, None, tolerance);
    let (start_meter, end_meter) = meter_window(threshold, interval, tolerance);
    OccurrenceWindow {
        start_at: start.map(rfc3339),
        end_at: Some(rfc3339(end)),
        start_meter,
        end_meter,
    }
}

#[allow(clippy::too_many_arguments)]
async fn generate_calendar(
    db: &impl ConnectionTrait,
    counters: &mut RunCounters,
    plan: &PlanRow,
    target: &Target,
    trigger: &Map<String, Value>,
    tolerance: Option<&Tolerance>,
    as_of: DateTime<Utc>,
    horizon_end: DateTime<Utc>,
) -> AppResult<()> {
    let recurrence = parse_recurrence(trigger)?;
    if plan.strategy == "fixed" {
        // Dates are counted from `effective_from` (k-th period), so calendar months never drift.
        let anchor = parse_ts(&plan.effective_from, "effective_from")?;
        let mut k: u32 = 0;
        let mut due = anchor;
        while due < as_of {
            k += 1;
            due = nth_date(anchor, recurrence, k);
        }
        while due <= horizon_end {
            let due_at = rfc3339(due);
            let basis = format!("{}fixed:{}", target.basis_prefix, due_at);
            let window = calendar_window(due, recurrence, tolerance);
            emit_occurrence(
                db,
                counters,
                plan,
                target,
                "fixed",
                basis,
                Some(due_at),
                None,
                None,
                &window,
                None,
            )
            .await?;
            k += 1;
            due = nth_date(anchor, recurrence, k);
        }
        return Ok(());
    }
    // floating: one period after the last completion (or after `effective_from`).
    let base = match last_completion_at(db, plan.plan_id, target).await? {
        Some(done) => done,
        None => plan.effective_from.clone(),
    };
    let base = parse_ts(&base, "floating base")?;
    let mut k: u32 = 1;
    let mut due = nth_date(base, recurrence, k);
    while due < as_of {
        k += 1;
        due = nth_date(base, recurrence, k);
    }
    if due <= horizon_end {
        let due_at = rfc3339(due);
        let basis = format!("{}floating:{}", target.basis_prefix, due_at);
        let window = calendar_window(due, recurrence, tolerance);
        emit_occurrence(
            db,
            counters,
            plan,
            target,
            "floating",
            basis,
            Some(due_at),
            None,
            None,
            &window,
            None,
        )
        .await?;
    }
    Ok(())
}

/// Pure meter strategy.
#[allow(clippy::too_many_arguments)]
async fn generate_meter(
    db: &impl ConnectionTrait,
    counters: &mut RunCounters,
    plan: &PlanRow,
    target: &Target,
    trigger: &Map<String, Value>,
    tolerance: Option<&Tolerance>,
    as_of: DateTime<Utc>,
) -> AppResult<()> {
    let Some(meter) = resolve_meter(db, trigger, target.equipment_id).await? else {
        if trigger.get("asset_meter_id").and_then(Value::as_i64).unwrap_or(0) > 0 {
            return Err(AppError::ValidationFailed(vec![
                "Meter trigger references unknown or inactive asset meter.".to_string(),
            ]));
        }
        // The asset has no meter of that type: nothing can be due on it.
        counters.skipped += 1;
        return Ok(());
    };
    let absolute = trigger
        .get("threshold_value")
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0);
    let interval = trigger
        .get("interval_value")
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0);
    let threshold = match (absolute, interval) {
        (Some(absolute), _) => absolute,
        (None, Some(interval)) => {
            let start = match trigger.get("start_reading").and_then(Value::as_f64) {
                Some(start) => start,
                None => lifetime_at(db, &meter, &plan.effective_from).await?,
            };
            let last = last_threshold(db, plan.plan_id, &target.basis_prefix, &meter.reference).await?;
            next_meter_threshold(start, interval, meter.lifetime, last)
        }
        (None, None) => {
            return Err(AppError::ValidationFailed(vec![
                "meter trigger requires threshold_value or interval_value > 0.".to_string(),
            ]))
        }
    };
    let basis = format!("{}meter:{}:{:.4}", target.basis_prefix, meter.reference, threshold);
    if meter.lifetime >= threshold {
        let due = meter
            .last_read_at
            .as_deref()
            .and_then(|r| DateTime::parse_from_rfc3339(r).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(as_of);
        let window = meter_occurrence_window(due, threshold, interval, tolerance);
        emit_occurrence(
            db,
            counters,
            plan,
            target,
            "meter",
            basis,
            Some(rfc3339(due)),
            Some(threshold),
            Some(meter.lifetime),
            &window,
            Some(format!("asset_meter:{}", meter.meter_id)),
        )
        .await?;
    } else {
        insert_trigger_event(
            db,
            plan.plan_id,
            plan.version_id,
            "meter",
            Some(format!("asset_meter:{}", meter.meter_id)),
            Some(meter.lifetime),
            Some(threshold),
            false,
            None,
        )
        .await?;
        counters.events += 1;
    }
    Ok(())
}

async fn last_threshold(
    db: &impl ConnectionTrait,
    plan_id: i64,
    prefix: &str,
    meter_ref: &str,
) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MAX(due_meter_value) AS last_threshold FROM pm_occurrences
             WHERE pm_plan_id = ? AND due_basis LIKE ?",
            [plan_id.into(), format!("{prefix}meter:{meter_ref}:%").into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => r.try_get("", "last_threshold")?,
        None => None,
    })
}

/// "Whichever comes first": the meter part of a calendar version (`meter` block).
#[allow(clippy::too_many_arguments)]
async fn apply_meter_first(
    db: &impl ConnectionTrait,
    counters: &mut RunCounters,
    plan: &PlanRow,
    target: &Target,
    spec: &Map<String, Value>,
    tolerance: Option<&Tolerance>,
    as_of: DateTime<Utc>,
) -> AppResult<()> {
    let Some(interval) = spec.get("interval_value").and_then(Value::as_f64).filter(|v| *v > 0.0) else {
        return Err(AppError::ValidationFailed(vec![
            "meter block requires interval_value > 0.".to_string(),
        ]));
    };
    let Some(meter) = resolve_meter(db, spec, target.equipment_id).await? else {
        return Ok(());
    };
    let anchor = match reading_at_last_completion(db, plan.plan_id, target).await? {
        Some(r) => r,
        None => match spec.get("start_reading").and_then(Value::as_f64) {
            Some(start) => start,
            None => lifetime_at(db, &meter, &plan.effective_from).await?,
        },
    };
    let threshold = anchor + interval;
    if meter.lifetime < threshold {
        return Ok(());
    }
    let crossed = meter
        .last_read_at
        .as_deref()
        .and_then(|r| DateTime::parse_from_rfc3339(r).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(as_of);
    let crossed_at = rfc3339(crossed);
    let window = meter_occurrence_window(crossed, threshold, Some(interval), tolerance);

    // Already due (or being done) no later than the meter date: nothing to bring forward.
    let already_due = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS c FROM pm_occurrences
                 WHERE pm_plan_id = ? AND equipment_id IS ?
                   AND (status = 'in_progress' OR (status IN ({OPEN_STATUSES}) AND due_at <= ?))"
            ),
            [
                plan.plan_id.into(),
                target.equipment_id.into(),
                crossed_at.clone().into(),
            ],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "c"))
        .transpose()?
        .unwrap_or(0);
    if already_due > 0 {
        return Ok(());
    }
    let next_open = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT id FROM pm_occurrences
                 WHERE pm_plan_id = ? AND equipment_id IS ? AND status IN ({OPEN_STATUSES})
                 ORDER BY due_at ASC, id ASC LIMIT 1"
            ),
            [plan.plan_id.into(), target.equipment_id.into()],
        ))
        .await?;
    if let Some(row) = next_open {
        let id: i64 = row.try_get("", "id")?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE pm_occurrences
             SET due_at = ?, due_meter_value = ?, window_start_at = ?, window_end_at = ?,
                 window_start_meter = ?, window_end_meter = ?,
                 row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [
                crossed_at.into(),
                threshold.into(),
                window.start_at.clone().into(),
                window.end_at.clone().into(),
                window.start_meter.into(),
                window.end_meter.into(),
                id.into(),
            ],
        ))
        .await?;
        insert_trigger_event(
            db,
            plan.plan_id,
            plan.version_id,
            "meter",
            Some(format!("asset_meter:{}", meter.meter_id)),
            Some(meter.lifetime),
            Some(threshold),
            false,
            Some(id),
        )
        .await?;
        counters.events += 1;
        return Ok(());
    }
    let basis = format!("{}meter:{}:{:.4}", target.basis_prefix, meter.reference, threshold);
    emit_occurrence(
        db,
        counters,
        plan,
        target,
        "meter",
        basis,
        Some(crossed_at),
        Some(threshold),
        Some(meter.lifetime),
        &window,
        Some(format!("asset_meter:{}", meter.meter_id)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn generate_on_demand(
    db: &impl ConnectionTrait,
    counters: &mut RunCounters,
    plan: &PlanRow,
    target: &Target,
    trigger: &Map<String, Value>,
    tolerance: Option<&Tolerance>,
    as_of: DateTime<Utc>,
    codes: &HashSet<String>,
) -> AppResult<()> {
    let key = if plan.strategy == "event" {
        "event_code"
    } else {
        "condition_code"
    };
    let code = trigger
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "{} trigger requires {key}.",
            plan.strategy
        )]));
    }
    if codes.contains(&code) {
        let due_at = rfc3339(as_of);
        let basis = format!(
            "{}{}:{}:{}",
            target.basis_prefix,
            plan.strategy,
            code,
            as_of.format("%Y-%m-%d")
        );
        let (start, end) = time_window(as_of, None, tolerance);
        let window = OccurrenceWindow {
            start_at: start.map(rfc3339),
            end_at: Some(rfc3339(end)),
            start_meter: None,
            end_meter: None,
        };
        emit_occurrence(
            db,
            counters,
            plan,
            target,
            &plan.strategy,
            basis,
            Some(due_at),
            None,
            None,
            &window,
            Some(code),
        )
        .await?;
    } else {
        insert_trigger_event(
            db,
            plan.plan_id,
            plan.version_id,
            &plan.strategy,
            Some(code),
            None,
            None,
            false,
            None,
        )
        .await?;
        counters.events += 1;
    }
    Ok(())
}

pub async fn generate(
    db: &DatabaseConnection,
    input: GeneratePmOccurrencesInput,
) -> AppResult<GeneratePmOccurrencesResult> {
    let as_of = parse_as_of_or_now(input.as_of.as_deref())?;
    let horizon_days = input.horizon_days.unwrap_or(30).clamp(1, 365);
    let horizon_end = as_of + Duration::days(horizon_days);
    let normalise = |codes: Option<Vec<String>>| -> HashSet<String> {
        codes
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.trim().to_uppercase())
            .collect()
    };
    let event_codes = normalise(input.event_codes);
    let condition_codes = normalise(input.condition_codes);

    let mut sql = String::from(
        "SELECT p.id AS pm_plan_id, p.strategy_type, p.asset_scope_type, p.asset_scope_id,
                pv.id AS version_id, pv.effective_from, pv.trigger_definition_json
         FROM pm_plans p
         JOIN pm_plan_versions pv ON pv.id = p.current_version_id
         WHERE p.is_active = 1
           AND p.lifecycle_status IN ('active', 'suspended')
           AND pv.status = 'published'",
    );
    let mut binds: Vec<sea_orm::Value> = Vec::new();
    if let Some(plan_id) = input.pm_plan_id {
        sql.push_str(" AND p.id = ?");
        binds.push(plan_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let tx = db.begin().await?;
    // Occurrences are derived by every device from the synchronised plans (each device runs the
    // generator): they get a deterministic identity (`occurrence_sync_id`) and are not captured
    // here. A person's later action on an occurrence is captured and sent with the full row.
    crate::sync::capture::begin_suppression(&tx).await?;
    let mut counters = RunCounters::default();

    for row in rows {
        let plan = PlanRow {
            plan_id: row.try_get("", "pm_plan_id")?,
            version_id: row.try_get("", "version_id")?,
            strategy: row.try_get("", "strategy_type")?,
            effective_from: row.try_get("", "effective_from")?,
        };
        let scope_type: String = row.try_get("", "asset_scope_type")?;
        let scope_id: Option<i64> = row.try_get("", "asset_scope_id")?;
        let trigger_json: String = row.try_get("", "trigger_definition_json")?;
        let trigger_value = serde_json::from_str::<Value>(&trigger_json)
            .map_err(|_| AppError::ValidationFailed(vec!["trigger_definition_json must be valid JSON.".to_string()]))?;
        let Some(trigger) = trigger_value.as_object() else {
            return Err(AppError::ValidationFailed(vec![
                "trigger_definition_json must be a JSON object.".to_string(),
            ]));
        };
        let tolerance = parse_tolerance(trigger)?;
        let targets = resolve_targets(&tx, &scope_type, scope_id, input.equipment_id).await?;
        for target in &targets {
            match plan.strategy.as_str() {
                "fixed" | "floating" => {
                    generate_calendar(
                        &tx,
                        &mut counters,
                        &plan,
                        target,
                        trigger,
                        tolerance.as_ref(),
                        as_of,
                        horizon_end,
                    )
                    .await?;
                    if let Some(meter_spec) = trigger.get("meter").and_then(Value::as_object) {
                        apply_meter_first(&tx, &mut counters, &plan, target, meter_spec, tolerance.as_ref(), as_of)
                            .await?;
                    }
                }
                "meter" => {
                    generate_meter(&tx, &mut counters, &plan, target, trigger, tolerance.as_ref(), as_of).await?;
                }
                "event" => {
                    generate_on_demand(
                        &tx,
                        &mut counters,
                        &plan,
                        target,
                        trigger,
                        tolerance.as_ref(),
                        as_of,
                        &event_codes,
                    )
                    .await?;
                }
                "condition" => {
                    generate_on_demand(
                        &tx,
                        &mut counters,
                        &plan,
                        target,
                        trigger,
                        tolerance.as_ref(),
                        as_of,
                        &condition_codes,
                    )
                    .await?;
                }
                other => {
                    return Err(AppError::ValidationFailed(vec![format!(
                        "Unsupported PM strategy type '{other}'."
                    )]));
                }
            }
        }
    }
    crate::sync::capture::end_suppression(&tx).await?;
    tx.commit().await?;
    for occurrence_id in &counters.ids {
        let _ = emit_pm_due_event(db, *occurrence_id).await;
    }
    Ok(GeneratePmOccurrencesResult {
        generated_count: counters.generated,
        skipped_count: counters.skipped,
        trigger_events_recorded: counters.events,
        occurrence_ids: counters.ids,
    })
}
