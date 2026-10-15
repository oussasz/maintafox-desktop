//! Step 1 of a study: the failure and suspension records of its equipment, with their ages in
//! the study time base, the analyst's choices applied, and the data checks.
//!
//! Model (stated on screen under "Method and assumptions"):
//! - each failure record (`failure_events`, unplanned) ends a life; the repair renews the item,
//!   so the next age counts from the restoration instant;
//! - the first failure of an equipment counts from the previous failure before the period (any
//!   date), or from its commissioning when that falls inside the period; otherwise its age is
//!   unknown and the record cannot be used, but its repair still starts the next age;
//! - a completed preventive work order may be a removal before failure (suspension); it is used
//!   only once the analyst confirms it replaced the item;
//! - the time from the last renewal to the end of the period is a suspension (still running);
//! - an excluded record is ignored: the next age counts from the previous renewal;
//! - in a single-mode study, failures of other modes are either ignored or, when the study says
//!   so (competing modes), suspensions of the studied mode: the item was renewed while the
//!   studied mode had not occurred.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use crate::reliability::compute::eligible_unplanned_mtbf;
use crate::reliability::study::clock::{load_clock, parse_instant, parse_instant_opt, StudyClock};
use crate::reliability::study::domain::{
    AssetClockInfo, DataCheck, ReliabilityStudy, StudyDataset, StudyRecord, RECORD_COMPETING, RECORD_END_OF_PERIOD,
    RECORD_FAILURE, RECORD_PM_SUSPENSION,
};
use crate::reliability::study::store::{
    get, get_study, load_record_history, RecordChoice, BASIS_CALENDAR, COMPETING_SUSPEND,
};
use crate::reliability::weibull_fit::WeibullObservation;

/// A DI issued more than this many hours before the repair start is reported.
const DI_LEAD_THRESHOLD_HOURS: f64 = 1.0;
/// Below this share of the period covered by run-hour data, the coverage is a warning.
const COVERAGE_WARNING: f64 = 0.9;

pub const BASIS_OPERATING: &str = "operating";

pub struct BuiltDataset {
    pub dataset: StudyDataset,
    pub observations: Vec<WeibullObservation>,
    /// Effective clock per equipment (calendar for all when the study is on calendar time).
    pub clocks: HashMap<i64, StudyClock>,
    pub period_start: DateTime<Utc>,
    /// End of the period, capped at now.
    pub period_end: DateTime<Utc>,
    /// Failures used in the analysis.
    pub failures_used: i64,
    /// Study hours of all equipment over the period.
    pub total_hours: f64,
}

struct FailureRow {
    id: i64,
    equipment_id: i64,
    base_at: DateTime<Utc>,
    from_repair_start: bool,
    restored_at: Option<DateTime<Utc>>,
    downtime: f64,
    mode_id: Option<i64>,
    mode_code: Option<String>,
    mode_label: Option<String>,
    cause_id: Option<i64>,
    cause_label: Option<String>,
    mechanism_label: Option<String>,
    cause_nd: bool,
    eligible: bool,
    wo_id: Option<i64>,
    wo_code: Option<String>,
    di_id: Option<i64>,
    di_code: Option<String>,
    di_at: Option<DateTime<Utc>>,
}

struct PmRow {
    wo_id: i64,
    wo_code: String,
    equipment_id: i64,
    done_at: DateTime<Utc>,
}

enum Item {
    Failure(FailureRow),
    Pm(PmRow),
    /// Failure of another mode (competing modes counted as suspensions).
    Competing(FailureRow),
}

/// Which failure records a query keeps.
#[derive(Clone, Copy)]
enum ModeScope {
    /// The study mode (or every mode), with uncoded records.
    Study(Option<i64>),
    /// Coded records of any other mode than the study mode.
    Other(i64),
}

fn hours(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_milliseconds() as f64 / 3_600_000.0
}

