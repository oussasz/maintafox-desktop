use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::reliability::advanced_rams::domain::{
    CreateFmecaAnalysisInput, CreateRcmStudyInput, FmecaAnalysesFilter, FmecaAnalysis, FmecaItem, FmecaItemWithContext,
    FmecaItemsEquipmentFilter, FmecaSeverityOccurrenceMatrix, FmecaSoCell, RamIshikawaDiagram,
    RamIshikawaDiagramsFilter, RcmDecision, RcmStudiesFilter, RcmStudy, ReliabilityRulIndicator,
    SuggestedPartForFailure, SuggestedPartsForFailureInput, UpdateFmecaAnalysisInput, UpdateRcmStudyInput,
    UpsertFmecaItemInput, UpsertRamIshikawaDiagramInput, UpsertRcmDecisionInput, WeibullCurvePoint,
    WeibullDashboardInput, WeibullDashboardPayload, WeibullFitRecord, WeibullFitRunInput, WeibullPmMarker,
};
use crate::reliability::compute::eligible_unplanned_mtbf;
use crate::reliability::exposure_inference::{load_exposure_basis, ExposureBasis};
use crate::reliability::readiness_gate::{
    evaluate_equipment_readiness, ReliabilityReadinessGate, ReliabilityReadinessInput,
};
use crate::reliability::weibull_fit::{fit_weibull_censored_with_ci, WeibullObservation, WEIBULL_ALGORITHM_VERSION};
use crate::settings;

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("advanced_rams decode '{field}': {err}"))
}

fn parse_ts(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::ValidationFailed(vec![format!("GATE_REL_DATE_INVALID: invalid timestamp: {e}")]))
}

/// Exposure-hour time base marker persisted on `weibull_fit_results.time_base`.
pub const WEIBULL_TIME_BASE_EXPOSURE: &str = "exposure_hours";

/// One eligible failure event used to build a Weibull dataset.
#[derive(Debug, Clone, Serialize)]
pub struct WeibullSourceEvent {
    pub id: i64,
    pub failed_at: DateTime<Utc>,
    /// Restoration instant; when it lies strictly between this and the next failure the next interval
    /// starts here (repair time is not operating exposure).
    pub restored_at: Option<DateTime<Utc>>,
}

impl WeibullSourceEvent {
    fn renewal(&self, next: Option<DateTime<Utc>>) -> DateTime<Utc> {
        match (self.restored_at, next) {
            (Some(r), Some(n)) if r > self.failed_at && r < n => r,
            (Some(r), None) if r > self.failed_at => r,
            _ => self.failed_at,
        }
    }
}

/// Times-between-failures in exposure hours plus the optional right-censored tail.
/// Intervals with zero exposure (e.g. two failures inside one non-operating window) are dropped and
/// counted, since a Weibull time-to-failure must be positive.
#[must_use]
pub fn weibull_observations_from_events(
    events: &[WeibullSourceEvent],
    basis: &ExposureBasis,
    censor_at: Option<DateTime<Utc>>,
) -> (Vec<WeibullObservation>, usize) {
    let mut out = Vec::with_capacity(events.len());
    let mut dropped = 0_usize;
    for w in events.windows(2) {
        let start = w[0].renewal(Some(w[1].failed_at));
        let h = basis.hours_between(start, w[1].failed_at);
        if h > 0.0 && h.is_finite() {
            out.push(WeibullObservation::failure(h));
        } else {
            dropped += 1;
        }
    }
    if let (Some(end), Some(last)) = (censor_at, events.last()) {
        let h = basis.hours_between(last.renewal(None), end);
        if h > 0.0 && h.is_finite() {
            out.push(WeibullObservation::censored(h));
        }
    }
    (out, dropped)
}

/// Eligible (`eligible_unplanned_mtbf`) failure events for one equipment, optionally one failure mode,
/// ordered by failure time.
pub async fn load_weibull_source_events(
    db: &DatabaseConnection,
    equipment_id: i64,
    period_start: Option<&str>,
    period_end: Option<&str>,
    failure_mode_id: Option<i64>,
) -> AppResult<Vec<WeibullSourceEvent>> {
    let mut sql = String::from(
        "SELECT id, COALESCE(failed_at, detected_at, created_at) AS ts, restored_at, eligible_flags_json
         FROM failure_events WHERE equipment_id = ?",
    );
    let mut vals: Vec<sea_orm::Value> = vec![equipment_id.into()];
    if let Some(ps) = period_start {
        sql.push_str(" AND COALESCE(failed_at, detected_at, created_at) >= ?");
        vals.push(ps.to_string().into());
    }
    if let Some(pe) = period_end {
        sql.push_str(" AND COALESCE(failed_at, detected_at, created_at) <= ?");
        vals.push(pe.to_string().into());
    }
    if let Some(m) = failure_mode_id {
        sql.push_str(" AND failure_mode_id = ?");
        vals.push(m.into());
    }
    sql.push_str(" ORDER BY ts ASC, id ASC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let flags: String = r
            .try_get("", "eligible_flags_json")
            .map_err(|e| decode_err("eligible_flags_json", e))?;
        if !eligible_unplanned_mtbf(&flags) {
            continue;
        }
        let ts: String = r.try_get("", "ts").map_err(|e| decode_err("ts", e))?;
        let restored: Option<String> = r.try_get("", "restored_at").map_err(|e| decode_err("restored_at", e))?;
        out.push(WeibullSourceEvent {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            failed_at: parse_ts(&ts)?,
            restored_at: restored.as_deref().map(parse_ts).transpose()?,
        });
    }
    Ok(out)
}

#[derive(Serialize)]
struct WeibullDatasetCanonical<'a> {
    algorithm_version: &'a str,
    time_base: &'a str,
    equipment_id: i64,
    period_start: Option<&'a str>,
    period_end: Option<&'a str>,
    failure_mode_id: Option<i64>,
    include_censored: bool,
    exposure_source: &'a str,
    events: &'a [WeibullSourceEvent],
    observations: &'a [WeibullObservation],
}

fn weibull_dataset_hash(canonical: &WeibullDatasetCanonical<'_>) -> String {
    let json = serde_json::to_string(canonical).unwrap_or_default();
    hex::encode(Sha256::digest(json.as_bytes()))
}

