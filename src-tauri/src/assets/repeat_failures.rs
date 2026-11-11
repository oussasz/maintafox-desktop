//! Repeat failures of an equipment ("pannes répétitives", L2-09).
//!
//! An equipment is flagged when it has at least N unplanned failures (`failure_events`,
//! `is_planned = 0`, dated as RAMS reads them: failure, else detection, else record time) within
//! the last D days. N and D are tenant settings (`reliability.repeat_failure_count`,
//! `reliability.repeat_failure_window_days`, Paramètres > Fiabilité); a missing or invalid setting
//! is an error, never a guessed rule.
//!
//! A flagged equipment is followed up with "5 pourquoi" records (`asset_five_why_analyses`) and a
//! tracked list of corrective actions (`asset_corrective_actions`: action, owner, due date,
//! status), both migration 289.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Tenant setting: fewest unplanned failures in the window that flag an equipment.
pub const SETTING_COUNT: &str = "reliability.repeat_failure_count";
/// Tenant setting: length of the window, in days.
pub const SETTING_WINDOW_DAYS: &str = "reliability.repeat_failure_window_days";

pub const GATE_ASSET_FIVE_WHY_PROBLEM_REQUIRED: &str = "GATE_ASSET_FIVE_WHY_PROBLEM_REQUIRED";
pub const GATE_ASSET_FIVE_WHY_CHANGED_ELSEWHERE: &str = "GATE_ASSET_FIVE_WHY_CHANGED_ELSEWHERE";
pub const GATE_ASSET_ACTION_TEXT_REQUIRED: &str = "GATE_ASSET_ACTION_TEXT_REQUIRED";
pub const GATE_ASSET_ACTION_OWNER_REQUIRED: &str = "GATE_ASSET_ACTION_OWNER_REQUIRED";
pub const GATE_ASSET_ACTION_OWNER_UNKNOWN: &str = "GATE_ASSET_ACTION_OWNER_UNKNOWN";
pub const GATE_ASSET_ACTION_DUE_DATE_REQUIRED: &str = "GATE_ASSET_ACTION_DUE_DATE_REQUIRED";
pub const GATE_ASSET_ACTION_DUE_DATE_INVALID: &str = "GATE_ASSET_ACTION_DUE_DATE_INVALID";
pub const GATE_ASSET_ACTION_STATUS_INVALID: &str = "GATE_ASSET_ACTION_STATUS_INVALID";
pub const GATE_ASSET_ACTION_CHANGED_ELSEWHERE: &str = "GATE_ASSET_ACTION_CHANGED_ELSEWHERE";

/// Statuses of a corrective action (the workflow of the list, not a plant list).
pub const ACTION_STATUSES: &[&str] = &["open", "done", "cancelled"];

/// Longest text kept in one field of an analysis or an action.
pub const TEXT_MAX_CHARS: usize = 2000;

/// Date of a failure as RAMS reads it (alias `fe`).
const FAILURE_DATE_SQL: &str = "datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at))";

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "repeat failure row decode failed for column '{column}': {e}"
    ))
}

fn refusal(message: String) -> AppError {
    AppError::ValidationFailed(vec![message])
}

// ─── Rule and flag ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RepeatFailureRule {
    pub min_failures: i64,
    pub window_days: i64,
    /// Start of the window (failures on or after it count).
    pub since: String,
}