/// SQL predicate on `fe.failure_mode_id` for a study mode (the mode or its child codes);
/// `uncoded` also keeps records without a mode (they may belong to it).
fn mode_clause(uncoded: bool) -> String {
    format!(
        "(? IS NULL {} OR fe.failure_mode_id = ? OR fe.failure_mode_id IN (SELECT id FROM reference_values WHERE parent_id = ?))",
        if uncoded { "OR fe.failure_mode_id IS NULL" } else { "" }
    )
}

fn mode_values(mode: Option<i64>) -> [sea_orm::Value; 3] {
    let v = || {
        mode.map(sea_orm::Value::from)
            .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
    };
    [v(), v(), v()]
}

async fn load_failures(
    db: &DatabaseConnection,
    equipment_id: i64,
    start: &str,
    end: &str,
    mode: Option<i64>,
) -> AppResult<Vec<FailureRow>> {
    load_failures_scoped(db, equipment_id, start, end, ModeScope::Study(mode)).await
}

async fn load_failures_scoped(
    db: &DatabaseConnection,
    equipment_id: i64,
    start: &str,
    end: &str,
    scope: ModeScope,
) -> AppResult<Vec<FailureRow>> {
    let sql = format!(
        "SELECT fe.id, fe.equipment_id, fe.failed_at, COALESCE(fe.failed_at, fe.detected_at, fe.created_at) AS ts,
                fe.restored_at, fe.downtime_duration_hours, fe.failure_mode_id, fe.failure_cause_id,
                fe.cause_not_determined, fe.eligible_flags_json,
                m.code AS mode_code, m.label AS mode_label, c.label AS cause_label,
                mech.label AS mechanism_label,
                wo.id AS wo_id, wo.code AS wo_code, di.id AS di_id, di.code AS di_code,
                COALESCE(di.observed_at, di.submitted_at) AS di_at
         FROM failure_events fe
         LEFT JOIN reference_values m ON m.id = fe.failure_mode_id
         LEFT JOIN reference_values c ON c.id = fe.failure_cause_id
         LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
         LEFT JOIN intervention_requests di ON di.id = wo.source_di_id
         LEFT JOIN work_order_failure_details wfd ON wfd.id = (
              SELECT MAX(x.id) FROM work_order_failure_details x WHERE x.work_order_id = wo.id)
         LEFT JOIN reference_values mech ON mech.id = COALESCE(fe.failure_mechanism_id, wfd.failure_mechanism_id)
         WHERE fe.equipment_id = ? AND COALESCE(fe.is_planned, 0) = 0
           AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
           AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
           AND {}
         ORDER BY datetime(ts) ASC, fe.id ASC",
        match scope {
            ModeScope::Study(_) => mode_clause(true),
            ModeScope::Other(_) => format!("fe.failure_mode_id IS NOT NULL AND NOT {}", mode_clause(false)),
        }
    );
    let mut vals: Vec<sea_orm::Value> = vec![equipment_id.into(), start.into(), end.into()];
    vals.extend(mode_values(match scope {
        ModeScope::Study(m) => m,
        ModeScope::Other(m) => Some(m),
    }));
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let ts: String = get(r, "ts")?;
        let Some(base_at) = parse_instant_opt(Some(&ts)) else {
            continue;
        };
        let failed_at: Option<String> = get(r, "failed_at")?;
        let flags: String = get(r, "eligible_flags_json")?;
        let cause_nd: i64 = get(r, "cause_not_determined")?;
        let restored: Option<String> = get(r, "restored_at")?;
        let di_at: Option<String> = get(r, "di_at")?;
        out.push(FailureRow {
            id: get(r, "id")?,
            equipment_id: get(r, "equipment_id")?,
            base_at,
            from_repair_start: failed_at.is_some(),
            restored_at: parse_instant_opt(restored.as_deref()),
            downtime: get(r, "downtime_duration_hours")?,
            mode_id: get(r, "failure_mode_id")?,
            mode_code: get(r, "mode_code")?,
            mode_label: get(r, "mode_label")?,
            cause_id: get(r, "failure_cause_id")?,
            cause_label: get(r, "cause_label")?,
            mechanism_label: get(r, "mechanism_label")?,
            cause_nd: cause_nd != 0,
            eligible: eligible_unplanned_mtbf(&flags),
            wo_id: get(r, "wo_id")?,
            wo_code: get(r, "wo_code")?,
            di_id: get(r, "di_id")?,
            di_code: get(r, "di_code")?,
            di_at: parse_instant_opt(di_at.as_deref()),
        });
    }
    Ok(out)
}