fn opt_str_value(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn opt_f64_value(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

fn opt_i64_value(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

/// Fit a right-censored Weibull on exposure-hour times between eligible unplanned failures.
/// Refused (`GATE_REL_READINESS_*`) when the StrictRam readiness gate denies the equipment.
pub async fn run_and_store_weibull_fit(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: WeibullFitRunInput,
) -> AppResult<WeibullFitRecord> {
    let include_censored = input.include_censored.unwrap_or(true);
    let gate = evaluate_equipment_readiness(
        db,
        ReliabilityReadinessInput {
            equipment_id: input.equipment_id,
            period_start: input.period_start.clone(),
            period_end: input.period_end.clone(),
            min_sample_n: None,
        },
    )
    .await?;
    if !gate.allowed() {
        return Err(gate.deny_error());
    }
    let basis = load_exposure_basis(db, input.equipment_id).await?.ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "GATE_REL_READINESS_NO_EXPOSURE: no runtime exposure for this equipment.".into(),
        ])
    })?;

    let events = load_weibull_source_events(
        db,
        input.equipment_id,
        input.period_start.as_deref(),
        input.period_end.as_deref(),
        input.failure_mode_id,
    )
    .await?;
    let now = Utc::now();
    let censor_at = if include_censored {
        let end = input.period_end.as_deref().map(parse_ts).transpose()?.unwrap_or(now);
        Some(if end < now { end } else { now })
    } else {
        None
    };
    let (observations, dropped) = weibull_observations_from_events(&events, &basis, censor_at);
    let fit = fit_weibull_censored_with_ci(&observations);
    let failure_hours: Vec<f64> = observations.iter().filter(|o| !o.censored).map(|o| o.hours).collect();
    let inter_json = serde_json::to_string(&failure_hours)?;
    let observations_json = serde_json::to_string(&observations)?;
    let dataset_hash = weibull_dataset_hash(&WeibullDatasetCanonical {
        algorithm_version: WEIBULL_ALGORITHM_VERSION,
        time_base: WEIBULL_TIME_BASE_EXPOSURE,
        equipment_id: input.equipment_id,
        period_start: input.period_start.as_deref(),
        period_end: input.period_end.as_deref(),
        failure_mode_id: input.failure_mode_id,
        include_censored,
        exposure_source: basis.source(),
        events: &events,
        observations: &observations,
    });
    let mut message = fit.message.clone();
    if dropped > 0 {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(&format!("{dropped} zero-exposure interval(s) dropped"));
    }

    let uid = user_id.map(i64::from);
    let eid = format!("weibull_fit:{}", Uuid::new_v4());
    let ok = fit.adequate_sample;
    let pick = |v: f64| if ok { Some(v) } else { None };
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO weibull_fit_results (
            entity_sync_id, equipment_id, period_start, period_end, n_points,
            inter_arrival_hours_json, beta, eta, beta_ci_low, beta_ci_high,
            eta_ci_low, eta_ci_high, adequate_sample, message, row_version, created_at, created_by_id,
            algorithm_version, dataset_hash_sha256, time_base, failure_mode_id,
            n_failures, n_censored, observations_json, exposure_source
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                eid.into(),
                input.equipment_id.into(),
                opt_str_value(input.period_start.clone()),
                opt_str_value(input.period_end.clone()),
                (observations.len() as i64).into(),
                inter_json.into(),
                opt_f64_value(pick(fit.beta)),
                opt_f64_value(pick(fit.eta)),
                opt_f64_value(pick(fit.beta_ci_low)),
                opt_f64_value(pick(fit.beta_ci_high)),
                opt_f64_value(pick(fit.eta_ci_low)),
                opt_f64_value(pick(fit.eta_ci_high)),
                i64::from(ok).into(),
                message.into(),
                now.to_rfc3339().into(),
                opt_i64_value(uid),
                WEIBULL_ALGORITHM_VERSION.into(),
                dataset_hash.into(),
                WEIBULL_TIME_BASE_EXPOSURE.into(),
                opt_i64_value(input.failure_mode_id),
                (fit.n_failures as i64).into(),
                (fit.n_censored as i64).into(),
                observations_json.into(),
                basis.source().into(),
            ],
        ))
        .await?;
    let id = crate::db::inserted_id(&inserted)?;
    get_weibull_fit(db, id)
        .await?
        .ok_or_else(|| AppError::SyncError("weibull_fit_results insert missing.".into()))
}

const WEIBULL_SELECT: &str = "SELECT id, entity_sync_id, equipment_id, period_start, period_end, n_points,
        inter_arrival_hours_json, beta, eta, beta_ci_low, beta_ci_high,
        eta_ci_low, eta_ci_high, adequate_sample, message, row_version, created_at, created_by_id,
        algorithm_version, dataset_hash_sha256, time_base, failure_mode_id,
        n_failures, n_censored, observations_json, exposure_source
     FROM weibull_fit_results";

pub async fn get_weibull_fit(db: &DatabaseConnection, id: i64) -> AppResult<Option<WeibullFitRecord>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{WEIBULL_SELECT} WHERE id = ?"),
            [id.into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => map_weibull(&r).map(Some),
    }
}

/// Latest fit of any version (history / audit display).
pub async fn get_latest_weibull_fit_for_equipment(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<Option<WeibullFitRecord>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{WEIBULL_SELECT} WHERE equipment_id = ? ORDER BY id DESC LIMIT 1"),
            [equipment_id.into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => map_weibull(&r).map(Some),
    }
}

/// Latest adequate fit on the exposure-hour time base — the only fits R(t) / RUL may use.
/// Legacy calendar-gap fits (migration 151) are ignored.
async fn latest_usable_weibull_fit(db: &DatabaseConnection, equipment_id: i64) -> AppResult<Option<WeibullFitRecord>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{WEIBULL_SELECT} WHERE equipment_id = ? AND time_base = ? AND adequate_sample = 1
                   AND beta > 0 AND eta > 0
                 ORDER BY id DESC LIMIT 1"
            ),
            [equipment_id.into(), WEIBULL_TIME_BASE_EXPOSURE.into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => map_weibull(&r).map(Some),
    }
}

/// Current age in exposure hours since the last renewal (restoration of the last eligible failure of
/// the fit's failure-mode scope) — the same clock the fit's times-between-failures were measured on.
async fn exposure_age_hours(
    db: &DatabaseConnection,
    equipment_id: i64,
    failure_mode_id: Option<i64>,
    basis: &ExposureBasis,
) -> AppResult<Option<f64>> {
    let events = load_weibull_source_events(db, equipment_id, None, None, failure_mode_id).await?;
    Ok(events
        .last()
        .map(|last| basis.hours_between(last.renewal(None), Utc::now())))
}

