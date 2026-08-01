use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderTypeOption {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub is_system: bool,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkOrderTypeInput {
    pub code: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkOrderTypeInput {
    pub label: Option<String>,
    pub code: Option<String>,
    pub is_active: Option<bool>,
}

pub const REQUIRED_WORK_ORDER_TYPES: [(&str, &str); 7] = [
    ("corrective", "Corrective"),
    ("preventive", "Preventive"),
    ("improvement", "Improvement"),
    ("inspection", "Inspection"),
    ("emergency", "Emergency"),
    ("overhaul", "Overhaul"),
    ("condition_based", "Condition-Based"),
];

fn normalize_type_code(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub async fn ensure_required_work_order_types(db: &DatabaseConnection) -> AppResult<()> {
    for (code, label) in REQUIRED_WORK_ORDER_TYPES {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_types (code, label, is_system, is_active) \
             VALUES (?, ?, 1, 1) \
             ON CONFLICT(code) DO UPDATE SET \
                label = excluded.label, \
                is_system = 1, \
                is_active = CASE \
                    WHEN work_order_types.is_active IS NULL THEN 1 \
                    ELSE work_order_types.is_active \
                END",
            [code.into(), label.into()],
        ))
        .await?;
    }

    let mut missing: Vec<String> = Vec::new();
    for (code, _) in REQUIRED_WORK_ORDER_TYPES {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_types WHERE lower(code) = ? LIMIT 1",
                [code.into()],
            ))
            .await?;
        if row.is_none() {
            missing.push(code.to_string());
        }
    }

    if !missing.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "work_order_types integrity check failed. Missing required codes: {}",
            missing.join(", ")
        )));
    }

    Ok(())
}

pub async fn resolve_work_order_type_id_by_code(
    db: &DatabaseConnection,
    type_code: &str,
) -> AppResult<i64> {
    let normalized = normalize_type_code(type_code);
    if normalized.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le type d'OT est obligatoire (type_code).".into(),
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, is_active FROM work_order_types WHERE lower(code) = ? LIMIT 1",
            [normalized.clone().into()],
        ))
        .await?;

    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT introuvable (type_code='{}').",
            normalized
        )]));
    };

    let id: i64 = row.try_get("", "id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "work_order_types.id decode failed for type_code='{}': {e}",
            normalized
        ))
    })?;
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT inactif (type_code='{}').",
            normalized
        )]));
    }

    Ok(id)
}

pub async fn resolve_work_order_type_code_by_id(
    db: &DatabaseConnection,
    type_id: i64,
) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code, is_active FROM work_order_types WHERE id = ? LIMIT 1",
            [type_id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT introuvable (type_id={type_id})."
        )]));
    };

    let code: String = row.try_get("", "code").map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "work_order_types.code decode failed for type_id={type_id}: {e}"
        ))
    })?;
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT inactif (type_id={type_id})."
        )]));
    }

    Ok(code)
}

pub async fn list_work_order_types(db: &DatabaseConnection) -> AppResult<Vec<WorkOrderTypeOption>> {
    ensure_required_work_order_types(db).await?;

    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, label, is_system, is_active \
             FROM work_order_types \
             ORDER BY is_system DESC, code ASC"
                .to_string(),
        ))
        .await?;

    let mut out: Vec<WorkOrderTypeOption> = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row.try_get("", "id").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("work_order_types.id decode: {e}"))
        })?;
        let code: String = row.try_get("", "code").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("work_order_types.code decode: {e}"))
        })?;
        let label: String = row.try_get("", "label").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("work_order_types.label decode: {e}"))
        })?;
        let is_system_raw: i64 = row.try_get("", "is_system").unwrap_or(0);
        let is_active_raw: i64 = row.try_get("", "is_active").unwrap_or(1);
        out.push(WorkOrderTypeOption {
            id,
            code,
            label,
            is_system: is_system_raw != 0,
            is_active: is_active_raw != 0,
        });
    }

    Ok(out)
}

fn validate_label(raw: &str) -> AppResult<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Libellé obligatoire.".into()]));
    }
    if s.len() > 200 {
        return Err(AppError::ValidationFailed(vec!["Libellé trop long (max 200).".into()]));
    }
    Ok(s.to_string())
}

fn validate_custom_type_code(raw: &str) -> AppResult<String> {
    let s = normalize_type_code(raw);
    if s.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Code obligatoire.".into()]));
    }
    if s.len() > 64 {
        return Err(AppError::ValidationFailed(vec!["Code trop long (max 64).".into()]));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AppError::ValidationFailed(vec![
            "Le code ne peut contenir que des lettres minuscules, chiffres et underscores.".into(),
        ]));
    }
    Ok(s)
}