/// Renewal instant before the period: the previous failure of the same mode, or commissioning
/// inside the period.
async fn initial_renewal(
    db: &DatabaseConnection,
    equipment_id: i64,
    start: &str,
    mode: Option<i64>,
    period_start: DateTime<Utc>,
) -> AppResult<Option<(DateTime<Utc>, &'static str)>> {
    let sql = format!(
        "SELECT COALESCE(fe.restored_at, fe.failed_at, fe.detected_at, fe.created_at) AS renewal
         FROM failure_events fe
         WHERE fe.equipment_id = ? AND COALESCE(fe.is_planned, 0) = 0
           AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) < datetime(?)
           AND {}
         ORDER BY datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) DESC, fe.id DESC LIMIT 1",
        mode_clause(false)
    );
    let mut vals: Vec<sea_orm::Value> = vec![equipment_id.into(), start.into()];
    vals.extend(mode_values(mode));
    if let Some(r) = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?
    {
        let renewal: String = get(&r, "renewal")?;
        if let Some(t) = parse_instant_opt(Some(&renewal)) {
            return Ok(Some((t, "previous_failure")));
        }
    }
    let eq = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT commissioning_date FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?;
    if let Some(r) = eq {
        let c: Option<String> = get(&r, "commissioning_date")?;
        let parsed = c.as_deref().and_then(|s| {
            parse_instant_opt(Some(s)).or_else(|| {
                chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|n| n.and_utc())
            })
        });
        if let Some(t) = parsed.filter(|t| *t >= period_start) {
            return Ok(Some((t, "commissioning")));
        }
    }
    Ok(None)
}

async fn load_pm_removals(db: &DatabaseConnection, equipment_id: i64, start: &str, end: &str) -> AppResult<Vec<PmRow>> {
    // Preventive renewals: work orders of a preventive nature (EN 13306).
    let preventive = crate::wo::types::nature_sql_list(&crate::wo::types::MaintenanceNature::PREVENTIVE);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, wo.code, COALESCE(wo.actual_end, wo.mechanically_completed_at, wo.closed_at) AS done_at
             FROM work_orders wo
             JOIN reference_values t ON t.id = wo.type_id
             JOIN work_order_statuses st ON st.id = wo.status_id
             WHERE wo.equipment_id = ? AND t.semantic_tag IN ({preventive})
               AND (st.macro_state IN ('completed', 'closed') OR st.code IN ('completed', 'closed'))
               AND COALESCE(wo.actual_end, wo.mechanically_completed_at, wo.closed_at) IS NOT NULL
               AND datetime(COALESCE(wo.actual_end, wo.mechanically_completed_at, wo.closed_at)) >= datetime(?)
               AND datetime(COALESCE(wo.actual_end, wo.mechanically_completed_at, wo.closed_at)) <= datetime(?)
             ORDER BY datetime(done_at) ASC, wo.id ASC"
            ),
            [equipment_id.into(), start.into(), end.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let done: String = get(r, "done_at")?;
        if let Some(done_at) = parse_instant_opt(Some(&done)) {
            out.push(PmRow {
                wo_id: get(r, "id")?,
                wo_code: get(r, "code")?,
                equipment_id,
                done_at,
            });
        }
    }
    Ok(out)
}

