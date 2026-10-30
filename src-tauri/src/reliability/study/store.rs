//! Persistence of reliability studies, their equipment, the analyst's record choices and the
//! per-class cost defaults.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait, TryGetable};

use crate::errors::{AppError, AppResult};
use crate::reliability::study::clock::parse_instant;
use crate::reliability::study::domain::{
    CostProfile, CreateStudyInput, RecordHistoryEntry, ReliabilityStudy, SaveStudyCostsInput, SetStudyRecordInput,
    StudyAsset, UpdateStudyInput, ACTION_EXCLUDE, ACTION_INCLUDE, ACTION_USE_DI_TIME, ACTION_USE_REPAIR_TIME,
    RECORD_COMPETING, RECORD_END_OF_PERIOD, RECORD_FAILURE, RECORD_PM_SUSPENSION,
};

pub const STUDY_ACTIVE: &str = "active";
pub const STUDY_ARCHIVED: &str = "archived";
pub const BASIS_AUTO: &str = "auto";
pub const BASIS_CALENDAR: &str = "calendar";
pub const COMPETING_IGNORE: &str = "ignore";
pub const COMPETING_SUSPEND: &str = "suspend";

/// At most this many equipment in one study (pooling only makes sense for a group of like items).
pub const MAX_STUDY_ASSETS: usize = 50;

pub(crate) fn get<T: TryGetable>(row: &QueryResult, col: &str) -> AppResult<T> {
    row.try_get("", col)
        .map_err(|e| AppError::SyncError(format!("reliability study decode '{col}': {e}")))
}

pub(crate) fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

pub(crate) fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

pub(crate) fn opt_str(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

fn now() -> String {
    crate::clock::now().to_rfc3339()
}

async fn load_assets(db: &impl ConnectionTrait, study_id: i64) -> AppResult<Vec<StudyAsset>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.asset_id_code, e.name, e.class_id, ec.name AS class_name
             FROM reliability_study_assets a
             JOIN equipment e ON e.id = a.equipment_id
             LEFT JOIN equipment_classes ec ON ec.id = e.class_id
             WHERE a.study_id = ?
             ORDER BY e.asset_id_code ASC",
            [study_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(StudyAsset {
                equipment_id: get(r, "id")?,
                asset_code: get(r, "asset_id_code")?,
                asset_name: get(r, "name")?,
                class_id: get(r, "class_id")?,
                class_name: get(r, "class_name")?,
            })
        })
        .collect()
}

const STUDY_SELECT: &str = "SELECT s.id, s.title, s.period_start, s.period_end, s.failure_mode_id,
        fc.code AS mode_code, fc.label AS mode_label, s.time_basis, s.competing_modes, s.mission_hours, s.planned_cost, s.failure_cost,
        s.cost_currency_code, s.pm_plan_id, s.status, s.row_version, s.created_at, s.updated_at,
        COALESCE(ua.display_name, ua.username) AS creator,
        (SELECT COUNT(*) FROM reliability_study_decisions d WHERE d.study_id = s.id) AS decision_count
     FROM reliability_studies s
     LEFT JOIN reference_values fc ON fc.id = s.failure_mode_id
     LEFT JOIN user_accounts ua ON ua.id = s.created_by_id";

async fn map_study(db: &impl ConnectionTrait, r: &QueryResult) -> AppResult<ReliabilityStudy> {
    let id: i64 = get(r, "id")?;
    Ok(ReliabilityStudy {
        id,
        title: get(r, "title")?,
        period_start: get(r, "period_start")?,
        period_end: get(r, "period_end")?,
        failure_mode_id: get(r, "failure_mode_id")?,
        failure_mode_code: get(r, "mode_code")?,
        failure_mode_label: get(r, "mode_label")?,
        time_basis: get(r, "time_basis")?,
        competing_modes: get(r, "competing_modes")?,
        mission_hours: get(r, "mission_hours")?,
        planned_cost: get(r, "planned_cost")?,
        failure_cost: get(r, "failure_cost")?,
        cost_currency_code: get(r, "cost_currency_code")?,
        pm_plan_id: get(r, "pm_plan_id")?,
        status: get(r, "status")?,
        assets: load_assets(db, id).await?,
        decision_count: get(r, "decision_count")?,
        created_by_name: get(r, "creator")?,
        row_version: get(r, "row_version")?,
        created_at: get(r, "created_at")?,
        updated_at: get(r, "updated_at")?,
    })
}

