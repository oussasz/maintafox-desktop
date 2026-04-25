//! Work order lifecycle statuses (`work_order_statuses`) — engine contract + reference UI.
//!
//! Must stay aligned with migration `m20260409_000022_wo_domain_core` (12 canonical rows).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Canonical rows: (code, label_en, color_hex, macro_state, is_terminal, sequence)
const REQUIRED_WORK_ORDER_STATUSES: &[(&str, &str, &str, &str, i64, i64)] = &[
    ("draft", "Draft", "#94A3B8", "open", 0, 1),
    ("awaiting_approval", "Awaiting Approval", "#F59E0B", "open", 0, 2),
    ("planned", "Planned", "#3B82F6", "open", 0, 3),
    ("ready_to_schedule", "Ready To Schedule", "#6366F1", "open", 0, 4),
    ("assigned", "Assigned", "#8B5CF6", "executing", 0, 5),
    (
        "waiting_for_prerequisite",
        "Waiting For Prerequisite",
        "#F97316",
        "executing",
        0,
        6,
    ),
    ("in_progress", "In Progress", "#10B981", "executing", 0, 7),
    ("paused", "Paused", "#EF4444", "executing", 0, 8),
    (
        "mechanically_complete",
        "Mechanically Complete",
        "#06B6D4",
        "completed",
        0,
        9,
    ),
    (
        "technically_verified",
        "Technically Verified",
        "#22C55E",
        "completed",
        0,
        10,
    ),
    ("closed", "Closed", "#64748B", "closed", 1, 11),
    ("cancelled", "Cancelled", "#DC2626", "cancelled", 1, 12),
];

const REQUIRED_COUNT: usize = REQUIRED_WORK_ORDER_STATUSES.len();

#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderStatusOption {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub color: String,
    pub macro_state: String,
    pub is_terminal: bool,
    pub is_system: bool,
    pub sequence: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkOrderStatusInput {
    pub label: Option<String>,
    pub color: Option<String>,
}

fn validate_label(raw: &str, field: &str) -> AppResult<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!("{field} obligatoire.")]));
    }
    if s.len() > 120 {
        return Err(AppError::ValidationFailed(vec![format!("{field} trop long.")]));
    }
    Ok(s.to_string())
}

fn validate_color_hex(raw: &str) -> AppResult<String> {
    let s = raw.trim();
    if s.len() != 7 || !s.starts_with('#') {
        return Err(AppError::ValidationFailed(vec![
            "Couleur : format hex attendu (#RRGGBB).".into(),
        ]));
    }
    if !s.chars().skip(1).all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::ValidationFailed(vec![
            "Couleur : caractères hexadécimaux uniquement.".into(),
        ]));
    }
    Ok(s.to_ascii_uppercase())
}

fn map_status_row(row: &sea_orm::QueryResult) -> AppResult<WorkOrderStatusOption> {
    let id: i64 = row
        .try_get("", "id")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.id: {e}")]))?;
    let code: String = row
        .try_get("", "code")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.code: {e}")]))?;
    let label: String = row
        .try_get("", "label")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.label: {e}")]))?;
    let color: String = row
        .try_get("", "color")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.color: {e}")]))?;
    let macro_state: String = row
        .try_get("", "macro_state")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.macro_state: {e}")]))?;
    let is_terminal_raw: i64 = row
        .try_get("", "is_terminal")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.is_terminal: {e}")]))?;
    let is_system_raw: i64 = row
        .try_get("", "is_system")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.is_system: {e}")]))?;
    let sequence: i64 = row
        .try_get("", "sequence")
        .map_err(|e| AppError::ValidationFailed(vec![format!("work_order_statuses.sequence: {e}")]))?;

    Ok(WorkOrderStatusOption {
        id,
        code,
        label,
        color,
        macro_state,
        is_terminal: is_terminal_raw != 0,
        is_system: is_system_raw != 0,
        sequence,
    })
}

/// Idempotent upsert of the 12 system statuses (same pattern as `ensure_required_work_order_types`).
pub async fn ensure_required_work_order_statuses(db: &DatabaseConnection) -> AppResult<()> {
    for &(code, label, color, macro_state, is_terminal, sequence) in REQUIRED_WORK_ORDER_STATUSES {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_statuses (code, label, color, macro_state, is_terminal, is_system, sequence) \
             VALUES (?, ?, ?, ?, ?, 1, ?) \
             ON CONFLICT(code) DO UPDATE SET \
                label = excluded.label, \
                color = excluded.color, \
                macro_state = excluded.macro_state, \
                is_terminal = excluded.is_terminal, \
                is_system = 1, \
                sequence = excluded.sequence",
            [
                code.into(),
                label.into(),
                color.into(),
                macro_state.into(),
                is_terminal.into(),
                sequence.into(),
            ],
        ))
        .await?;
    }

    let mut missing: Vec<String> = Vec::new();
    for &(code, _, _, _, _, _) in REQUIRED_WORK_ORDER_STATUSES {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1",
                [code.into()],
            ))
            .await?;
        if row.is_none() {
            missing.push(code.to_string());
        }
    }

    if !missing.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "work_order_statuses integrity check failed; missing codes: {}",
            missing.join(", ")
        )));
    }

    Ok(())
}

/// Fast path before hot paths: repair empty / partial catalogs without re-upserting every time.
pub async fn ensure_work_order_statuses_if_needed(db: &DatabaseConnection) -> AppResult<()> {
    let count_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_order_statuses".to_string(),
        ))
        .await?;
    let n: i64 = count_row
        .as_ref()
        .and_then(|r| r.try_get::<i64>("", "c").ok())
        .unwrap_or(0);
    if n < REQUIRED_COUNT as i64 {
        ensure_required_work_order_statuses(db).await?;
    }
    Ok(())
}

pub async fn list_work_order_statuses(db: &DatabaseConnection) -> AppResult<Vec<WorkOrderStatusOption>> {
    ensure_work_order_statuses_if_needed(db).await?;

    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, label, color, macro_state, is_terminal, is_system, sequence \
             FROM work_order_statuses \
             ORDER BY sequence ASC, id ASC"
                .to_string(),
        ))
        .await?;

    let mut out: Vec<WorkOrderStatusOption> = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_status_row(&row)?);
    }
    Ok(out)
}

pub async fn update_work_order_status(
    db: &DatabaseConnection,
    id: i64,
    input: UpdateWorkOrderStatusInput,
) -> AppResult<WorkOrderStatusOption> {
    let current = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, color, macro_state, is_terminal, is_system, sequence \
             FROM work_order_statuses WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderStatus".into(),
            id: id.to_string(),
        })?;

    let mut sets: Vec<&'static str> = Vec::new();
    let mut params: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref l) = input.label {
        let v = validate_label(l, "Libellé")?;
        sets.push("label = ?");
        params.push(v.into());
    }

    if let Some(ref c) = input.color {
        let v = validate_color_hex(c)?;
        sets.push("color = ?");
        params.push(v.into());
    }

    if sets.is_empty() {
        return map_status_row(&current);
    }

    params.push(id.into());
    let sql = format!(
        "UPDATE work_order_statuses SET {} WHERE id = ?",
        sets.join(", ")
    );

    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, color, macro_state, is_terminal, is_system, sequence \
             FROM work_order_statuses WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderStatus".into(),
            id: id.to_string(),
        })?;

    map_status_row(&row)
}