fn map_weibull(row: &sea_orm::QueryResult) -> AppResult<WeibullFitRecord> {
    Ok(WeibullFitRecord {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        period_start: row
            .try_get::<Option<String>>("", "period_start")
            .map_err(|e| decode_err("period_start", e))?,
        period_end: row
            .try_get::<Option<String>>("", "period_end")
            .map_err(|e| decode_err("period_end", e))?,
        n_points: row.try_get("", "n_points").map_err(|e| decode_err("n_points", e))?,
        inter_arrival_hours_json: row
            .try_get("", "inter_arrival_hours_json")
            .map_err(|e| decode_err("inter_arrival_hours_json", e))?,
        beta: row
            .try_get::<Option<f64>>("", "beta")
            .map_err(|e| decode_err("beta", e))?,
        eta: row
            .try_get::<Option<f64>>("", "eta")
            .map_err(|e| decode_err("eta", e))?,
        beta_ci_low: row
            .try_get::<Option<f64>>("", "beta_ci_low")
            .map_err(|e| decode_err("beta_ci_low", e))?,
        beta_ci_high: row
            .try_get::<Option<f64>>("", "beta_ci_high")
            .map_err(|e| decode_err("beta_ci_high", e))?,
        eta_ci_low: row
            .try_get::<Option<f64>>("", "eta_ci_low")
            .map_err(|e| decode_err("eta_ci_low", e))?,
        eta_ci_high: row
            .try_get::<Option<f64>>("", "eta_ci_high")
            .map_err(|e| decode_err("eta_ci_high", e))?,
        adequate_sample: row
            .try_get::<i64>("", "adequate_sample")
            .map_err(|e| decode_err("adequate_sample", e))?
            != 0,
        message: row.try_get("", "message").map_err(|e| decode_err("message", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        algorithm_version: row
            .try_get::<Option<String>>("", "algorithm_version")
            .map_err(|e| decode_err("algorithm_version", e))?,
        dataset_hash_sha256: row
            .try_get::<Option<String>>("", "dataset_hash_sha256")
            .map_err(|e| decode_err("dataset_hash_sha256", e))?,
        time_base: row
            .try_get::<Option<String>>("", "time_base")
            .map_err(|e| decode_err("time_base", e))?,
        failure_mode_id: row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?,
        n_failures: row
            .try_get::<Option<i64>>("", "n_failures")
            .map_err(|e| decode_err("n_failures", e))?,
        n_censored: row
            .try_get::<Option<i64>>("", "n_censored")
            .map_err(|e| decode_err("n_censored", e))?,
        observations_json: row
            .try_get::<Option<String>>("", "observations_json")
            .map_err(|e| decode_err("observations_json", e))?,
        exposure_source: row
            .try_get::<Option<String>>("", "exposure_source")
            .map_err(|e| decode_err("exposure_source", e))?,
    })
}

fn validate_sod(s: i64, o: i64, d: i64) -> AppResult<()> {
    for (name, code, v) in [
        ("severity", "GATE_REL_SEVERITY_RANGE", s),
        ("occurrence", "GATE_REL_OCCURRENCE_RANGE", o),
        ("detectability", "GATE_REL_DETECTABILITY_RANGE", d),
    ] {
        if !(1..=10).contains(&v) {
            return Err(AppError::ValidationFailed(vec![format!(
                "{code}:value={v}: {name} must be 1-10, got {v}"
            )]));
        }
    }
    Ok(())
}

async fn ensure_work_failure_modes_published_and_active_exists(db: &DatabaseConnection) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM reference_values rv
             INNER JOIN reference_sets rs ON rs.id = rv.set_id
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
               AND rs.status = 'published'
               AND rv.is_active = 1",
            [],
        ))
        .await?;
    let count: i64 = row
        .ok_or_else(|| AppError::SyncError("WORK.FAILURE_MODES published count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("work_failure_modes_published_count", e))?;
    if count <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FMECA_REFERENCE_MODES_NOT_PUBLISHED".into(),
        ]));
    }
    Ok(())
}

async fn ensure_failure_mode_id_governed(db: &DatabaseConnection, failure_mode_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM reference_values rv
             INNER JOIN reference_sets rs ON rs.id = rv.set_id
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE rv.id = ?
               AND rv.is_active = 1
               AND rs.status = 'published'
               AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))",
            [failure_mode_id.into()],
        ))
        .await?;
    let count: i64 = row
        .ok_or_else(|| AppError::SyncError("governed failure mode check missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("governed_failure_mode_count", e))?;
    if count <= 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_FMECA_FAILURE_MODE_NOT_GOVERNED:failure_mode_id={failure_mode_id}"
        )]));
    }
    Ok(())
}

fn map_fmeca_analysis(row: &sea_orm::QueryResult) -> AppResult<FmecaAnalysis> {
    Ok(FmecaAnalysis {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        boundary_definition: row
            .try_get("", "boundary_definition")
            .map_err(|e| decode_err("boundary_definition", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_fmeca_analyses(
    db: &DatabaseConnection,
    filter: FmecaAnalysesFilter,
) -> AppResult<Vec<FmecaAnalysis>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                row_version, created_at, created_by_id, updated_at
         FROM fmeca_analyses WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_fmeca_analysis).collect()
}

pub async fn create_fmeca_analysis(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: CreateFmecaAnalysisInput,
) -> AppResult<FmecaAnalysis> {
    let now = Utc::now().to_rfc3339();
    let eid = format!("fmeca_analysis:{}", Uuid::new_v4());
    let status = input.status.unwrap_or_else(|| "draft".into());
    let boundary = input.boundary_definition.unwrap_or_default();
    let inserted = db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO fmeca_analyses (
            entity_sync_id, equipment_id, title, boundary_definition, status, row_version, created_at, created_by_id, updated_at
        ) VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            boundary.into(),
            status.into(),
            now.clone().into(),
            user_id.map(i64::from).map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = crate::db::inserted_id(&inserted)?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                    row_version, created_at, created_by_id, updated_at
             FROM fmeca_analyses WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("fmeca_analyses row missing.".into()))?;
    map_fmeca_analysis(&row)
}

pub async fn update_fmeca_analysis(
    db: &DatabaseConnection,
    input: UpdateFmecaAnalysisInput,
) -> AppResult<FmecaAnalysis> {
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE fmeca_analyses SET
                title = COALESCE(?, title),
                boundary_definition = COALESCE(?, boundary_definition),
                status = COALESCE(?, status),
                row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input
                    .title
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input
                    .boundary_definition
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input
                    .status
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                Utc::now().to_rfc3339().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: fmeca_analyses update conflict.".into(),
        ]));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                    row_version, created_at, created_by_id, updated_at
             FROM fmeca_analyses WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "FmecaAnalysis".into(),
            id: input.id.to_string(),
        })?;
    map_fmeca_analysis(&row)
}