/// The rule in force, read from the settings.
pub async fn repeat_failure_rule(db: &impl ConnectionTrait) -> AppResult<RepeatFailureRule> {
    let min_failures = crate::settings::catalog::integer_setting(db, SETTING_COUNT).await?;
    let window_days = crate::settings::catalog::integer_setting(db, SETTING_WINDOW_DAYS).await?;
    let since =
        crate::analytics::as_of::instant(crate::analytics::as_of::days_before(crate::clock::now(), window_days));
    Ok(RepeatFailureRule {
        min_failures,
        window_days,
        since,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RepeatFailureAsset {
    pub asset_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub failure_count: i64,
    pub last_failure_at: Option<String>,
}

/// The flagged equipment (most failures first) and the rule that flags them.
#[derive(Debug, Clone, Serialize)]
pub struct RepeatFailureOverview {
    pub rule: RepeatFailureRule,
    pub assets: Vec<RepeatFailureAsset>,
}

fn map_asset(row: &QueryResult) -> AppResult<RepeatFailureAsset> {
    Ok(RepeatFailureAsset {
        asset_id: row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?,
        asset_code: row.try_get("", "asset_code").map_err(|e| decode_err("asset_code", e))?,
        asset_name: row.try_get("", "asset_name").map_err(|e| decode_err("asset_name", e))?,
        failure_count: row
            .try_get("", "failure_count")
            .map_err(|e| decode_err("failure_count", e))?,
        last_failure_at: row
            .try_get("", "last_failure_at")
            .map_err(|e| decode_err("last_failure_at", e))?,
    })
}

/// Failure count per equipment within the window (`asset_id = NULL` → every equipment), keeping
/// those at or above `min_failures`.
async fn counts(
    db: &impl ConnectionTrait,
    rule: &RepeatFailureRule,
    asset_id: Option<i64>,
    min_failures: i64,
) -> AppResult<Vec<RepeatFailureAsset>> {
    let sql = format!(
        "SELECT e.id AS asset_id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                COUNT(*) AS failure_count, MAX({FAILURE_DATE_SQL}) AS last_failure_at \
           FROM failure_events fe JOIN equipment e ON e.id = fe.equipment_id \
          WHERE fe.is_planned = 0 AND e.deleted_at IS NULL \
            AND {FAILURE_DATE_SQL} >= datetime(?) \
            AND (? IS NULL OR e.id = ?) \
          GROUP BY e.id, e.asset_id_code, e.name \
         HAVING COUNT(*) >= ? \
          ORDER BY failure_count DESC, last_failure_at DESC, e.asset_id_code ASC"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                rule.since.clone().into(),
                asset_id.into(),
                asset_id.into(),
                min_failures.into(),
            ],
        ))
        .await?;
    rows.iter().map(map_asset).collect()
}

/// Every equipment flagged "pannes répétitives" now (asset list, home page).
pub async fn list_repeat_failure_assets(db: &impl ConnectionTrait) -> AppResult<RepeatFailureOverview> {
    let rule = repeat_failure_rule(db).await?;
    let assets = counts(db, &rule, None, rule.min_failures).await?;
    Ok(RepeatFailureOverview { rule, assets })
}

/// Repeat-failure state of one equipment, with its follow-up counts.
#[derive(Debug, Clone, Serialize)]
pub struct AssetRepeatFailureStatus {
    pub asset_id: i64,
    pub rule: RepeatFailureRule,
    pub failure_count: i64,
    pub last_failure_at: Option<String>,
    pub is_repeat_failure: bool,
    pub open_action_count: i64,
    pub overdue_action_count: i64,
}

pub async fn asset_repeat_failure_status(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<AssetRepeatFailureStatus> {
    let rule = repeat_failure_rule(db).await?;
    let found = counts(db, &rule, Some(asset_id), 1).await?.into_iter().next();
    let (failure_count, last_failure_at) = found.map_or((0, None), |a| (a.failure_count, a.last_failure_at));
    let today = crate::clock::now().format("%Y-%m-%d").to_string();
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE WHEN status = 'open' THEN 1 ELSE 0 END), 0) AS open_count, \
                    COALESCE(SUM(CASE WHEN status = 'open' AND due_date < ? THEN 1 ELSE 0 END), 0) AS overdue_count \
               FROM asset_corrective_actions WHERE equipment_id = ?",
            [today.into(), asset_id.into()],
        ))
        .await?;
    let (open_action_count, overdue_action_count) = match row {
        Some(r) => (
            r.try_get::<i64>("", "open_count")
                .map_err(|e| decode_err("open_count", e))?,
            r.try_get::<i64>("", "overdue_count")
                .map_err(|e| decode_err("overdue_count", e))?,
        ),
        None => (0, 0),
    };
    Ok(AssetRepeatFailureStatus {
        asset_id,
        is_repeat_failure: failure_count >= rule.min_failures,
        rule,
        failure_count,
        last_failure_at,
        open_action_count,
        overdue_action_count,
    })
}

// ─── Shared checks ───────────────────────────────────────────────────────────────

async fn assert_asset(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<()> {
    let found = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?;
    if found.is_none() {
        return Err(AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        });
    }
    Ok(())
}

/// Trimmed text, `None` when empty; refused beyond [`TEXT_MAX_CHARS`].
fn text(field: &str, raw: Option<&str>) -> AppResult<Option<String>> {
    let t = raw.map(str::trim).filter(|s| !s.is_empty());
    if let Some(t) = t {
        if t.chars().count() > TEXT_MAX_CHARS {
            return Err(refusal(format!(
                "GATE_ASSET_FOLLOW_UP_TEXT_TOO_LONG:field={field}:max={TEXT_MAX_CHARS}: Le champ {field} dépasse {TEXT_MAX_CHARS} caractères."
            )));
        }
    }
    Ok(t.map(str::to_string))
}