pub async fn get_study(db: &impl ConnectionTrait, id: i64) -> AppResult<ReliabilityStudy> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{STUDY_SELECT} WHERE s.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study".into(),
            id: id.to_string(),
        })?;
    map_study(db, &row).await
}

pub async fn list_studies(db: &DatabaseConnection, include_archived: bool) -> AppResult<Vec<ReliabilityStudy>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{STUDY_SELECT} WHERE (? = 1 OR s.status = 'active') ORDER BY s.updated_at DESC, s.id DESC LIMIT 200"
            ),
            [i64::from(include_archived).into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(map_study(db, r).await?);
    }
    Ok(out)
}

fn validate_period(start: &str, end: &str) -> AppResult<()> {
    let (s, e) = (parse_instant(start)?, parse_instant(end)?);
    if e <= s {
        return Err(invalid("STUDY_PERIOD_EMPTY"));
    }
    Ok(())
}

async fn validate_assets(db: &impl ConnectionTrait, ids: &[i64]) -> AppResult<Vec<i64>> {
    let mut unique: Vec<i64> = Vec::new();
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    if unique.is_empty() {
        return Err(invalid("STUDY_NO_ASSETS"));
    }
    if unique.len() > MAX_STUDY_ASSETS {
        return Err(invalid("STUDY_TOO_MANY_ASSETS"));
    }
    for id in &unique {
        let ok = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM equipment WHERE id = ? AND deleted_at IS NULL",
                [(*id).into()],
            ))
            .await?
            .is_some();
        if !ok {
            return Err(AppError::NotFound {
                entity: "equipment".into(),
                id: id.to_string(),
            });
        }
    }
    Ok(unique)
}

async fn validate_mode(db: &impl ConnectionTrait, mode: Option<i64>) -> AppResult<()> {
    if let Some(m) = mode {
        let ok = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id
                 JOIN reference_domains d ON d.id = rs.domain_id
                 WHERE rv.id = ? AND UPPER(TRIM(d.code)) = 'WORK.FAILURE_MODES'",
                [m.into()],
            ))
            .await?
            .is_some();
        if !ok {
            return Err(invalid("STUDY_FAILURE_MODE_UNKNOWN"));
        }
    }
    Ok(())
}

async fn default_title(db: &impl ConnectionTrait, ids: &[i64], mode: Option<i64>) -> AppResult<String> {
    let mut codes = Vec::new();
    for id in ids.iter().take(4) {
        if let Some(r) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT asset_id_code FROM equipment WHERE id = ?",
                [(*id).into()],
            ))
            .await?
        {
            codes.push(get::<String>(&r, "asset_id_code")?);
        }
    }
    let mut title = codes.join(", ");
    if ids.len() > 4 {
        title.push_str(&format!(" +{}", ids.len() - 4));
    }
    if let Some(m) = mode {
        if let Some(r) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT label FROM reference_values WHERE id = ?",
                [m.into()],
            ))
            .await?
        {
            title.push_str(" · ");
            title.push_str(&get::<String>(&r, "label")?);
        }
    }
    Ok(title)
}

async fn replace_assets(db: &impl ConnectionTrait, study_id: i64, ids: &[i64]) -> AppResult<()> {
    let ts = now();
    let current: Vec<i64> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM reliability_study_assets WHERE study_id = ?",
            [study_id.into()],
        ))
        .await?
        .iter()
        .map(|r| get::<i64>(r, "equipment_id"))
        .collect::<AppResult<_>>()?;
    for old in current.iter().filter(|c| !ids.contains(c)) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM reliability_study_assets WHERE study_id = ? AND equipment_id = ?",
            [study_id.into(), (*old).into()],
        ))
        .await?;
    }
    for id in ids.iter().filter(|i| !current.contains(i)) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reliability_study_assets (entity_sync_id, study_id, equipment_id, row_version, created_at, updated_at)
             VALUES (?, ?, ?, 1, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                study_id.into(),
                (*id).into(),
                ts.clone().into(),
                ts.clone().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

pub async fn create_study(
    db: &DatabaseConnection,
    input: CreateStudyInput,
    actor_id: i64,
) -> AppResult<ReliabilityStudy> {
    validate_period(&input.period_start, &input.period_end)?;
    validate_mode(db, input.failure_mode_id).await?;
    let ids = validate_assets(db, &input.equipment_ids).await?;
    let title = match input.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => default_title(db, &ids, input.failure_mode_id).await?,
    };
    let ts = now();
    let txn = db.begin().await?;
    let sync_id = crate::sync::identity::new_entity_sync_id();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reliability_studies (entity_sync_id, title, period_start, period_end, failure_mode_id,
            time_basis, status, created_by_id, row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'auto', 'active', ?, 1, ?, ?)",
        [
            sync_id.clone().into(),
            title.into(),
            input.period_start.into(),
            input.period_end.into(),
            opt_i64(input.failure_mode_id),
            actor_id.into(),
            ts.clone().into(),
            ts.into(),
        ],
    ))
    .await?;
    let id: i64 = get(
        &txn.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reliability_studies WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("reliability study insert missing".into()))?,
        "id",
    )?;
    replace_assets(&txn, id, &ids).await?;
    txn.commit().await?;
    get_study(db, id).await
}