pub async fn delete_fmeca_analysis(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM fmeca_analyses WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_fmeca_item(row: &sea_orm::QueryResult) -> AppResult<FmecaItem> {
    Ok(FmecaItem {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        analysis_id: row
            .try_get("", "analysis_id")
            .map_err(|e| decode_err("analysis_id", e))?,
        component_id: row
            .try_get::<Option<i64>>("", "component_id")
            .map_err(|e| decode_err("component_id", e))?,
        functional_failure: row
            .try_get("", "functional_failure")
            .map_err(|e| decode_err("functional_failure", e))?,
        failure_mode_id: row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?,
        failure_effect: row
            .try_get("", "failure_effect")
            .map_err(|e| decode_err("failure_effect", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        occurrence: row.try_get("", "occurrence").map_err(|e| decode_err("occurrence", e))?,
        detectability: row
            .try_get("", "detectability")
            .map_err(|e| decode_err("detectability", e))?,
        rpn: row.try_get("", "rpn").map_err(|e| decode_err("rpn", e))?,
        recommended_action: row
            .try_get("", "recommended_action")
            .map_err(|e| decode_err("recommended_action", e))?,
        current_control: row
            .try_get("", "current_control")
            .map_err(|e| decode_err("current_control", e))?,
        linked_pm_plan_id: row
            .try_get::<Option<i64>>("", "linked_pm_plan_id")
            .map_err(|e| decode_err("linked_pm_plan_id", e))?,
        linked_work_order_id: row
            .try_get::<Option<i64>>("", "linked_work_order_id")
            .map_err(|e| decode_err("linked_work_order_id", e))?,
        revised_rpn: row
            .try_get::<Option<i64>>("", "revised_rpn")
            .map_err(|e| decode_err("revised_rpn", e))?,
        source_ram_ishikawa_diagram_id: row
            .try_get::<Option<i64>>("", "source_ram_ishikawa_diagram_id")
            .map_err(|e| decode_err("source_ram_ishikawa_diagram_id", e))?,
        source_ishikawa_flow_node_id: row
            .try_get::<Option<String>>("", "source_ishikawa_flow_node_id")
            .map_err(|e| decode_err("source_ishikawa_flow_node_id", e))?,
        review_required: row
            .try_get::<i64>("", "review_required")
            .map_err(|e| decode_err("review_required", e))?
            != 0,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_fmeca_items(db: &DatabaseConnection, analysis_id: i64) -> AppResult<Vec<FmecaItem>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                    failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                    linked_pm_plan_id, linked_work_order_id, revised_rpn,
                    source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id, review_required,
                    row_version, updated_at
             FROM fmeca_items WHERE analysis_id = ? ORDER BY id ASC",
            [analysis_id.into()],
        ))
        .await?;
    rows.iter().map(map_fmeca_item).collect()
}

pub async fn upsert_fmeca_item(db: &DatabaseConnection, input: UpsertFmecaItemInput) -> AppResult<FmecaItem> {
    validate_sod(input.severity, input.occurrence, input.detectability)?;
    ensure_work_failure_modes_published_and_active_exists(db).await?;
    if let Some(failure_mode_id) = input.failure_mode_id {
        ensure_failure_mode_id_governed(db, failure_mode_id).await?;
    }
    let rpn = input.severity * input.occurrence * input.detectability;
    let now = Utc::now().to_rfc3339();
    let ff = input.functional_failure.clone().unwrap_or_default();
    let fe = input.failure_effect.clone().unwrap_or_default();
    let ra = input.recommended_action.clone().unwrap_or_default();
    let cc = input.current_control.clone().unwrap_or_default();
    let nid = if let Some(id) = input.id {
        let exp = input.expected_row_version.ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INV_ROW_VERSION_REQUIRED: expected_row_version required.".into()
            ])
        })?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE fmeca_items SET
                    component_id = ?, functional_failure = ?, failure_mode_id = ?, failure_effect = ?,
                    severity = ?, occurrence = ?, detectability = ?, rpn = ?,
                    recommended_action = ?, current_control = ?,
                    linked_pm_plan_id = ?, linked_work_order_id = ?, revised_rpn = ?,
                    row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    input
                        .component_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    ff.into(),
                    input
                        .failure_mode_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    fe.into(),
                    input.severity.into(),
                    input.occurrence.into(),
                    input.detectability.into(),
                    rpn.into(),
                    ra.into(),
                    cc.into(),
                    input
                        .linked_pm_plan_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .linked_work_order_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .revised_rpn
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: fmeca_items update conflict.".into(),
            ]));
        }
        id
    } else {
        if let Some(did) = input.source_ram_ishikawa_diagram_id {
            let d_exists = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM ram_ishikawa_diagrams WHERE id = ?",
                    [did.into()],
                ))
                .await?;
            if d_exists.is_none() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "GATE_REL_ISHIKAWA_UNKNOWN:diagram_id={did}: Ishikawa diagram not found (source_ram_ishikawa_diagram_id={did})."
                )]));
            }
        }
        let eid = format!("fmeca_item:{}", Uuid::new_v4());
        let inserted = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO fmeca_items (
                entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn,
                recommended_action, current_control, linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
                [
                    eid.into(),
                    input.analysis_id.into(),
                    input
                        .component_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    ff.into(),
                    input
                        .failure_mode_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    fe.into(),
                    input.severity.into(),
                    input.occurrence.into(),
                    input.detectability.into(),
                    rpn.into(),
                    ra.into(),
                    cc.into(),
                    input
                        .linked_pm_plan_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .linked_work_order_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .revised_rpn
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .source_ram_ishikawa_diagram_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input
                        .source_ishikawa_flow_node_id
                        .clone()
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                    now.into(),
                ],
            ))
            .await?;
        crate::db::inserted_id(&inserted)?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                    failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                    linked_pm_plan_id, linked_work_order_id, revised_rpn,
                    source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id, review_required,
                    row_version, updated_at
             FROM fmeca_items WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("fmeca_items row missing.".into()))?;
    map_fmeca_item(&row)
}

pub async fn delete_fmeca_item(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM fmeca_items WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_rcm_study(row: &sea_orm::QueryResult) -> AppResult<RcmStudy> {
    Ok(RcmStudy {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_rcm_studies(db: &DatabaseConnection, filter: RcmStudiesFilter) -> AppResult<Vec<RcmStudy>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
         FROM rcm_studies WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_rcm_study).collect()
}

pub async fn create_rcm_study(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: CreateRcmStudyInput,
) -> AppResult<RcmStudy> {
    let now = Utc::now().to_rfc3339();
    let eid = format!("rcm_study:{}", Uuid::new_v4());
    let status = input.status.unwrap_or_else(|| "draft".into());
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO rcm_studies (
            entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
        ) VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
            [
                eid.into(),
                input.equipment_id.into(),
                input.title.into(),
                status.into(),
                now.clone().into(),
                user_id
                    .map(i64::from)
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                now.into(),
            ],
        ))
        .await?;
    let id = crate::db::inserted_id(&inserted)?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
             FROM rcm_studies WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("rcm_studies row missing.".into()))?;
    map_rcm_study(&row)
}

pub async fn update_rcm_study(db: &DatabaseConnection, input: UpdateRcmStudyInput) -> AppResult<RcmStudy> {
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rcm_studies SET
                title = COALESCE(?, title),
                status = COALESCE(?, status),
                row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input
                    .title
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input
                    .status
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                Utc::now().to_rfc3339().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: rcm_studies update conflict.".into(),
        ]));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
             FROM rcm_studies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "RcmStudy".into(),
            id: input.id.to_string(),
        })?;
    map_rcm_study(&row)
}

pub async fn delete_rcm_study(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM rcm_studies WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_rcm_decision(row: &sea_orm::QueryResult) -> AppResult<RcmDecision> {
    Ok(RcmDecision {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        study_id: row.try_get("", "study_id").map_err(|e| decode_err("study_id", e))?,
        function_description: row
            .try_get("", "function_description")
            .map_err(|e| decode_err("function_description", e))?,
        functional_failure: row
            .try_get("", "functional_failure")
            .map_err(|e| decode_err("functional_failure", e))?,
        failure_mode_id: row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?,
        consequence_category: row
            .try_get("", "consequence_category")
            .map_err(|e| decode_err("consequence_category", e))?,
        selected_tactic: row
            .try_get("", "selected_tactic")
            .map_err(|e| decode_err("selected_tactic", e))?,
        justification: row
            .try_get("", "justification")
            .map_err(|e| decode_err("justification", e))?,
        review_due_at: row
            .try_get::<Option<String>>("", "review_due_at")
            .map_err(|e| decode_err("review_due_at", e))?,
        linked_pm_plan_id: row
            .try_get::<Option<i64>>("", "linked_pm_plan_id")
            .map_err(|e| decode_err("linked_pm_plan_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_rcm_decisions(db: &DatabaseConnection, study_id: i64) -> AppResult<Vec<RcmDecision>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                    consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                    row_version, updated_at
             FROM rcm_decisions WHERE study_id = ? ORDER BY id ASC",
            [study_id.into()],
        ))
        .await?;
    rows.iter().map(map_rcm_decision).collect()
}

const RCM_TACTICS: &[&str] = &[
    "condition_based",
    "time_based",
    "failure_finding",
    "run_to_failure",
    "redesign",
];

fn validate_tactic(t: &str) -> AppResult<()> {
    if RCM_TACTICS.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "GATE_REL_TACTIC_INVALID: selected_tactic must be one of {:?}",
            RCM_TACTICS
        )]))
    }
}