// ─── 5 pourquoi ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FiveWhyAnalysis {
    pub id: i64,
    pub asset_id: i64,
    pub problem_statement: String,
    pub why_1: Option<String>,
    pub why_2: Option<String>,
    pub why_3: Option<String>,
    pub why_4: Option<String>,
    pub why_5: Option<String>,
    pub root_cause: Option<String>,
    pub created_by_name: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveFiveWhyPayload {
    /// `None` = a new analysis.
    pub id: Option<i64>,
    pub asset_id: i64,
    /// Required to change an existing analysis (optimistic concurrency).
    pub expected_row_version: Option<i64>,
    pub problem_statement: String,
    pub why_1: Option<String>,
    pub why_2: Option<String>,
    pub why_3: Option<String>,
    pub why_4: Option<String>,
    pub why_5: Option<String>,
    pub root_cause: Option<String>,
}

const FIVE_WHY_SELECT: &str = "SELECT a.id, a.equipment_id, a.problem_statement, a.why_1, a.why_2, a.why_3, a.why_4, a.why_5, a.root_cause, COALESCE(u.display_name, u.username) AS created_by_name, a.row_version, a.created_at, a.updated_at FROM asset_five_why_analyses a LEFT JOIN user_accounts u ON u.id = a.created_by_id";

fn map_five_why(row: &QueryResult) -> AppResult<FiveWhyAnalysis> {
    let get_opt = |c: &str| row.try_get::<Option<String>>("", c).map_err(|e| decode_err(c, e));
    Ok(FiveWhyAnalysis {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        asset_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        problem_statement: row
            .try_get("", "problem_statement")
            .map_err(|e| decode_err("problem_statement", e))?,
        why_1: get_opt("why_1")?,
        why_2: get_opt("why_2")?,
        why_3: get_opt("why_3")?,
        why_4: get_opt("why_4")?,
        why_5: get_opt("why_5")?,
        root_cause: get_opt("root_cause")?,
        created_by_name: get_opt("created_by_name")?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

/// The "5 pourquoi" records of an equipment, newest first.
pub async fn list_five_whys(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<Vec<FiveWhyAnalysis>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{FIVE_WHY_SELECT} WHERE a.equipment_id = ? ORDER BY a.created_at DESC, a.id DESC"),
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_five_why).collect()
}

/// Records a new "5 pourquoi" or changes one (the problem statement is required).
pub async fn save_five_why(
    db: &DatabaseConnection,
    payload: SaveFiveWhyPayload,
    actor_id: i32,
) -> AppResult<FiveWhyAnalysis> {
    let problem = text("problem_statement", Some(payload.problem_statement.as_str()))?.ok_or_else(|| {
        refusal(format!(
            "{GATE_ASSET_FIVE_WHY_PROBLEM_REQUIRED}: Décrivez le problème analysé."
        ))
    })?;
    let whys = [
        text("why_1", payload.why_1.as_deref())?,
        text("why_2", payload.why_2.as_deref())?,
        text("why_3", payload.why_3.as_deref())?,
        text("why_4", payload.why_4.as_deref())?,
        text("why_5", payload.why_5.as_deref())?,
    ];
    let root_cause = text("root_cause", payload.root_cause.as_deref())?;
    let now = crate::clock::now().to_rfc3339();
    let actor = i64::from(actor_id);
    let txn = db.begin().await?;
    assert_asset(&txn, payload.asset_id).await?;
    let id = match payload.id {
        None => {
            let sync_id = crate::sync::identity::new_entity_sync_id();
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO asset_five_why_analyses (entity_sync_id, equipment_id, problem_statement, why_1, why_2, why_3, why_4, why_5, root_cause, created_by_id, updated_by_id, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    sync_id.clone().into(),
                    payload.asset_id.into(),
                    problem.into(),
                    whys[0].clone().into(),
                    whys[1].clone().into(),
                    whys[2].clone().into(),
                    whys[3].clone().into(),
                    whys[4].clone().into(),
                    root_cause.into(),
                    actor.into(),
                    actor.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
            txn.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM asset_five_why_analyses WHERE entity_sync_id = ?",
                [sync_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("five-why created but not found")))?
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?
        }
        Some(id) => {
            let changed = txn
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE asset_five_why_analyses SET problem_statement = ?, why_1 = ?, why_2 = ?, why_3 = ?, why_4 = ?, why_5 = ?, root_cause = ?, updated_by_id = ?, updated_at = ?, row_version = row_version + 1 WHERE id = ? AND equipment_id = ? AND row_version = ?",
                    [
                        problem.into(),
                        whys[0].clone().into(),
                        whys[1].clone().into(),
                        whys[2].clone().into(),
                        whys[3].clone().into(),
                        whys[4].clone().into(),
                        root_cause.into(),
                        actor.into(),
                        now.into(),
                        id.into(),
                        payload.asset_id.into(),
                        payload.expected_row_version.unwrap_or(-1).into(),
                    ],
                ))
                .await?
                .rows_affected();
            if changed == 0 {
                return Err(refusal(format!(
                    "{GATE_ASSET_FIVE_WHY_CHANGED_ELSEWHERE}: Cette analyse a été modifiée ailleurs ; rechargez-la avant de la modifier."
                )));
            }
            id
        }
    };
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{FIVE_WHY_SELECT} WHERE a.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("five-why saved but not found")))?;
    let saved = map_five_why(&row)?;
    txn.commit().await?;
    Ok(saved)
}

