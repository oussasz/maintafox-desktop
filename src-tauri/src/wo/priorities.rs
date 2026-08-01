//! Work order priority / urgency level catalog (`urgency_levels`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Canonical 5-level scale (must stay aligned with migration `m20260409_000022` + `m20260720_000102`).
const REQUIRED_URGENCY_LEVELS: &[(i64, &str, &str, &str, &str)] = &[
    (1, "very_low", "Very Low", "Très basse", "#64748B"),
    (2, "low", "Low", "Basse", "#3B82F6"),
    (3, "medium", "Medium", "Normale", "#F59E0B"),
    (4, "high", "High", "Haute", "#F97316"),
    (5, "critical", "Critical", "Urgente", "#DC2626"),
];

/// Idempotent repair: ensures `urgency_levels` always contains the system scale (mirrors `ensure_required_work_order_types`).
pub async fn ensure_required_urgency_levels(db: &DatabaseConnection) -> AppResult<()> {
    for &(level, code, label, label_fr, hex) in REQUIRED_URGENCY_LEVELS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO urgency_levels (level, label, label_fr, hex_color, code, is_system, is_active) \
             VALUES (?, ?, ?, ?, ?, 1, 1) \
             ON CONFLICT(level) DO UPDATE SET \
                label = excluded.label, \
                label_fr = excluded.label_fr, \
                hex_color = excluded.hex_color, \
                code = COALESCE(urgency_levels.code, excluded.code), \
                is_system = 1, \
                is_active = CASE \
                    WHEN urgency_levels.is_active IS NULL THEN 1 \
                    ELSE urgency_levels.is_active \
                END",
            [
                level.into(),
                label.into(),
                label_fr.into(),
                hex.into(),
                code.into(),
            ],
        ))
        .await?;
    }

    let mut missing: Vec<i64> = Vec::new();
    for &(level, _, _, _, _) in REQUIRED_URGENCY_LEVELS {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM urgency_levels WHERE level = ? LIMIT 1",
                [level.into()],
            ))
            .await?;
        if row.is_none() {
            missing.push(level);
        }
    }

    if !missing.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "urgency_levels integrity check failed; missing levels: {:?}",
            missing
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderPriorityOption {
    pub id: i64,
    pub level: i64,
    pub code: String,
    pub label: String,
    pub label_fr: String,
    pub hex_color: String,
    pub is_system: bool,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkOrderPriorityInput {
    pub label: Option<String>,
    pub label_fr: Option<String>,
    pub is_active: Option<bool>,
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

async fn priority_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<WorkOrderPriorityOption>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, level, code, label, label_fr, hex_color, is_system, is_active \
             FROM urgency_levels WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(map_priority_row(&row)?))
}

fn map_priority_row(row: &sea_orm::QueryResult) -> AppResult<WorkOrderPriorityOption> {
    let id: i64 = row
        .try_get("", "id")
        .map_err(|e| AppError::ValidationFailed(vec![format!("urgency_levels.id: {e}")]))?;
    let level: i64 = row
        .try_get("", "level")
        .map_err(|e| AppError::ValidationFailed(vec![format!("urgency_levels.level: {e}")]))?;
    let code: String = row
        .try_get("", "code")
        .unwrap_or_else(|_| String::new());
    let label: String = row
        .try_get("", "label")
        .map_err(|e| AppError::ValidationFailed(vec![format!("urgency_levels.label: {e}")]))?;
    let label_fr: String = row
        .try_get("", "label_fr")
        .map_err(|e| AppError::ValidationFailed(vec![format!("urgency_levels.label_fr: {e}")]))?;
    let hex_color: String = row
        .try_get("", "hex_color")
        .map_err(|e| AppError::ValidationFailed(vec![format!("urgency_levels.hex_color: {e}")]))?;
    let is_system_raw: i64 = row.try_get("", "is_system").unwrap_or(1);
    let is_active_raw: i64 = row.try_get("", "is_active").unwrap_or(1);

    Ok(WorkOrderPriorityOption {
        id,
        level,
        code,
        label,
        label_fr,
        hex_color,
        is_system: is_system_raw != 0,
        is_active: is_active_raw != 0,
    })
}

pub async fn list_work_order_priorities(db: &DatabaseConnection) -> AppResult<Vec<WorkOrderPriorityOption>> {
    let count_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM urgency_levels".to_string(),
        ))
        .await?;
    let n: i64 = count_row
        .as_ref()
        .and_then(|r| r.try_get::<i64>("", "c").ok())
        .unwrap_or(0);
    if n < REQUIRED_URGENCY_LEVELS.len() as i64 {
        ensure_required_urgency_levels(db).await?;
    }

    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, level, code, label, label_fr, hex_color, is_system, is_active \
             FROM urgency_levels \
             ORDER BY level ASC"
                .to_string(),
        ))
        .await?;

    let mut out: Vec<WorkOrderPriorityOption> = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_priority_row(&row)?);
    }
    Ok(out)
}

pub async fn update_work_order_priority(
    db: &DatabaseConnection,
    id: i64,
    input: UpdateWorkOrderPriorityInput,
) -> AppResult<WorkOrderPriorityOption> {
    let current = priority_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UrgencyLevel".into(),
            id: id.to_string(),
        })?;

    let mut sets: Vec<&'static str> = Vec::new();
    let mut params: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref l) = input.label {
        let v = validate_label(l, "Libellé (EN)")?;
        sets.push("label = ?");
        params.push(v.into());
    }

    if let Some(ref l) = input.label_fr {
        let v = validate_label(l, "Libellé (FR)")?;
        sets.push("label_fr = ?");
        params.push(v.into());
    }

    if let Some(active) = input.is_active {
        sets.push("is_active = ?");
        params.push((if active { 1 } else { 0 }).into());
    }

    if sets.is_empty() {
        return Ok(current);
    }

    params.push(id.into());
    let sql = format!(
        "UPDATE urgency_levels SET {} WHERE id = ?",
        sets.join(", ")
    );

    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;

    priority_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UrgencyLevel".into(),
            id: id.to_string(),
        })
}