pub async fn upsert_rcm_decision(db: &DatabaseConnection, input: UpsertRcmDecisionInput) -> AppResult<RcmDecision> {
    validate_tactic(&input.selected_tactic)?;
    let now = Utc::now().to_rfc3339();
    let fd = input.function_description.clone().unwrap_or_default();
    let ff = input.functional_failure.clone().unwrap_or_default();
    let cc = input.consequence_category.clone().unwrap_or_default();
    let jus = input.justification.clone().unwrap_or_default();
    let nid = if let Some(id) = input.id {
        let exp = input.expected_row_version.ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INV_ROW_VERSION_REQUIRED: expected_row_version required.".into()
            ])
        })?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE rcm_decisions SET
                    function_description = ?, functional_failure = ?, failure_mode_id = ?,
                    consequence_category = ?, selected_tactic = ?, justification = ?,
                    review_due_at = ?, linked_pm_plan_id = ?,
                    row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    fd.into(),
                    ff.into(),
                    input
                        .failure_mode_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    cc.into(),
                    input.selected_tactic.into(),
                    jus.into(),
                    input
                        .review_due_at
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                    input
                        .linked_pm_plan_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: rcm_decisions update conflict.".into(),
            ]));
        }
        id
    } else {
        let eid = format!("rcm_decision:{}", Uuid::new_v4());
        let inserted = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO rcm_decisions (
                entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
                [
                    eid.into(),
                    input.study_id.into(),
                    fd.into(),
                    ff.into(),
                    input
                        .failure_mode_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    cc.into(),
                    input.selected_tactic.into(),
                    jus.into(),
                    input
                        .review_due_at
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                    input
                        .linked_pm_plan_id
                        .map(sea_orm::Value::from)
                        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    now.into(),
                ],
            ))
            .await?;
        crate::db::inserted_id(&inserted)?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                    consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                    row_version, updated_at
             FROM rcm_decisions WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("rcm_decisions row missing.".into()))?;
    map_rcm_decision(&row)
}

pub async fn delete_rcm_decision(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM rcm_decisions WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

const DEFAULT_FMECA_RPN_CRITICAL: i64 = 150;
const SETTING_KEY_FMECA_RPN_CRITICAL: &str = "ram.fmeca_rpn_critical_threshold";

fn parse_json_int_setting(json: &str, default: i64) -> i64 {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return default;
    }
    if let Ok(v) = serde_json::from_str::<i64>(trimmed) {
        return v.clamp(1, 1_000_000);
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        match val {
            serde_json::Value::Number(n) => {
                return n
                    .as_i64()
                    .or_else(|| n.as_f64().map(|f| f.round() as i64))
                    .unwrap_or(default)
                    .clamp(1, 1_000_000);
            }
            serde_json::Value::Object(o) => {
                if let Some(v) = o.get("value") {
                    if let Some(i) = v.as_i64() {
                        return i.clamp(1, 1_000_000);
                    }
                    if let Some(f) = v.as_f64() {
                        return (f.round() as i64).clamp(1, 1_000_000);
                    }
                }
            }
            _ => {}
        }
    }
    default
}

/// Tenant-configurable FMECA RPN threshold for spare-line / inventory checks (`app_settings`).
pub async fn fmeca_rpn_critical_threshold_i64(db: &DatabaseConnection) -> AppResult<i64> {
    fmeca_rpn_critical_threshold_i64_in(db).await
}

/// The same setting read on any connection (a transaction included: the re-check of a received
/// close runs inside the apply transaction, `wo::closeout_sync`).
pub(crate) async fn fmeca_rpn_critical_threshold_i64_in(db: &impl ConnectionTrait) -> AppResult<i64> {
    let row = settings::get_setting(db, SETTING_KEY_FMECA_RPN_CRITICAL, "tenant").await?;
    let Some(s) = row else {
        return Ok(DEFAULT_FMECA_RPN_CRITICAL);
    };
    Ok(parse_json_int_setting(
        &s.setting_value_json,
        DEFAULT_FMECA_RPN_CRITICAL,
    ))
}

fn weibull_r(beta: f64, eta: f64, t: f64) -> Option<f64> {
    if beta <= 0.0 || eta <= 0.0 || t < 0.0 || !t.is_finite() {
        return None;
    }
    Some((-(t / eta).powf(beta)).exp())
}

fn weibull_residual_median_hours(beta: f64, eta: f64, t: f64) -> Option<f64> {
    if beta <= 0.0 || eta <= 0.0 || !t.is_finite() {
        return None;
    }
    let t0 = t.max(0.0);
    let tb = t0.powf(beta);
    let inner = tb + eta.powf(beta) * std::f64::consts::LN_2;
    if inner <= 0.0 {
        return Some(0.0);
    }
    let x = inner.powf(1.0 / beta) - t0;
    Some(x.max(0.0))
}

fn build_weibull_curve_points(
    beta: f64,
    eta: f64,
    beta_low: Option<f64>,
    beta_high: Option<f64>,
    eta_low: Option<f64>,
    eta_high: Option<f64>,
    t_max: f64,
    n: usize,
) -> Vec<WeibullCurvePoint> {
    let bl = beta_low.unwrap_or(beta).max(0.0001);
    let bh = beta_high.unwrap_or(beta).max(0.0001);
    let el = eta_low.unwrap_or(eta).max(0.0001);
    let eh = eta_high.unwrap_or(eta).max(0.0001);
    let m = n.max(10);
    let mut out = Vec::with_capacity(m + 1);
    for i in 0..=m {
        let t = (t_max * i as f64) / m as f64;
        let r = weibull_r(beta, eta, t).unwrap_or(0.0);
        let r_a = weibull_r(bl, el, t).unwrap_or(r);
        let r_b = weibull_r(bh, eh, t).unwrap_or(r);
        let r_low = r_a.min(r_b).clamp(0.0, 1.0);
        let r_high = r_a.max(r_b).clamp(0.0, 1.0);
        out.push(WeibullCurvePoint { t, r, r_low, r_high });
    }
    out
}

async fn industrial_beta_reference_for_equipment_class(
    db: &DatabaseConnection,
    class_code: Option<String>,
) -> AppResult<(Option<f64>, Option<String>)> {
    let Some(code) = class_code else {
        return Ok((None, None));
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT beta_reference, standard_code, source_document, revision_tag
             FROM reliability_beta_benchmarks
             WHERE UPPER(TRIM(equipment_class_code)) = UPPER(TRIM(?)) AND is_active = 1
             ORDER BY id DESC
             LIMIT 1",
            [code.into()],
        ))
        .await?;
    let Some(r) = row else {
        return Ok((None, None));
    };
    let beta_reference: f64 = r
        .try_get("", "beta_reference")
        .map_err(|e| decode_err("beta_reference", e))?;
    let standard_code: String = r
        .try_get("", "standard_code")
        .map_err(|e| decode_err("standard_code", e))?;
    let source_document: String = r
        .try_get("", "source_document")
        .map_err(|e| decode_err("source_document", e))?;
    let revision_tag: Option<String> = r
        .try_get("", "revision_tag")
        .map_err(|e| decode_err("revision_tag", e))?;
    let source = if let Some(rev) = revision_tag {
        format!("{standard_code} / {source_document} / {rev}")
    } else {
        format!("{standard_code} / {source_document}")
    };
    Ok((Some(beta_reference), Some(source)))
}