// ─── Corrective actions ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CorrectiveAction {
    pub id: i64,
    pub asset_id: i64,
    pub action_text: String,
    pub owner_personnel_id: Option<i64>,
    pub owner_name: Option<String>,
    pub owner_employee_code: Option<String>,
    pub due_date: Option<String>,
    pub status: String,
    pub completed_at: Option<String>,
    /// Open and past its due date.
    pub is_overdue: bool,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveCorrectiveActionPayload {
    /// `None` = a new action.
    pub id: Option<i64>,
    pub asset_id: i64,
    /// Required to change an existing action (optimistic concurrency).
    pub expected_row_version: Option<i64>,
    pub action_text: String,
    pub owner_personnel_id: Option<i64>,
    /// `YYYY-MM-DD`.
    pub due_date: Option<String>,
    pub status: String,
}

const ACTION_SELECT: &str = "SELECT a.id, a.equipment_id, a.action_text, a.owner_personnel_id, p.full_name AS owner_name, p.employee_code AS owner_employee_code, a.due_date, a.status, a.completed_at, CASE WHEN a.status = 'open' AND a.due_date < ? THEN 1 ELSE 0 END AS is_overdue, a.row_version, a.created_at, a.updated_at FROM asset_corrective_actions a LEFT JOIN personnel p ON p.id = a.owner_personnel_id";