pub async fn update_study(db: &DatabaseConnection, input: UpdateStudyInput) -> AppResult<ReliabilityStudy> {
    let current = get_study(db, input.id).await?;
    if current.row_version != input.expected_row_version {
        return Err(invalid("STUDY_STALE"));
    }
    let title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map_or(current.title.clone(), str::to_string);
    let period_start = input.period_start.clone().unwrap_or(current.period_start.clone());
    let period_end = input.period_end.clone().unwrap_or(current.period_end.clone());
    validate_period(&period_start, &period_end)?;
    let mode = if input.clear_failure_mode {
        None
    } else {
        input.failure_mode_id.or(current.failure_mode_id)
    };
    validate_mode(db, mode).await?;
    let basis = input.time_basis.clone().unwrap_or(current.time_basis.clone());
    if basis != BASIS_AUTO && basis != BASIS_CALENDAR {
        return Err(invalid("STUDY_TIME_BASIS_INVALID"));
    }
    let competing = input.competing_modes.clone().unwrap_or(current.competing_modes.clone());
    if competing != COMPETING_IGNORE && competing != COMPETING_SUSPEND {
        return Err(invalid("STUDY_COMPETING_MODES_INVALID"));
    }
    let status = input.status.clone().unwrap_or(current.status.clone());
    if status != STUDY_ACTIVE && status != STUDY_ARCHIVED {
        return Err(invalid("STUDY_STATUS_INVALID"));
    }
    let plan = if input.clear_pm_plan {
        None
    } else {
        input.pm_plan_id.or(current.pm_plan_id)
    };
    let ids = match &input.equipment_ids {
        Some(ids) => Some(validate_assets(db, ids).await?),
        None => None,
    };
    let txn = db.begin().await?;
    let res = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_studies SET title = ?, period_start = ?, period_end = ?, failure_mode_id = ?,
                time_basis = ?, competing_modes = ?, pm_plan_id = ?, status = ?, row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                title.into(),
                period_start.into(),
                period_end.into(),
                opt_i64(mode),
                basis.into(),
                competing.into(),
                opt_i64(plan),
                status.into(),
                now().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if res.rows_affected() == 0 {
        return Err(invalid("STUDY_STALE"));
    }
    if let Some(ids) = ids {
        replace_assets(&txn, input.id, &ids).await?;
    }
    txn.commit().await?;
    get_study(db, input.id).await
}

pub async fn save_costs(
    db: &DatabaseConnection,
    input: SaveStudyCostsInput,
    actor_id: i64,
) -> AppResult<ReliabilityStudy> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !positive(input.planned_cost) || !positive(input.failure_cost) {
        return Err(invalid("STUDY_COSTS_INVALID"));
    }
    let currency = input.currency_code.trim().to_uppercase();
    if currency.is_empty() {
        return Err(invalid("STUDY_COSTS_CURRENCY_MISSING"));
    }
    let txn = db.begin().await?;
    let res = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_studies SET planned_cost = ?, failure_cost = ?, cost_currency_code = ?,
                row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input.planned_cost.into(),
                input.failure_cost.into(),
                currency.clone().into(),
                now().into(),
                input.study_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if res.rows_affected() == 0 {
        return Err(invalid("STUDY_STALE"));
    }
    if let Some(class_id) = input.save_as_class_default_for {
        upsert_cost_profile(
            &txn,
            class_id,
            input.planned_cost,
            input.failure_cost,
            &currency,
            actor_id,
        )
        .await?;
    }
    txn.commit().await?;
    get_study(db, input.study_id).await
}