async fn load_reliability_dashboard_policy(db: &DatabaseConnection) -> AppResult<(f64, f64, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT danger_threshold_r, pm_threshold_r, pm_label
             FROM reliability_dashboard_policies
             WHERE scope_code = 'global'
             LIMIT 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::SyncError("Missing reliability_dashboard_policies row for scope_code='global'".into())
        })?;
    let danger_threshold_r: f64 = row
        .try_get("", "danger_threshold_r")
        .map_err(|e| decode_err("danger_threshold_r", e))?;
    let pm_threshold_r: f64 = row
        .try_get("", "pm_threshold_r")
        .map_err(|e| decode_err("pm_threshold_r", e))?;
    let pm_label: String = row.try_get("", "pm_label").map_err(|e| decode_err("pm_label", e))?;
    if !(0.0..=1.0).contains(&danger_threshold_r) || !(0.0..1.0).contains(&pm_threshold_r) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_REL_DASHBOARD_THRESHOLDS_INVALID: reliability_dashboard_policies.danger_threshold_r must be in [0.0, 1.0], pm_threshold_r in (0.0, 1.0)"
                .into(),
        ]));
    }
    Ok((danger_threshold_r, pm_threshold_r, pm_label))
}

async fn sum_spare_stock_for_work_order(db: &DatabaseConnection, wo_id: i64) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(sb.available_qty), 0.0) AS q
             FROM work_order_parts wop
             LEFT JOIN stock_balances sb ON sb.article_id = wop.article_id
             WHERE wop.work_order_id = ? AND wop.article_id IS NOT NULL
               AND wop.consumption_status <> 'cancelled'",
            [wo_id.into()],
        ))
        .await?;
    let r = row.ok_or_else(|| AppError::SyncError("spare stock aggregate missing.".into()))?;
    r.try_get::<f64>("", "q").map_err(|e| decode_err("q", e))
}