fn map_action(row: &QueryResult) -> AppResult<CorrectiveAction> {
    Ok(CorrectiveAction {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        asset_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        action_text: row
            .try_get("", "action_text")
            .map_err(|e| decode_err("action_text", e))?,
        owner_personnel_id: row
            .try_get("", "owner_personnel_id")
            .map_err(|e| decode_err("owner_personnel_id", e))?,
        owner_name: row.try_get("", "owner_name").map_err(|e| decode_err("owner_name", e))?,
        owner_employee_code: row
            .try_get("", "owner_employee_code")
            .map_err(|e| decode_err("owner_employee_code", e))?,
        due_date: row.try_get("", "due_date").map_err(|e| decode_err("due_date", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        completed_at: row
            .try_get("", "completed_at")
            .map_err(|e| decode_err("completed_at", e))?,
        is_overdue: row
            .try_get::<i64>("", "is_overdue")
            .map_err(|e| decode_err("is_overdue", e))?
            != 0,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn today() -> String {
    crate::clock::now().format("%Y-%m-%d").to_string()
}

/// The corrective actions of an equipment: open first (by due date), then the others.
pub async fn list_corrective_actions(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<Vec<CorrectiveAction>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{ACTION_SELECT} WHERE a.equipment_id = ? \
                 ORDER BY CASE WHEN a.status = 'open' THEN 0 ELSE 1 END, a.due_date ASC, a.id ASC"
            ),
            [today().into(), asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_action).collect()
}

/// Checks an action's fields; returns `(text, owner, due date, status)`.
pub fn checked_action(payload: &SaveCorrectiveActionPayload) -> AppResult<(String, i64, String, String)> {
    let mut errors = Vec::new();
    let action = match text("action_text", Some(payload.action_text.as_str())) {
        Ok(Some(t)) => Some(t),
        Ok(None) => {
            errors.push(format!(
                "{GATE_ASSET_ACTION_TEXT_REQUIRED}: Décrivez l'action corrective."
            ));
            None
        }
        Err(AppError::ValidationFailed(m)) => {
            errors.extend(m);
            None
        }
        Err(e) => return Err(e),
    };
    if payload.owner_personnel_id.is_none() {
        errors.push(format!(
            "{GATE_ASSET_ACTION_OWNER_REQUIRED}: Indiquez le responsable de l'action."
        ));
    }
    let due = payload.due_date.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match due {
        None => errors.push(format!(
            "{GATE_ASSET_ACTION_DUE_DATE_REQUIRED}: Indiquez l'échéance de l'action."
        )),
        Some(d) if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() => errors.push(format!(
            "{GATE_ASSET_ACTION_DUE_DATE_INVALID}: L'échéance « {d} » n'est pas une date (AAAA-MM-JJ)."
        )),
        Some(_) => {}
    }
    let status = payload.status.trim();
    if !ACTION_STATUSES.contains(&status) {
        errors.push(format!(
            "{GATE_ASSET_ACTION_STATUS_INVALID}:status={status}: Statut d'action inconnu « {status} »."
        ));
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    Ok((
        action.unwrap_or_default(),
        payload.owner_personnel_id.unwrap_or_default(),
        due.unwrap_or_default().to_string(),
        status.to_string(),
    ))
}

/// Records a new corrective action or changes one (text, owner, due date and status required).
pub async fn save_corrective_action(
    db: &DatabaseConnection,
    payload: SaveCorrectiveActionPayload,
    actor_id: i32,
) -> AppResult<CorrectiveAction> {
    let (action, owner, due, status) = checked_action(&payload)?;
    let now = crate::clock::now().to_rfc3339();
    let actor = i64::from(actor_id);
    let txn = db.begin().await?;
    assert_asset(&txn, payload.asset_id).await?;
    let owner_known = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM personnel WHERE id = ?",
            [owner.into()],
        ))
        .await?
        .is_some();
    if !owner_known {
        return Err(refusal(format!(
            "{GATE_ASSET_ACTION_OWNER_UNKNOWN}: Le responsable choisi n'existe pas dans le personnel."
        )));
    }
    let completed_at: Option<String> = (status == "done").then(|| now.clone());
    let id = match payload.id {
        None => {
            let sync_id = crate::sync::identity::new_entity_sync_id();
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO asset_corrective_actions (entity_sync_id, equipment_id, action_text, owner_personnel_id, due_date, status, completed_at, created_by_id, updated_by_id, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    sync_id.clone().into(),
                    payload.asset_id.into(),
                    action.into(),
                    owner.into(),
                    due.into(),
                    status.into(),
                    completed_at.into(),
                    actor.into(),
                    actor.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
            txn.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM asset_corrective_actions WHERE entity_sync_id = ?",
                [sync_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("corrective action created but not found")))?
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?
        }
        Some(id) => {
            // The completion time is kept while the action stays done, set when it becomes done
            // and cleared when it is reopened or cancelled.
            let changed = txn
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE asset_corrective_actions SET action_text = ?, owner_personnel_id = ?, due_date = ?, completed_at = CASE WHEN ? = 'done' THEN COALESCE(completed_at, ?) ELSE NULL END, status = ?, updated_by_id = ?, updated_at = ?, row_version = row_version + 1 WHERE id = ? AND equipment_id = ? AND row_version = ?",
                    [
                        action.into(),
                        owner.into(),
                        due.into(),
                        status.clone().into(),
                        now.clone().into(),
                        status.into(),
                        actor.into(),
                        now.into(),
                        id.into(),
                        payload.asset_id.into(),
                        payload.expected_row_version.unwrap_or(-1).into(),
                    ],
                ))
                .await?
                .rows_affected();
            if changed == 0 {
                return Err(refusal(format!(
                    "{GATE_ASSET_ACTION_CHANGED_ELSEWHERE}: Cette action a été modifiée ailleurs ; rechargez la liste avant de la modifier."
                )));
            }
            id
        }
    };
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ACTION_SELECT} WHERE a.id = ?"),
            [today().into(), id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("corrective action saved but not found")))?;
    let saved = map_action(&row)?;
    txn.commit().await?;
    Ok(saved)
}
