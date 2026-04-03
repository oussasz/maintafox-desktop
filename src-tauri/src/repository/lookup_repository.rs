use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult,
    Statement,
};
use serde::{Deserialize, Serialize};
use crate::errors::{AppError, AppResult};
use super::{Page, PageRequest, SearchFilter};

// ── DTOs ──────────────────────────────────────────────────────────────────

/// Summary row for a lookup domain (used in list views).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct LookupDomainSummary {
    pub id: i32,
    pub sync_id: String,
    pub domain_key: String,
    pub display_name: String,
    pub domain_type: String,
    pub is_extensible: i32,
    pub is_locked: i32,
    pub schema_version: i32,
    pub value_count: Option<i64>,
}

/// Full record for a single lookup value (used in detail and resolution).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct LookupValueRecord {
    pub id: i32,
    pub sync_id: String,
    pub domain_id: i32,
    pub code: String,
    pub label: String,
    pub fr_label: Option<String>,
    pub en_label: Option<String>,
    pub description: Option<String>,
    pub sort_order: i32,
    pub is_active: i32,
    pub is_system: i32,
    pub color: Option<String>,
    pub parent_value_id: Option<i32>,
}

/// Lightweight value row for dropdown population.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct LookupValueOption {
    pub id: i32,
    pub code: String,
    pub label: String,
    pub fr_label: Option<String>,
    pub en_label: Option<String>,
    pub color: Option<String>,
    pub is_active: i32,
}

// ── Query filters ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LookupDomainFilter {
    pub domain_type: Option<String>,
    #[serde(flatten)]
    pub search: SearchFilter,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LookupValueFilter {
    pub domain_id: Option<i32>,
    pub domain_key: Option<String>,
    pub is_active_only: Option<bool>,
    #[serde(flatten)]
    pub search: SearchFilter,
}

// ── Repository functions ──────────────────────────────────────────────────

/// Lists all lookup domains, with optional type filter.
/// Returns a paginated list of domain summary rows.
pub async fn list_lookup_domains(
    db: &DatabaseConnection,
    filter: &LookupDomainFilter,
    page: &PageRequest,
) -> AppResult<Page<LookupDomainSummary>> {
    // Count query
    let count_sql = format!(
        "SELECT COUNT(*) as cnt FROM lookup_domains WHERE deleted_at IS NULL{}{}",
        filter.search.query.as_ref().map(|_| " AND (display_name LIKE ? OR domain_key LIKE ?)").unwrap_or(""),
        filter.domain_type.as_ref().map(|_| " AND domain_type = ?").unwrap_or(""),
    );
    // List query with value count
    let list_sql = format!(
        r#"
        SELECT
            ld.id, ld.sync_id, ld.domain_key, ld.display_name, ld.domain_type,
            ld.is_extensible, ld.is_locked, ld.schema_version,
            (SELECT COUNT(*) FROM lookup_values lv
             WHERE lv.domain_id = ld.id AND lv.deleted_at IS NULL) AS value_count
        FROM lookup_domains ld
        WHERE ld.deleted_at IS NULL{}{}
        ORDER BY ld.domain_key ASC
        LIMIT {} OFFSET {}
        "#,
        filter.search.query.as_ref().map(|_| " AND (ld.display_name LIKE ? OR ld.domain_key LIKE ?)").unwrap_or(""),
        filter.domain_type.as_ref().map(|_| " AND ld.domain_type = ?").unwrap_or(""),
        page.limit(),
        page.offset(),
    );

    // Build bind values for count
    let q_pattern = filter.search.query.as_ref().map(|q| format!("%{q}%"));
    let mut count_binds: Vec<sea_orm::Value> = Vec::new();
    if let Some(p) = &q_pattern {
        count_binds.push(p.clone().into());
        count_binds.push(p.clone().into());
    }
    if let Some(dt) = &filter.domain_type {
        count_binds.push(dt.clone().into());
    }

    let count_stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &count_sql, count_binds);
    let count_result = db.query_one(count_stmt).await?;
    let total: u64 = count_result
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0) as u64;

    // Build bind values for list
    let mut list_binds: Vec<sea_orm::Value> = Vec::new();
    if let Some(p) = &q_pattern {
        list_binds.push(p.clone().into());
        list_binds.push(p.clone().into());
    }
    if let Some(dt) = &filter.domain_type {
        list_binds.push(dt.clone().into());
    }

    let list_stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &list_sql, list_binds);
    let rows = LookupDomainSummary::find_by_statement(list_stmt)
        .all(db)
        .await?;

    Ok(Page::new(rows, total, page))
}