async fn work_order_type_by_id(
    db: &DatabaseConnection,
    id: i64,
) -> AppResult<Option<WorkOrderTypeOption>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, is_system, is_active \
             FROM work_order_types WHERE id = ? LIMIT 1",
            [id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let id: i64 = row.try_get("", "id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("work_order_types.id decode: {e}"))
    })?;
    let code: String = row.try_get("", "code").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("work_order_types.code decode: {e}"))
    })?;
    let label: String = row.try_get("", "label").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("work_order_types.label decode: {e}"))
    })?;
    let is_system_raw: i64 = row.try_get("", "is_system").unwrap_or(0);
    let is_active_raw: i64 = row.try_get("", "is_active").unwrap_or(1);
    Ok(Some(WorkOrderTypeOption {
        id,
        code,
        label,
        is_system: is_system_raw != 0,
        is_active: is_active_raw != 0,
    }))
}

pub async fn create_work_order_type(
    db: &DatabaseConnection,
    input: CreateWorkOrderTypeInput,
) -> AppResult<WorkOrderTypeOption> {
    ensure_required_work_order_types(db).await?;

    let code = validate_custom_type_code(&input.code)?;
    let label = validate_label(&input.label)?;

    let dup = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_types WHERE lower(code) = ? LIMIT 1",
            [code.clone().into()],
        ))
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Un type avec le code « {code} » existe déjà."
        )]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_types (code, label, is_system, is_active) VALUES (?, ?, 0, 1)",
        [code.into(), label.into()],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("work_order_types insert id missing")))?;
    let new_id: i64 = row.try_get("", "id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("last_insert_rowid decode: {e}"))
    })?;

    work_order_type_by_id(db, new_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderType".into(),
            id: new_id.to_string(),
        })
}

pub async fn update_work_order_type(
    db: &DatabaseConnection,
    id: i64,
    input: UpdateWorkOrderTypeInput,
) -> AppResult<WorkOrderTypeOption> {
    ensure_required_work_order_types(db).await?;

    let current = work_order_type_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderType".into(),
            id: id.to_string(),
        })?;

    if current.is_system {
        if let Some(ref c) = input.code {
            let normalized = normalize_type_code(c);
            if !normalized.is_empty() && normalized != normalize_type_code(&current.code) {
                return Err(AppError::ValidationFailed(vec![
                    "Le code des types système est verrouillé.".into(),
                ]));
            }
        }
    }

    let mut sets: Vec<&'static str> = Vec::new();
    let mut params: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref l) = input.label {
        let v = validate_label(l)?;
        sets.push("label = ?");
        params.push(v.into());
    }

    if let Some(active) = input.is_active {
        sets.push("is_active = ?");
        params.push((if active { 1 } else { 0 }).into());
    }

    if !current.is_system {
        if let Some(ref c) = input.code {
            let new_code = validate_custom_type_code(c)?;
            if new_code != normalize_type_code(&current.code) {
                let dup = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT id FROM work_order_types WHERE lower(code) = ? AND id != ? LIMIT 1",
                        [new_code.clone().into(), id.into()],
                    ))
                    .await?;
                if dup.is_some() {
                    return Err(AppError::ValidationFailed(vec![format!(
                        "Un type avec le code « {new_code} » existe déjà."
                    )]));
                }
                sets.push("code = ?");
                params.push(new_code.into());
            }
        }
    }

    if sets.is_empty() {
        return Ok(current);
    }

    params.push(id.into());
    let sql = format!(
        "UPDATE work_order_types SET {} WHERE id = ?",
        sets.join(", ")
    );

    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;

    work_order_type_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderType".into(),
            id: id.to_string(),
        })
}

pub async fn delete_work_order_type(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    ensure_required_work_order_types(db).await?;

    let current = work_order_type_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrderType".into(),
            id: id.to_string(),
        })?;

    if current.is_system {
        return Err(AppError::ValidationFailed(vec![
            "Impossible de supprimer un type système.".into(),
        ]));
    }

    let cnt: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_orders WHERE type_id = ?",
            [id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "c").ok())
        .unwrap_or(0);

    if cnt > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de supprimer ce type : {cnt} ordre(s) de travail y sont liés. Désactivez-le plutôt."
        )]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM work_order_types WHERE id = ? AND is_system = 0",
        [id.into()],
    ))
    .await?;

    Ok(())
}