async fn work_order_has_spare_lines(db: &DatabaseConnection, wo_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_order_parts WHERE work_order_id = ? AND article_id IS NOT NULL \
             AND consumption_status <> 'cancelled'",
            [wo_id.into()],
        ))
        .await?;
    let c: i64 = row
        .ok_or_else(|| AppError::SyncError("work_order_parts count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    Ok(c > 0)
}

pub async fn get_fmeca_severity_occurrence_matrix(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<FmecaSeverityOccurrenceMatrix> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT fi.severity, fi.occurrence, COUNT(*) AS c
             FROM fmeca_items fi
             INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id
             WHERE fa.equipment_id = ? AND fi.review_required = 0
             GROUP BY fi.severity, fi.occurrence",
            [equipment_id.into()],
        ))
        .await?;
    let mut map: HashMap<(i64, i64), i64> = HashMap::new();
    for r in &rows {
        let s: i64 = r.try_get("", "severity").map_err(|e| decode_err("severity", e))?;
        let o: i64 = r.try_get("", "occurrence").map_err(|e| decode_err("occurrence", e))?;
        let c: i64 = r.try_get("", "c").map_err(|e| decode_err("c", e))?;
        map.insert((s, o), c);
    }
    let mut cells: Vec<FmecaSoCell> = Vec::with_capacity(100);
    for s in 1_i64..=10 {
        for o in 1_i64..=10 {
            let count = map.get(&(s, o)).copied().unwrap_or(0);
            cells.push(FmecaSoCell {
                severity: s,
                occurrence: o,
                count,
            });
        }
    }

    let ref_count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM reference_values rv
             INNER JOIN reference_sets rs ON rs.id = rv.set_id
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
               AND rs.status = 'published'
               AND rv.is_active = 1",
            [],
        ))
        .await?;
    let reference_modes_published_count: i64 = ref_count_row
        .ok_or_else(|| AppError::SyncError("reference mode count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("reference_mode_count", e))?;

    let fmeca_link_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM fmeca_items fi
             INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id
             WHERE fa.equipment_id = ? AND fi.failure_mode_id IS NOT NULL",
            [equipment_id.into()],
        ))
        .await?;
    let fmeca_mode_links_count: i64 = fmeca_link_row
        .ok_or_else(|| AppError::SyncError("fmeca mode link count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("fmeca_mode_links_count", e))?;

    let orphan_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM fmeca_items fi
             INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id
             LEFT JOIN reference_values rv ON rv.id = fi.failure_mode_id
             LEFT JOIN reference_sets rs ON rs.id = rv.set_id
             LEFT JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE fa.equipment_id = ? AND fi.failure_mode_id IS NOT NULL
               AND (
                 rv.id IS NULL
                 OR rs.status <> 'published'
                 OR UPPER(TRIM(rd.code)) <> UPPER(TRIM('WORK.FAILURE_MODES'))
               )",
            [equipment_id.into()],
        ))
        .await?;
    let fmeca_orphan_mode_links_count: i64 = orphan_row
        .ok_or_else(|| AppError::SyncError("fmeca orphan mode count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("fmeca_orphan_mode_links_count", e))?;

    let reference_domain_ready = reference_modes_published_count > 0;
    let (warning_code, warning_message) = if !reference_domain_ready && fmeca_mode_links_count > 0 {
        (
            Some("reference_modes_missing_published_set".to_string()),
            Some(
                "FMECA has linked failure modes but WORK.FAILURE_MODES has no published active values. Publish reference values to restore governance consistency."
                    .to_string(),
            ),
        )
    } else if fmeca_orphan_mode_links_count > 0 {
        (
            Some("fmeca_mode_links_orphaned".to_string()),
            Some("Some FMECA failure_mode_id links are orphaned/outside published WORK.FAILURE_MODES.".to_string()),
        )
    } else {
        (None, None)
    };

    Ok(FmecaSeverityOccurrenceMatrix {
        equipment_id,
        cells,
        reference_domain_ready,
        reference_modes_published_count,
        fmeca_mode_links_count,
        fmeca_orphan_mode_links_count,
        warning_code,
        warning_message,
    })
}

fn map_fmeca_item_with_ctx(row: &sea_orm::QueryResult) -> AppResult<FmecaItemWithContext> {
    let item = map_fmeca_item(row)?;
    let analysis_title: String = row
        .try_get("", "analysis_title")
        .map_err(|e| decode_err("analysis_title", e))?;
    let equipment_ctx: i64 = row
        .try_get("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
    Ok(FmecaItemWithContext {
        item,
        analysis_title,
        equipment_id: equipment_ctx,
        spare_stock_total: None,
        inventory_status: "not_applicable".into(),
    })
}

pub async fn list_fmeca_items_for_equipment(
    db: &DatabaseConnection,
    filter: FmecaItemsEquipmentFilter,
) -> AppResult<Vec<FmecaItemWithContext>> {
    let lim = filter.limit.unwrap_or(500).clamp(1, 2000);
    let mut sql = String::from(
        "SELECT fi.id AS id, fi.entity_sync_id AS entity_sync_id, fi.analysis_id AS analysis_id, \
         fi.component_id AS component_id, fi.functional_failure AS functional_failure, \
         fi.failure_mode_id AS failure_mode_id, fi.failure_effect AS failure_effect, \
         fi.severity AS severity, fi.occurrence AS occurrence, fi.detectability AS detectability, \
         fi.rpn AS rpn, fi.recommended_action AS recommended_action, fi.current_control AS current_control, \
         fi.linked_pm_plan_id AS linked_pm_plan_id, fi.linked_work_order_id AS linked_work_order_id, \
         fi.revised_rpn AS revised_rpn, \
         fi.source_ram_ishikawa_diagram_id AS source_ram_ishikawa_diagram_id, \
         fi.source_ishikawa_flow_node_id AS source_ishikawa_flow_node_id, \
         fi.review_required AS review_required, \
         fi.row_version AS row_version, fi.updated_at AS updated_at, \
         fa.title AS analysis_title, fa.equipment_id AS equipment_id \
         FROM fmeca_items fi \
         INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id \
         WHERE fa.equipment_id = ?",
    );
    let mut vals: Vec<sea_orm::Value> = vec![filter.equipment_id.into()];
    if let Some(sv) = filter.severity {
        sql.push_str(" AND fi.severity = ?");
        vals.push(sv.into());
    }
    if let Some(ov) = filter.occurrence {
        sql.push_str(" AND fi.occurrence = ?");
        vals.push(ov.into());
    }
    // Rows to review come after the ranked ones: their RPN was never chosen by a person.
    sql.push_str(" ORDER BY fi.review_required ASC, fi.rpn DESC, fi.id ASC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    let mut out: Vec<FmecaItemWithContext> = rows
        .iter()
        .map(map_fmeca_item_with_ctx)
        .collect::<Result<Vec<_>, _>>()?;
    let rpn_critical = fmeca_rpn_critical_threshold_i64(db).await?;
    for row in &mut out {
        if row.item.review_required || row.item.rpn <= rpn_critical {
            row.inventory_status = "not_applicable".into();
            continue;
        }
        let Some(wo_id) = row.item.linked_work_order_id else {
            row.inventory_status = "no_wo_link".into();
            continue;
        };
        if !work_order_has_spare_lines(db, wo_id).await? {
            row.inventory_status = "no_spare_lines".into();
            continue;
        }
        let qty = sum_spare_stock_for_work_order(db, wo_id).await?;
        row.spare_stock_total = Some(qty);
        row.inventory_status = if qty <= 0.0 {
            "critical_shortage".into()
        } else {
            "ok".into()
        };
    }
    Ok(out)
}

fn map_suggested_part_for_failure(row: &sea_orm::QueryResult) -> AppResult<SuggestedPartForFailure> {
    Ok(SuggestedPartForFailure {
        article_id: row.try_get("", "article_id").map_err(|e| decode_err("article_id", e))?,
        article_code: row
            .try_get("", "article_code")
            .map_err(|e| decode_err("article_code", e))?,
        article_name: row
            .try_get("", "article_name")
            .map_err(|e| decode_err("article_name", e))?,
        suggested_quantity: row
            .try_get("", "suggested_quantity")
            .map_err(|e| decode_err("suggested_quantity", e))?,
        priority: row.try_get("", "priority").map_err(|e| decode_err("priority", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        stock_on_hand: row
            .try_get("", "stock_on_hand")
            .map_err(|e| decode_err("stock_on_hand", e))?,
        stock_available: row
            .try_get("", "stock_available")
            .map_err(|e| decode_err("stock_available", e))?,
    })
}

pub async fn get_suggested_parts_for_failure(
    db: &DatabaseConnection,
    input: SuggestedPartsForFailureInput,
) -> AppResult<Vec<SuggestedPartForFailure>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                a.id AS article_id,
                a.article_code AS article_code,
                a.article_name AS article_name,
                COALESCE(SUM(fip.quantity_hint), 0.0) AS suggested_quantity,
                MIN(fip.priority) AS priority,
                GROUP_CONCAT(DISTINCT NULLIF(TRIM(fip.notes), '')) AS notes,
                COALESCE(SUM(sb.on_hand_qty), 0.0) AS stock_on_hand,
                COALESCE(SUM(sb.available_qty), 0.0) AS stock_available
             FROM fmeca_analyses fa
             INNER JOIN fmeca_items fi ON fi.analysis_id = fa.id
             INNER JOIN fmeca_item_parts fip ON fip.fmeca_item_id = fi.id
             INNER JOIN articles a ON a.id = fip.article_id
             LEFT JOIN stock_balances sb ON sb.article_id = a.id
             WHERE fa.equipment_id = ? AND fi.failure_mode_id = ?
             GROUP BY a.id, a.article_code, a.article_name
             ORDER BY priority ASC, suggested_quantity DESC, a.article_name ASC",
            [input.equipment_id.into(), input.failure_mode_id.into()],
        ))
        .await?;
    rows.iter().map(map_suggested_part_for_failure).collect()
}

async fn readiness_for_live_values(db: &DatabaseConnection, equipment_id: i64) -> AppResult<ReliabilityReadinessGate> {
    evaluate_equipment_readiness(
        db,
        ReliabilityReadinessInput {
            equipment_id,
            period_start: None,
            period_end: None,
            min_sample_n: None,
        },
    )
    .await
}

/// R(t) and median residual life at the current exposure age. Withheld when the readiness gate
/// denies the equipment, when no exposure-hour fit exists, or when no exposure basis exists.
pub async fn get_reliability_rul_indicator(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<ReliabilityRulIndicator> {
    let gate = readiness_for_live_values(db, equipment_id).await?;
    let empty = |message: String| ReliabilityRulIndicator {
        equipment_id,
        weibull_beta: None,
        weibull_eta_hours: None,
        reliability_at_t: None,
        predicted_rul_hours: None,
        t_hours: None,
        message,
        readiness_decision: gate.decision.clone(),
        blocking_reasons: gate.blocking_reasons.clone(),
    };
    if !gate.allowed() {
        return Ok(empty(format!(
            "Blocked by readiness gate: {}",
            gate.blocking_reasons.join(", ")
        )));
    }
    let Some(basis) = load_exposure_basis(db, equipment_id).await? else {
        return Ok(empty("No governed exposure basis for this equipment.".into()));
    };
    let Some(fit) = latest_usable_weibull_fit(db, equipment_id).await? else {
        return Ok(empty(
            "No adequate exposure-hour Weibull fit for this equipment; run a fit first.".into(),
        ));
    };
    let (beta, eta) = (fit.beta.unwrap_or(0.0), fit.eta.unwrap_or(0.0));
    let Some(t_eff) = exposure_age_hours(db, equipment_id, fit.failure_mode_id, &basis).await? else {
        return Ok(empty("No eligible failure to anchor the current exposure age.".into()));
    };

    Ok(ReliabilityRulIndicator {
        equipment_id,
        weibull_beta: Some(beta),
        weibull_eta_hours: Some(eta),
        reliability_at_t: weibull_r(beta, eta, t_eff),
        predicted_rul_hours: weibull_residual_median_hours(beta, eta, t_eff),
        t_hours: Some(t_eff),
        message: format!(
            "t = {:.1} h exposure since last restoration ({}); R(t) and RUL from Weibull fit (β={:.4}, η={:.1} h).",
            t_eff,
            basis.source(),
            beta,
            eta
        ),
        readiness_decision: gate.decision,
        blocking_reasons: gate.blocking_reasons,
    })
}

pub async fn get_weibull_dashboard_payload(
    db: &DatabaseConnection,
    input: WeibullDashboardInput,
) -> AppResult<WeibullDashboardPayload> {
    let include_censored = input.include_censored.unwrap_or(true);
    let t_offset_hours = input.t_offset_hours.unwrap_or(0.0);
    let t_offset_hours = if t_offset_hours.is_finite() {
        t_offset_hours
    } else {
        0.0
    };
    let (danger_threshold_r, pm_threshold_r, pm_label) = load_reliability_dashboard_policy(db).await?;
    let gate = readiness_for_live_values(db, input.equipment_id).await?;
    let basis = load_exposure_basis(db, input.equipment_id).await?;
    let fit = if gate.allowed() {
        latest_usable_weibull_fit(db, input.equipment_id).await?
    } else {
        None
    };
    let t_base = match (&fit, &basis) {
        (Some(f), Some(b)) => exposure_age_hours(db, input.equipment_id, f.failure_mode_id, b)
            .await?
            .unwrap_or(0.0),
        _ => 0.0,
    };
    let t_effective = (t_base + t_offset_hours).max(0.0);

    let Some(main_fit) = fit else {
        return Ok(WeibullDashboardPayload {
            equipment_id: input.equipment_id,
            include_censored,
            comparison_equipment_id: input.comparison_equipment_id,
            t_offset_hours,
            t_effective_hours: t_effective,
            beta_actual: None,
            beta_industrial_standard: None,
            beta_industrial_source: None,
            eta_hours: None,
            points: Vec::new(),
            comparison_points: Vec::new(),
            pm_marker: None,
            r_live: None,
            rul_live_hours: None,
            danger_threshold_r,
            pm_threshold_r,
            readiness_decision: gate.decision,
            blocking_reasons: gate.blocking_reasons,
        });
    };

    let beta = main_fit.beta.unwrap_or(0.0);
    let eta = main_fit.eta.unwrap_or(0.0);
    let t_max = (eta * 4.0).max(t_effective * 1.3).max(1.0);
    let points = build_weibull_curve_points(
        beta,
        eta,
        main_fit.beta_ci_low,
        main_fit.beta_ci_high,
        main_fit.eta_ci_low,
        main_fit.eta_ci_high,
        t_max,
        120,
    );

    let pm_t = eta * (-(pm_threshold_r).ln()).powf(1.0 / beta.max(0.0001));
    let pm_marker = weibull_r(beta, eta, pm_t).map(|r| WeibullPmMarker {
        t: pm_t,
        r,
        label: pm_label,
    });

    let comparison_points = match input.comparison_equipment_id {
        Some(cmp_id) if cmp_id > 0 && cmp_id != input.equipment_id => {
            match latest_usable_weibull_fit(db, cmp_id).await? {
                Some(cmp_fit) => build_weibull_curve_points(
                    cmp_fit.beta.unwrap_or(0.0),
                    cmp_fit.eta.unwrap_or(0.0),
                    cmp_fit.beta_ci_low,
                    cmp_fit.beta_ci_high,
                    cmp_fit.eta_ci_low,
                    cmp_fit.eta_ci_high,
                    t_max,
                    120,
                ),
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    };

    let class_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ec.code AS class_code
             FROM equipment e
             LEFT JOIN equipment_classes ec ON ec.id = e.class_id
             WHERE e.id = ?",
            [input.equipment_id.into()],
        ))
        .await?;
    let class_code = class_row
        .and_then(|r| r.try_get::<Option<String>>("", "class_code").ok())
        .flatten();
    let (beta_standard, beta_standard_source) = industrial_beta_reference_for_equipment_class(db, class_code).await?;

    Ok(WeibullDashboardPayload {
        equipment_id: input.equipment_id,
        include_censored,
        comparison_equipment_id: input.comparison_equipment_id,
        t_offset_hours,
        t_effective_hours: t_effective,
        beta_actual: Some(beta),
        beta_industrial_standard: beta_standard,
        beta_industrial_source: beta_standard_source,
        eta_hours: Some(eta),
        points,
        comparison_points,
        pm_marker,
        r_live: weibull_r(beta, eta, t_effective),
        rul_live_hours: weibull_residual_median_hours(beta, eta, t_effective),
        danger_threshold_r,
        pm_threshold_r,
        readiness_decision: gate.decision,
        blocking_reasons: gate.blocking_reasons,
    })
}

fn map_ram_ishikawa(row: &sea_orm::QueryResult) -> AppResult<RamIshikawaDiagram> {
    Ok(RamIshikawaDiagram {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        flow_json: row.try_get("", "flow_json").map_err(|e| decode_err("flow_json", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_ram_ishikawa_diagrams(
    db: &DatabaseConnection,
    filter: RamIshikawaDiagramsFilter,
) -> AppResult<Vec<RamIshikawaDiagram>> {
    let lim = filter.limit.unwrap_or(50).clamp(1, 200);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at \
         FROM ram_ishikawa_diagrams WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_ram_ishikawa).collect()
}

pub async fn upsert_ram_ishikawa_diagram(
    db: &DatabaseConnection,
    _user_id: Option<i32>,
    input: UpsertRamIshikawaDiagramInput,
) -> AppResult<RamIshikawaDiagram> {
    let now = Utc::now().to_rfc3339();
    let _ = _user_id;
    let nid = if let Some(id) = input.id {
        let exp = input.expected_row_version.ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INV_ROW_VERSION_REQUIRED: expected_row_version required.".into()
            ])
        })?;
        let title = input.title.clone().unwrap_or_default();
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE ram_ishikawa_diagrams SET
                    title = COALESCE(?, title),
                    flow_json = ?,
                    row_version = row_version + 1,
                    updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    title.into(),
                    input.flow_json.into(),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: ram_ishikawa_diagrams update conflict.".into(),
            ]));
        }
        id
    } else {
        let eid = format!("ram_ishikawa:{}", Uuid::new_v4());
        let title = input.title.clone().unwrap_or_default();
        let inserted = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO ram_ishikawa_diagrams (
                entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at
            ) VALUES (?, ?, ?, ?, 1, ?, ?)",
                [
                    eid.into(),
                    input.equipment_id.into(),
                    title.into(),
                    input.flow_json.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        crate::db::inserted_id(&inserted)?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at
             FROM ram_ishikawa_diagrams WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("ram_ishikawa_diagrams row missing.".into()))?;
    map_ram_ishikawa(&row)
}