/// Cost default of an equipment class. Its identity is derived from the class identity so two
/// devices setting the same class edit one row (a concurrent edit becomes a sync conflict).
pub async fn upsert_cost_profile(
    db: &impl ConnectionTrait,
    class_id: i64,
    planned: f64,
    failure: f64,
    currency: &str,
    actor_id: i64,
) -> AppResult<()> {
    let class = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM equipment_classes WHERE id = ?",
            [class_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_class".into(),
            id: class_id.to_string(),
        })?;
    let class_sync: Option<String> = get(&class, "entity_sync_id")?;
    let sync_id = match class_sync.filter(|s| !s.trim().is_empty()) {
        Some(s) => format!("reliability_cost_profile:{s}"),
        None => crate::sync::identity::new_entity_sync_id(),
    };
    let ts = now();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reliability_cost_profiles (entity_sync_id, equipment_class_id, planned_cost, failure_cost,
            currency_code, updated_by_id, row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)
         ON CONFLICT(equipment_class_id) DO UPDATE SET planned_cost = excluded.planned_cost,
            failure_cost = excluded.failure_cost, currency_code = excluded.currency_code,
            updated_by_id = excluded.updated_by_id, row_version = reliability_cost_profiles.row_version + 1,
            updated_at = excluded.updated_at",
        [
            sync_id.into(),
            class_id.into(),
            planned.into(),
            failure.into(),
            currency.to_string().into(),
            actor_id.into(),
            ts.clone().into(),
            ts.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn get_cost_profile(db: &impl ConnectionTrait, class_id: i64) -> AppResult<Option<CostProfile>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.equipment_class_id, ec.name, p.planned_cost, p.failure_cost, p.currency_code, p.updated_at
             FROM reliability_cost_profiles p JOIN equipment_classes ec ON ec.id = p.equipment_class_id
             WHERE p.equipment_class_id = ?",
            [class_id.into()],
        ))
        .await?;
    row.map(|r| {
        Ok(CostProfile {
            equipment_class_id: get(&r, "equipment_class_id")?,
            class_name: get(&r, "name")?,
            planned_cost: get(&r, "planned_cost")?,
            failure_cost: get(&r, "failure_cost")?,
            currency_code: get(&r, "currency_code")?,
            updated_at: get(&r, "updated_at")?,
        })
    })
    .transpose()
}

/// Parsed record key.
pub enum RecordRef {
    Failure(i64),
    Competing(i64),
    Pm(i64),
    End(i64),
}

pub fn parse_record_key(key: &str) -> AppResult<RecordRef> {
    let (kind, id) = key.split_once(':').ok_or_else(|| invalid("STUDY_RECORD_KEY_INVALID"))?;
    let id: i64 = id.parse().map_err(|_| invalid("STUDY_RECORD_KEY_INVALID"))?;
    match kind {
        "failure" => Ok(RecordRef::Failure(id)),
        "competing" => Ok(RecordRef::Competing(id)),
        "pm" => Ok(RecordRef::Pm(id)),
        "end" => Ok(RecordRef::End(id)),
        _ => Err(invalid("STUDY_RECORD_KEY_INVALID")),
    }
}

pub fn record_key(
    kind: &str,
    failure_event_id: Option<i64>,
    work_order_id: Option<i64>,
    equipment_id: Option<i64>,
) -> Option<String> {
    match kind {
        RECORD_FAILURE => failure_event_id.map(|i| format!("failure:{i}")),
        RECORD_COMPETING => failure_event_id.map(|i| format!("competing:{i}")),
        RECORD_PM_SUSPENSION => work_order_id.map(|i| format!("pm:{i}")),
        RECORD_END_OF_PERIOD => equipment_id.map(|i| format!("end:{i}")),
        _ => None,
    }
}