/// Returns all active values for a given domain, ordered by sort_order.
/// This is the hot path for dropdown population — must be fast.
pub async fn get_domain_values(
    db: &DatabaseConnection,
    domain_key: &str,
    active_only: bool,
) -> AppResult<Vec<LookupValueOption>> {
    let active_clause = if active_only { " AND lv.is_active = 1" } else { "" };
    let sql = format!(
        r#"
        SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.color, lv.is_active
        FROM lookup_values lv
        INNER JOIN lookup_domains ld ON ld.id = lv.domain_id
        WHERE ld.domain_key = ?
          AND lv.deleted_at IS NULL
          {active_clause}
        ORDER BY lv.sort_order ASC, lv.label ASC
        "#,
    );
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        [domain_key.into()],
    );
    Ok(LookupValueOption::find_by_statement(stmt)
        .all(db)
        .await?)
}

/// Resolves a single lookup value by its integer id.
/// Used at render time to convert a stored FK to a displayable label.
pub async fn get_value_by_id(
    db: &DatabaseConnection,
    value_id: i32,
) -> AppResult<LookupValueRecord> {
    let sql = r#"
        SELECT id, sync_id, domain_id, code, label, fr_label, en_label,
               description, sort_order, is_active, is_system, color, parent_value_id
        FROM lookup_values
        WHERE id = ? AND deleted_at IS NULL
    "#;
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [value_id.into()]);
    LookupValueRecord::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "lookup_value".into(),
            id: value_id.to_string(),
        })
}

/// Looks up a value by its code within a named domain.
/// Used for import mapping and controlled vocabulary enforcement.
pub async fn get_value_by_code(
    db: &DatabaseConnection,
    domain_key: &str,
    code: &str,
) -> AppResult<LookupValueRecord> {
    let sql = r#"
        SELECT lv.id, lv.sync_id, lv.domain_id, lv.code, lv.label, lv.fr_label,
               lv.en_label, lv.description, lv.sort_order, lv.is_active, lv.is_system,
               lv.color, lv.parent_value_id
        FROM lookup_values lv
        INNER JOIN lookup_domains ld ON ld.id = lv.domain_id
        WHERE ld.domain_key = ? AND lv.code = ? AND lv.deleted_at IS NULL
    "#;
    let stmt = Statement::from_sql_and_values(
        DbBackend::Sqlite, sql,
        [domain_key.into(), code.into()],
    );
    LookupValueRecord::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: format!("lookup_value[{domain_key}]"),
            id: code.to_string(),
        })
}

/// Inserts a new lookup value into a domain.
/// Enforces: code must be unique within domain (via DB constraint).
/// Returns the newly assigned integer id.
pub async fn insert_lookup_value(
    db: &DatabaseConnection,
    domain_id: i32,
    code: &str,
    label: &str,
    fr_label: Option<&str>,
    en_label: Option<&str>,
    sort_order: i32,
    color: Option<&str>,
    _created_by_sync_id: &str,
) -> AppResult<i32> {
    use uuid::Uuid;
    use chrono::Utc;

    let sync_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let sql = r#"
        INSERT INTO lookup_values
            (sync_id, domain_id, code, label, fr_label, en_label,
             sort_order, is_active, is_system, color,
             created_at, updated_at, row_version)
        VALUES (?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?, ?, 1)
    "#;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [
            sync_id.into(),
            domain_id.into(),
            code.into(),
            label.into(),
            fr_label.map(|s| s.to_string()).into(),
            en_label.map(|s| s.to_string()).into(),
            sort_order.into(),
            color.map(|s| s.to_string()).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE constraint failed") {
            AppError::ValidationFailed(vec![format!(
                "Le code '{code}' existe d\u{00e9}j\u{00e0} dans ce domaine."
            )])
        } else {
            AppError::from(e)
        }
    })?;

    // Return the new id
    let row = db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT last_insert_rowid() as id;".to_string(),
    ))
    .await?;
    let new_id: i32 = row
        .and_then(|r| r.try_get::<i64>("", "id").ok())
        .unwrap_or(0) as i32;
    Ok(new_id)
}
