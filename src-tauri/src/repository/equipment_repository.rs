use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use crate::errors::{AppError, AppResult};
use super::{Page, PageRequest};

// ── DTOs ──────────────────────────────────────────────────────────────────

/// Equipment list row (dense table view).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct EquipmentListRow {
    pub id: i32,
    pub sync_id: String,
    pub asset_id_code: String,
    pub name: String,
    pub lifecycle_status: String,
    pub class_id: Option<i32>,
    pub class_name: Option<String>,
    pub installed_at_node_id: Option<i32>,
    pub node_name: Option<String>,
    pub criticality_value_id: Option<i32>,
    pub criticality_label: Option<String>,
    pub criticality_color: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

/// Equipment detail record (full fields for the detail panel).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct EquipmentDetail {
    pub id: i32,
    pub sync_id: String,
    pub asset_id_code: String,
    pub name: String,
    pub lifecycle_status: String,
    pub class_id: Option<i32>,
    pub class_name: Option<String>,
    pub installed_at_node_id: Option<i32>,
    pub node_name: Option<String>,
    pub criticality_value_id: Option<i32>,
    pub criticality_label: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub purchase_date: Option<String>,
    pub commissioning_date: Option<String>,
    pub warranty_expiry_date: Option<String>,
    pub replacement_value: Option<f64>,
    pub erp_asset_id: Option<String>,
    pub iot_asset_id: Option<String>,
    pub qr_code: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub row_version: i32,
}

// ── Query filters ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EquipmentFilter {
    pub node_id: Option<i32>,
    pub class_id: Option<i32>,
    pub lifecycle_status: Option<String>,
    pub criticality_value_id: Option<i32>,
    pub query: Option<String>,
    /// Include equipment in descendant nodes of `node_id`
    pub include_descendants: Option<bool>,
    pub ancestor_path_prefix: Option<String>,
}

// ── Repository functions ──────────────────────────────────────────────────

/// Paginated equipment list with optional filters.
pub async fn list_equipment(
    db: &DatabaseConnection,
    filter: &EquipmentFilter,
    page: &PageRequest,
) -> AppResult<Page<EquipmentListRow>> {
    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(node_id) = filter.node_id {
        if filter.include_descendants.unwrap_or(false) {
            if let Some(ref prefix) = filter.ancestor_path_prefix {
                where_clauses.push("n.ancestor_path LIKE ?".to_string());
                binds.push(format!("{prefix}%").into());
            } else {
                where_clauses.push("e.installed_at_node_id = ?".to_string());
                binds.push(node_id.into());
            }
        } else {
            where_clauses.push("e.installed_at_node_id = ?".to_string());
            binds.push(node_id.into());
        }
    }
    if let Some(ref status) = filter.lifecycle_status {
        where_clauses.push("e.lifecycle_status = ?".to_string());
        binds.push(status.clone().into());
    }
    if let Some(class_id) = filter.class_id {
        where_clauses.push("e.class_id = ?".to_string());
        binds.push(class_id.into());
    }
    if let Some(crit_id) = filter.criticality_value_id {
        where_clauses.push("e.criticality_value_id = ?".to_string());
        binds.push(crit_id.into());
    }
    if let Some(ref q) = filter.query {
        where_clauses.push(
            "(e.asset_id_code LIKE ? OR e.name LIKE ? OR e.serial_number LIKE ?)".to_string(),
        );
        let p = format!("%{q}%");
        binds.push(p.clone().into());
        binds.push(p.clone().into());
        binds.push(p.into());
    }

    let where_sql = where_clauses.join(" AND ");

    let count_sql = format!(
        r#"SELECT COUNT(*) as cnt FROM equipment e
           LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
           WHERE {where_sql}"#,
    );
    let list_sql = format!(
        r#"
        SELECT e.id, e.sync_id, e.asset_id_code, e.name, e.lifecycle_status,
               e.class_id, ec.name AS class_name,
               e.installed_at_node_id, n.name AS node_name,
               e.criticality_value_id, lv.label AS criticality_label, lv.color AS criticality_color,
               e.manufacturer, e.model
        FROM equipment e
        LEFT JOIN equipment_classes ec ON ec.id = e.class_id
        LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
        LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id
        WHERE {where_sql}
        ORDER BY e.asset_id_code ASC
        LIMIT {} OFFSET {}
        "#,
        page.limit(),
        page.offset()
    );

    let count_result = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &count_sql,
            binds.clone(),
        ))
        .await?;
    let total = count_result
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0) as u64;

    let rows = EquipmentListRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &list_sql,
        binds,
    ))
    .all(db)
    .await?;

    Ok(Page::new(rows, total, page))
}

/// Fetches full equipment detail by id.
pub async fn get_equipment_by_id(
    db: &DatabaseConnection,
    equipment_id: i32,
) -> AppResult<EquipmentDetail> {
    let sql = r#"
        SELECT e.id, e.sync_id, e.asset_id_code, e.name, e.lifecycle_status,
               e.class_id, ec.name AS class_name,
               e.installed_at_node_id, n.name AS node_name,
               e.criticality_value_id, lv.label AS criticality_label,
               e.manufacturer, e.model, e.serial_number, e.purchase_date,
               e.commissioning_date, e.warranty_expiry_date, e.replacement_value,
               e.erp_asset_id, e.iot_asset_id, e.qr_code, e.notes,
               e.created_at, e.updated_at, e.row_version
        FROM equipment e
        LEFT JOIN equipment_classes ec ON ec.id = e.class_id
        LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
        LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id
        WHERE e.id = ? AND e.deleted_at IS NULL
    "#;
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [equipment_id.into()]);
    EquipmentDetail::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: equipment_id.to_string(),
        })
}