/// Effective time basis and the clock of every equipment of the study.
pub async fn study_clocks(
    db: &DatabaseConnection,
    study: &ReliabilityStudy,
) -> AppResult<(bool, HashMap<i64, StudyClock>, HashMap<i64, StudyClock>)> {
    let mut own = HashMap::new();
    for a in &study.assets {
        own.insert(a.equipment_id, load_clock(db, a.equipment_id).await?);
    }
    let operating = study.time_basis != BASIS_CALENDAR && !own.is_empty() && own.values().all(StudyClock::is_operating);
    let effective = own
        .iter()
        .map(|(id, c)| (*id, if operating { c.clone() } else { StudyClock::Calendar }))
        .collect();
    Ok((operating, own, effective))
}

#[allow(clippy::too_many_lines)]
pub async fn build_dataset(db: &DatabaseConnection, study_id: i64) -> AppResult<BuiltDataset> {
    let study = get_study(db, study_id).await?;
    let period_start = parse_instant(&study.period_start)?;
    let now = Utc::now();
    let period_end = {
        let e = parse_instant(&study.period_end)?;
        if e > now {
            now
        } else {
            e
        }
    };
    let (start_s, end_s) = (period_start.to_rfc3339(), period_end.to_rfc3339());
    let (operating, own_clocks, clocks) = study_clocks(db, &study).await?;
    let (choices, history) = load_record_history(db, study_id).await?;
    let mode = study.failure_mode_id;
    let competing = mode.is_some() && study.competing_modes == COMPETING_SUSPEND;

    let mut records: Vec<StudyRecord> = Vec::new();
    let mut observations: Vec<WeibullObservation> = Vec::new();
    let mut checks: Vec<DataCheck> = Vec::new();
    let mut clock_infos = Vec::new();
    let mut total_hours = 0.0;
    let mut no_start_count = 0_i64;
    let mut pm_to_confirm = 0_i64;
    let empty = RecordChoice::default();

    for asset in &study.assets {
        let clock = clocks.get(&asset.equipment_id).cloned().unwrap_or(StudyClock::Calendar);
        let own = own_clocks
            .get(&asset.equipment_id)
            .cloned()
            .unwrap_or(StudyClock::Calendar);
        let coverage = own.coverage(period_start, period_end);
        let period_hours = clock.hours_between(period_start, period_end);
        total_hours += period_hours;
        clock_infos.push(AssetClockInfo {
            equipment_id: asset.equipment_id,
            asset_code: asset.asset_code.clone(),
            source: own.source().to_string(),
            coverage,
            period_hours,
            calendar_hours: hours(period_start, period_end).max(0.0),
        });
        let base_check = |kind: &str, severity: &str| DataCheck {
            kind: kind.into(),
            severity: severity.into(),
            record_key: None,
            equipment_id: Some(asset.equipment_id),
            asset_code: Some(asset.asset_code.clone()),
            work_order_id: None,
            work_order_code: None,
            di_code: None,
            hours: None,
            count: None,
            coverage: Some(coverage),
        };
        if own.is_operating() {
            checks.push(base_check(
                "operating_hours",
                if coverage >= COVERAGE_WARNING {
                    "success"
                } else {
                    "warning"
                },
            ));
        } else {
            checks.push(base_check("no_operating_hours", "danger"));
        }

        let mut items: Vec<(DateTime<Utc>, Item)> = Vec::new();
        for f in load_failures(db, asset.equipment_id, &start_s, &end_s, mode).await? {
            let key = format!("failure:{}", f.id);
            let use_di = choices.get(&key).is_some_and(|c| c.use_di_time);
            let at = match (use_di, f.di_at) {
                (true, Some(d)) if d < f.base_at => d,
                _ => f.base_at,
            };
            items.push((at, Item::Failure(f)));
        }
        for p in load_pm_removals(db, asset.equipment_id, &start_s, &end_s).await? {
            items.push((p.done_at, Item::Pm(p)));
        }
        if let (Some(m), true) = (mode, competing) {
            for f in load_failures_scoped(db, asset.equipment_id, &start_s, &end_s, ModeScope::Other(m)).await? {
                items.push((f.base_at, Item::Competing(f)));
            }
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));

        // With competing modes, a repair of any mode renews the item.
        let renewal_mode = if competing { None } else { mode };
        let mut start = initial_renewal(db, asset.equipment_id, &start_s, renewal_mode, period_start)
            .await?
            .map(|(t, _)| t);
        for (at, item) in items {
            let age = start.map(|s| clock.hours_between(s, at));
            let usable_age = age.filter(|h| h.is_finite() && *h > 0.0);
            match item {
                Item::Failure(f) => {
                    let key = format!("failure:{}", f.id);
                    let choice = choices.get(&key).unwrap_or(&empty);
                    let mut flags = Vec::new();
                    let di_lead = f
                        .di_at
                        .map(|d| hours(d, f.base_at))
                        .filter(|h| *h > DI_LEAD_THRESHOLD_HOURS);
                    if di_lead.is_some() {
                        flags.push("di_earlier".to_string());
                    }
                    if f.mode_id.is_none() {
                        flags.push("mode_missing".to_string());
                    } else if f.cause_id.is_none() && !f.cause_nd {
                        flags.push("cause_missing".to_string());
                    } else if f.mechanism_label.is_none() {
                        flags.push("mechanism_missing".to_string());
                    }
                    if start.is_none() {
                        flags.push("no_known_start".to_string());
                    }
                    let default_in = f.eligible;
                    let wanted = choice.include.unwrap_or(default_in);
                    let included = wanted && usable_age.is_some();
                    let state_reason = if usable_age.is_none() {
                        no_start_count += i64::from(start.is_none());
                        Some(if start.is_none() { "no_known_start" } else { "zero_age" })
                    } else if choice.include == Some(false) {
                        Some("analyst")
                    } else if choice.include == Some(true) && !default_in {
                        Some("analyst_included")
                    } else if !default_in {
                        Some("not_eligible")
                    } else {
                        None
                    };
                    let used_di = choice.use_di_time && f.di_at.is_some_and(|d| d < f.base_at);
                    for flag in &flags {
                        if flag == "no_known_start" {
                            continue;
                        }
                        checks.push(DataCheck {
                            kind: flag.clone(),
                            severity: "warning".into(),
                            record_key: Some(key.clone()),
                            equipment_id: Some(asset.equipment_id),
                            asset_code: Some(asset.asset_code.clone()),
                            work_order_id: f.wo_id,
                            work_order_code: f.wo_code.clone(),
                            di_code: f.di_code.clone(),
                            hours: if flag == "di_earlier" { di_lead } else { None },
                            count: None,
                            coverage: None,
                        });
                    }
                    // A repair renews the item even when its own age is unknown (first record).
                    if included || (wanted && start.is_none()) {
                        if included {
                            observations.push(WeibullObservation::failure(usable_age.unwrap_or_default()));
                        }
                        start = Some(match f.restored_at {
                            Some(r) if r > at => r,
                            _ => at,
                        });
                    }
                    records.push(StudyRecord {
                        key,
                        kind: RECORD_FAILURE.into(),
                        equipment_id: f.equipment_id,
                        asset_code: asset.asset_code.clone(),
                        failure_event_id: Some(f.id),
                        work_order_id: f.wo_id,
                        work_order_code: f.wo_code,
                        di_id: f.di_id,
                        di_code: f.di_code,
                        event_at: at.to_rfc3339(),
                        event_time_source: if used_di {
                            "di"
                        } else if f.from_repair_start {
                            "repair_start"
                        } else {
                            "detection"
                        }
                        .into(),
                        restored_at: f.restored_at.map(|r| r.to_rfc3339()),
                        age_hours: usable_age,
                        included,
                        state_reason: state_reason.map(str::to_string),
                        analyst_note: choice.note.clone(),
                        failure_mode_code: f.mode_code,
                        failure_mode_label: f.mode_label,
                        failure_cause_label: f.cause_label,
                        failure_mechanism_label: f.mechanism_label,
                        cause_not_determined: f.cause_nd,
                        downtime_hours: Some(f.downtime),
                        flags,
                        di_at: f.di_at.filter(|_| di_lead.is_some() || used_di).map(|d| d.to_rfc3339()),
                        di_lead_hours: di_lead,
                    });
                }
                Item::Competing(f) => {
                    let key = format!("competing:{}", f.id);
                    let choice = choices.get(&key).unwrap_or(&empty);
                    let wanted = choice.include.unwrap_or(true);
                    let included = wanted && usable_age.is_some();
                    if included || (wanted && start.is_none()) {
                        if included {
                            observations.push(WeibullObservation::censored(usable_age.unwrap_or_default()));
                        }
                        start = Some(match f.restored_at {
                            Some(r) if r > at => r,
                            _ => at,
                        });
                    }
                    let state_reason = if usable_age.is_none() {
                        Some(if start.is_none() { "no_known_start" } else { "zero_age" })
                    } else if choice.include == Some(false) {
                        Some("analyst")
                    } else {
                        None
                    };
                    records.push(StudyRecord {
                        key,
                        kind: RECORD_COMPETING.into(),
                        equipment_id: f.equipment_id,
                        asset_code: asset.asset_code.clone(),
                        failure_event_id: Some(f.id),
                        work_order_id: f.wo_id,
                        work_order_code: f.wo_code,
                        di_id: f.di_id,
                        di_code: f.di_code,
                        event_at: at.to_rfc3339(),
                        event_time_source: if f.from_repair_start {
                            "repair_start"
                        } else {
                            "detection"
                        }
                        .into(),
                        restored_at: f.restored_at.map(|r| r.to_rfc3339()),
                        age_hours: usable_age,
                        included,
                        state_reason: state_reason.map(str::to_string),
                        analyst_note: choice.note.clone(),
                        failure_mode_code: f.mode_code,
                        failure_mode_label: f.mode_label,
                        failure_cause_label: f.cause_label,
                        failure_mechanism_label: f.mechanism_label,
                        cause_not_determined: f.cause_nd,
                        downtime_hours: Some(f.downtime),
                        flags: Vec::new(),
                        di_at: None,
                        di_lead_hours: None,
                    });
                }
                Item::Pm(p) => {
                    let key = format!("pm:{}", p.wo_id);
                    let choice = choices.get(&key).unwrap_or(&empty);
                    if choice.include.is_none() {
                        pm_to_confirm += 1;
                    }
                    let included = choice.include == Some(true) && usable_age.is_some();
                    let start_known = start.is_some();
                    if choice.include == Some(true) {
                        if included {
                            observations.push(WeibullObservation::censored(usable_age.unwrap_or_default()));
                        }
                        start = Some(at);
                    }
                    let state_reason = if choice.include == Some(true) && usable_age.is_none() {
                        Some(if start_known { "zero_age" } else { "no_known_start" })
                    } else {
                        match choice.include {
                            None => Some("pm_not_confirmed"),
                            Some(false) => Some("analyst"),
                            Some(true) => None,
                        }
                    };
                    records.push(StudyRecord {
                        key,
                        kind: RECORD_PM_SUSPENSION.into(),
                        equipment_id: p.equipment_id,
                        asset_code: asset.asset_code.clone(),
                        failure_event_id: None,
                        work_order_id: Some(p.wo_id),
                        work_order_code: Some(p.wo_code),
                        di_id: None,
                        di_code: None,
                        event_at: at.to_rfc3339(),
                        event_time_source: "pm_completion".into(),
                        restored_at: None,
                        age_hours: usable_age,
                        included,
                        state_reason: state_reason.map(str::to_string),
                        analyst_note: choice.note.clone(),
                        failure_mode_code: None,
                        failure_mode_label: None,
                        failure_cause_label: None,
                        failure_mechanism_label: None,
                        cause_not_determined: false,
                        downtime_hours: None,
                        flags: Vec::new(),
                        di_at: None,
                        di_lead_hours: None,
                    });
                }
            }
        }
        if let Some(s) = start {
            let age = clock.hours_between(s, period_end);
            if age > 0.0 && age.is_finite() {
                let key = format!("end:{}", asset.equipment_id);
                let choice = choices.get(&key).unwrap_or(&empty);
                let included = choice.include.unwrap_or(true);
                if included {
                    observations.push(WeibullObservation::censored(age));
                }
                records.push(StudyRecord {
                    key,
                    kind: RECORD_END_OF_PERIOD.into(),
                    equipment_id: asset.equipment_id,
                    asset_code: asset.asset_code.clone(),
                    failure_event_id: None,
                    work_order_id: None,
                    work_order_code: None,
                    di_id: None,
                    di_code: None,
                    event_at: period_end.to_rfc3339(),
                    event_time_source: "period_end".into(),
                    restored_at: None,
                    age_hours: Some(age),
                    included,
                    state_reason: (!included).then(|| "analyst".to_string()),
                    analyst_note: choice.note.clone(),
                    failure_mode_code: None,
                    failure_mode_label: None,
                    failure_cause_label: None,
                    failure_mechanism_label: None,
                    cause_not_determined: false,
                    downtime_hours: None,
                    flags: Vec::new(),
                    di_at: None,
                    di_lead_hours: None,
                });
            }
        }
    }

    if pm_to_confirm > 0 {
        checks.push(DataCheck {
            kind: "pm_to_confirm".into(),
            severity: "info".into(),
            record_key: None,
            equipment_id: None,
            asset_code: None,
            work_order_id: None,
            work_order_code: None,
            di_code: None,
            hours: None,
            count: Some(pm_to_confirm),
            coverage: None,
        });
    }
    if no_start_count > 0 {
        checks.push(DataCheck {
            kind: "no_known_start".into(),
            severity: "info".into(),
            record_key: None,
            equipment_id: None,
            asset_code: None,
            work_order_id: None,
            work_order_code: None,
            di_code: None,
            hours: None,
            count: Some(no_start_count),
            coverage: None,
        });
    }
    // Warnings first, then information, then confirmations.
    let rank = |s: &str| match s {
        "danger" => 0,
        "warning" => 1,
        "info" => 2,
        _ => 3,
    };
    checks.sort_by_key(|c| rank(&c.severity));
    records.sort_by(|a, b| a.event_at.cmp(&b.event_at).then(a.asset_code.cmp(&b.asset_code)));

    let failures_used = records
        .iter()
        .filter(|r| r.included && r.kind == RECORD_FAILURE)
        .count() as i64;
    let suspensions_used = records
        .iter()
        .filter(|r| r.included && r.kind != RECORD_FAILURE)
        .count() as i64;
    let excluded = records.iter().filter(|r| !r.included).count() as i64;

    Ok(BuiltDataset {
        dataset: StudyDataset {
            study,
            effective_time_basis: if operating { BASIS_OPERATING } else { BASIS_CALENDAR }.to_string(),
            clocks: clock_infos,
            records,
            checks,
            failures_used,
            suspensions_used,
            excluded,
            history,
        },
        observations,
        clocks,
        period_start,
        period_end,
        failures_used,
        total_hours,
    })
}

/// Failures of the study scope after `since` (for the effect of an applied decision), and the
/// study hours of those equipment from `since` to now.
pub async fn failures_since(
    db: &DatabaseConnection,
    equipment_ids: &[i64],
    clocks: &HashMap<i64, StudyClock>,
    mode: Option<i64>,
    since: DateTime<Utc>,
) -> AppResult<(i64, f64)> {
    let now = Utc::now();
    let mut count = 0_i64;
    let mut total = 0.0;
    for id in equipment_ids {
        let rows = load_failures(db, *id, &since.to_rfc3339(), &now.to_rfc3339(), mode).await?;
        count += rows.iter().filter(|f| f.eligible && f.base_at > since).count() as i64;
        let clock = clocks.get(id).cloned().unwrap_or(StudyClock::Calendar);
        total += clock.hours_between(since, now);
    }
    Ok((count, total))
}