pub async fn delete_ram_ishikawa_diagram(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM ram_ishikawa_diagrams WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod weibull_dataset_tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        parse_ts(s).expect("ts")
    }

    /// Runtime log of 1 h at every hour mark from 00:00 to 23:00 on 2026-01-01..02 (48 increments).
    fn hourly_basis() -> ExposureBasis {
        let start = ts("2026-01-01T00:00:00Z");
        ExposureBasis::RuntimeLogs {
            increments: (0..48).map(|h| (start + chrono::Duration::hours(h), 1.0)).collect(),
        }
    }

    #[test]
    fn intervals_start_at_restoration_and_tail_is_censored() {
        let events = vec![
            WeibullSourceEvent {
                id: 1,
                failed_at: ts("2026-01-01T02:00:00Z"),
                restored_at: Some(ts("2026-01-01T05:00:00Z")),
            },
            WeibullSourceEvent {
                id: 2,
                failed_at: ts("2026-01-01T15:00:00Z"),
                restored_at: Some(ts("2026-01-01T16:00:00Z")),
            },
        ];
        let (obs, dropped) =
            weibull_observations_from_events(&events, &hourly_basis(), Some(ts("2026-01-02T00:00:00Z")));
        // (05:00, 15:00] → 10 h failure; (16:00, 24:00] → 8 h censored.
        assert_eq!(dropped, 0);
        assert_eq!(
            obs,
            vec![WeibullObservation::failure(10.0), WeibullObservation::censored(8.0)]
        );
    }

    #[test]
    fn zero_exposure_interval_is_dropped_not_fitted() {
        let events = vec![
            WeibullSourceEvent {
                id: 1,
                failed_at: ts("2026-01-01T10:10:00Z"),
                restored_at: None,
            },
            WeibullSourceEvent {
                id: 2,
                failed_at: ts("2026-01-01T10:50:00Z"),
                restored_at: None,
            },
        ];
        let (obs, dropped) = weibull_observations_from_events(&events, &hourly_basis(), None);
        assert!(obs.is_empty());
        assert_eq!(dropped, 1);
    }
}