/// Append one analyst choice to the study history.
pub async fn append_record_event(db: &DatabaseConnection, input: &SetStudyRecordInput, actor_id: i64) -> AppResult<()> {
    let study = get_study(db, input.study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    let action = input.action.as_str();
    if ![
        ACTION_EXCLUDE,
        ACTION_INCLUDE,
        ACTION_USE_DI_TIME,
        ACTION_USE_REPAIR_TIME,
    ]
    .contains(&action)
    {
        return Err(invalid("STUDY_RECORD_ACTION_INVALID"));
    }
    let reason = input.reason.as_deref().map(str::trim).unwrap_or_default().to_string();
    if action == ACTION_EXCLUDE && reason.is_empty() {
        return Err(invalid("STUDY_RECORD_REASON_REQUIRED"));
    }
    let (kind, fe, wo, eq) = match parse_record_key(&input.record_key)? {
        RecordRef::Failure(id) => (RECORD_FAILURE, Some(id), None, None),
        RecordRef::Competing(id) => (RECORD_COMPETING, Some(id), None, None),
        RecordRef::Pm(id) => (RECORD_PM_SUSPENSION, None, Some(id), None),
        RecordRef::End(id) => (RECORD_END_OF_PERIOD, None, None, Some(id)),
    };
    if kind != RECORD_FAILURE && (action == ACTION_USE_DI_TIME || action == ACTION_USE_REPAIR_TIME) {
        return Err(invalid("STUDY_RECORD_ACTION_INVALID"));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reliability_study_record_events (entity_sync_id, study_id, record_kind, failure_event_id,
            work_order_id, equipment_id, action, reason, actor_id, row_version, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            input.study_id.into(),
            kind.into(),
            opt_i64(fe),
            opt_i64(wo),
            opt_i64(eq),
            action.into(),
            reason.into(),
            actor_id.into(),
            now().into(),
        ],
    ))
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reliability_studies SET updated_at = ? WHERE id = ?",
        [now().into(), input.study_id.into()],
    ))
    .await?;
    Ok(())
}

/// Latest choices per record key: `(include override, use DI time, note)`.
#[derive(Debug, Clone, Default)]
pub struct RecordChoice {
    pub include: Option<bool>,
    pub use_di_time: bool,
    pub note: Option<String>,
}

pub async fn load_record_history(
    db: &impl ConnectionTrait,
    study_id: i64,
) -> AppResult<(std::collections::HashMap<String, RecordChoice>, Vec<RecordHistoryEntry>)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT h.record_kind, h.failure_event_id, h.work_order_id, h.equipment_id, h.action, h.reason, h.created_at,
                    COALESCE(ua.display_name, ua.username) AS actor,
                    COALESCE(wo.code, pmwo.code) AS wo_code, e.asset_id_code AS asset_code
             FROM reliability_study_record_events h
             LEFT JOIN user_accounts ua ON ua.id = h.actor_id
             LEFT JOIN failure_events fe ON fe.id = h.failure_event_id
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             LEFT JOIN work_orders pmwo ON pmwo.id = h.work_order_id
             LEFT JOIN equipment e ON e.id = COALESCE(h.equipment_id, fe.equipment_id, pmwo.equipment_id)
             WHERE h.study_id = ?
             ORDER BY h.created_at ASC, h.id ASC",
            [study_id.into()],
        ))
        .await?;
    let mut choices: std::collections::HashMap<String, RecordChoice> = std::collections::HashMap::new();
    let mut history = Vec::with_capacity(rows.len());
    for r in &rows {
        let kind: String = get(r, "record_kind")?;
        let Some(key) = record_key(
            &kind,
            get(r, "failure_event_id")?,
            get(r, "work_order_id")?,
            get(r, "equipment_id")?,
        ) else {
            continue;
        };
        let action: String = get(r, "action")?;
        let reason: String = get(r, "reason")?;
        let entry = choices.entry(key.clone()).or_default();
        match action.as_str() {
            ACTION_EXCLUDE => entry.include = Some(false),
            ACTION_INCLUDE => entry.include = Some(true),
            ACTION_USE_DI_TIME => entry.use_di_time = true,
            ACTION_USE_REPAIR_TIME => entry.use_di_time = false,
            _ => {}
        }
        entry.note = Some(reason.clone()).filter(|s| !s.is_empty()).or(entry.note.take());
        history.push(RecordHistoryEntry {
            record_key: key,
            work_order_code: get(r, "wo_code")?,
            asset_code: get(r, "asset_code")?,
            action,
            reason,
            actor_name: get(r, "actor")?,
            created_at: get(r, "created_at")?,
        });
    }
    history.reverse();
    Ok((choices, history))
}
