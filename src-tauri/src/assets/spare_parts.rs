//! Asset spare-parts list (SAP PM equipment BOM / Maximo asset spare parts).
//!
//! Each line links an asset to a stock article with the quantity installed on the asset and,
//! optionally, where it sits ("drive-end bearing") and whether it is a critical spare that must
//! be held in stock. Lines are deactivated, not deleted, so a removal replicates to the other
//! devices like any edit. Stock figures are read live from `stock_balances` across all locations.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{issue_params, org_validation_failed, AppError, AppResult};

/// Longest position label / note kept on a spare-parts line.
pub const POSITION_LABEL_MAX_CHARS: usize = 120;
pub const NOTES_MAX_CHARS: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSparePart {
    pub id: i64,
    pub asset_id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub unit_label: Option<String>,
    pub article_is_active: bool,
    pub quantity: f64,
    pub position_label: Option<String>,
    pub is_critical_spare: bool,
    pub notes: Option<String>,
    /// Sum over every stock location.
    pub on_hand_qty: f64,
    pub available_qty: f64,
    pub min_stock: f64,
    pub row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SparePartInput {
    pub quantity: f64,
    pub position_label: Option<String>,
    pub is_critical_spare: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddSparePartPayload {
    pub asset_id: i64,
    pub article_id: i64,
    pub line: SparePartInput,
}

/// Assets that use an article (inventory "where used").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleAssetUsage {
    pub asset_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub quantity: f64,
    pub is_critical_spare: bool,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "equipment_spare_parts decode failed for column '{column}': {e}"
    ))
}

fn get<T: sea_orm::TryGetable>(row: &QueryResult, column: &str) -> AppResult<T> {
    row.try_get::<T>("", column).map_err(|e| decode_err(column, e))
}

const LINE_SELECT: &str = "SELECT sp.id, sp.equipment_id, sp.article_id, sp.quantity, sp.position_label, \
    sp.is_critical_spare, sp.notes, sp.row_version, \
    a.article_code, a.article_name, a.is_active AS article_is_active, a.min_stock, u.label AS unit_label, \
    COALESCE((SELECT SUM(sb.on_hand_qty) FROM stock_balances sb WHERE sb.article_id = a.id), 0.0) AS on_hand_qty, \
    COALESCE((SELECT SUM(sb.available_qty) FROM stock_balances sb WHERE sb.article_id = a.id), 0.0) AS available_qty \
    FROM equipment_spare_parts sp \
    JOIN articles a ON a.id = sp.article_id \
    LEFT JOIN reference_values u ON u.id = a.unit_value_id";

fn map_line(row: &QueryResult) -> AppResult<AssetSparePart> {
    Ok(AssetSparePart {
        id: get(row, "id")?,
        asset_id: get(row, "equipment_id")?,
        article_id: get(row, "article_id")?,
        article_code: get(row, "article_code")?,
        article_name: get(row, "article_name")?,
        unit_label: get(row, "unit_label")?,
        article_is_active: get::<i64>(row, "article_is_active")? != 0,
        quantity: get(row, "quantity")?,
        position_label: get(row, "position_label")?,
        is_critical_spare: get::<i64>(row, "is_critical_spare")? != 0,
        notes: get(row, "notes")?,
        on_hand_qty: get(row, "on_hand_qty")?,
        available_qty: get(row, "available_qty")?,
        min_stock: get(row, "min_stock")?,
        row_version: get(row, "row_version")?,
    })
}

fn trimmed(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(ToString::to_string)
}

struct CleanLine {
    quantity: f64,
    position_label: Option<String>,
    is_critical_spare: bool,
    notes: Option<String>,
}

fn validate_line(line: &SparePartInput) -> AppResult<CleanLine> {
    if !line.quantity.is_finite() || line.quantity <= 0.0 {
        return Err(org_validation_failed(
            "ASSET_SPARE_QUANTITY_INVALID",
            "The quantity must be greater than zero.",
            issue_params(&[("value", line.quantity.to_string())]),
        ));
    }
    let position_label = trimmed(line.position_label.as_deref());
    if position_label
        .as_ref()
        .is_some_and(|p| p.chars().count() > POSITION_LABEL_MAX_CHARS)
    {
        return Err(org_validation_failed(
            "ASSET_SPARE_POSITION_TOO_LONG",
            format!("The position is limited to {POSITION_LABEL_MAX_CHARS} characters."),
            issue_params(&[("max", POSITION_LABEL_MAX_CHARS.to_string())]),
        ));
    }
    let notes = trimmed(line.notes.as_deref());
    if notes.as_ref().is_some_and(|n| n.chars().count() > NOTES_MAX_CHARS) {
        return Err(org_validation_failed(
            "ASSET_SPARE_NOTES_TOO_LONG",
            format!("Notes are limited to {NOTES_MAX_CHARS} characters."),
            issue_params(&[("max", NOTES_MAX_CHARS.to_string())]),
        ));
    }
    Ok(CleanLine {
        quantity: line.quantity,
        position_label,
        is_critical_spare: line.is_critical_spare,
        notes,
    })
}

async fn load_line(db: &impl ConnectionTrait, id: i64) -> AppResult<AssetSparePart> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LINE_SELECT} WHERE sp.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_spare_parts".into(),
            id: id.to_string(),
        })?;
    map_line(&row)
}

/// Active spare-parts lines of an asset, critical spares first.
pub async fn list_asset_spare_parts(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<Vec<AssetSparePart>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{LINE_SELECT} WHERE sp.equipment_id = ? AND sp.is_active = 1 \
                 ORDER BY sp.is_critical_spare DESC, a.article_code ASC"
            ),
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_line).collect()
}

/// Assets whose spare-parts list contains an article.
pub async fn list_article_asset_usage(db: &impl ConnectionTrait, article_id: i64) -> AppResult<Vec<ArticleAssetUsage>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id AS asset_id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                sp.quantity, sp.is_critical_spare \
             FROM equipment_spare_parts sp JOIN equipment e ON e.id = sp.equipment_id \
             WHERE sp.article_id = ? AND sp.is_active = 1 AND e.deleted_at IS NULL \
             ORDER BY e.asset_id_code ASC",
            [article_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(ArticleAssetUsage {
                asset_id: get(r, "asset_id")?,
                asset_code: get(r, "asset_code")?,
                asset_name: get(r, "asset_name")?,
                quantity: get(r, "quantity")?,
                is_critical_spare: get::<i64>(r, "is_critical_spare")? != 0,
            })
        })
        .collect()
}

/// Adds an article to an asset's spare-parts list.
pub async fn add_asset_spare_part(
    db: &DatabaseConnection,
    payload: AddSparePartPayload,
    actor_id: i64,
) -> AppResult<AssetSparePart> {
    let line = validate_line(&payload.line)?;
    let txn = db.begin().await?;
    let asset = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [payload.asset_id.into()],
        ))
        .await?;
    if asset.is_none() {
        return Err(AppError::NotFound {
            entity: "equipment".into(),
            id: payload.asset_id.to_string(),
        });
    }
    let article = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_code, is_active FROM articles WHERE id = ?",
            [payload.article_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "articles".into(),
            id: payload.article_id.to_string(),
        })?;
    let article_code: String = get(&article, "article_code")?;
    if get::<i64>(&article, "is_active")? == 0 {
        return Err(org_validation_failed(
            "ASSET_SPARE_ARTICLE_INACTIVE",
            format!("Article {article_code} is inactive."),
            issue_params(&[("code", article_code)]),
        ));
    }
    let duplicate = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_spare_parts WHERE equipment_id = ? AND article_id = ? AND is_active = 1",
            [payload.asset_id.into(), payload.article_id.into()],
        ))
        .await?;
    if duplicate.is_some() {
        return Err(org_validation_failed(
            "ASSET_SPARE_DUPLICATE",
            format!("Article {article_code} is already in this asset's spare-parts list."),
            issue_params(&[("code", article_code)]),
        ));
    }
    let now = crate::clock::now().to_rfc3339();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_spare_parts \
         (entity_sync_id, equipment_id, article_id, quantity, position_label, is_critical_spare, notes, \
          is_active, row_version, created_by_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 1, 1, ?, ?, ?)",
        [
            entity_sync_id.clone().into(),
            payload.asset_id.into(),
            payload.article_id.into(),
            line.quantity.into(),
            line.position_label.into(),
            i64::from(line.is_critical_spare).into(),
            line.notes.into(),
            actor_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let id: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_spare_parts WHERE entity_sync_id = ?",
            [entity_sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("spare-parts line missing after insert")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;
    let created = load_line(&txn, id).await?;
    txn.commit().await?;
    Ok(created)
}

/// Changes quantity, position, critical flag and notes of a line.
pub async fn update_asset_spare_part(
    db: &DatabaseConnection,
    id: i64,
    input: SparePartInput,
    expected_row_version: i64,
) -> AppResult<AssetSparePart> {
    let line = validate_line(&input)?;
    let txn = db.begin().await?;
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_spare_parts SET quantity = ?, position_label = ?, is_critical_spare = ?, \
             notes = ?, updated_at = ?, row_version = row_version + 1 \
             WHERE id = ? AND row_version = ? AND is_active = 1",
            [
                line.quantity.into(),
                line.position_label.into(),
                i64::from(line.is_critical_spare).into(),
                line.notes.into(),
                crate::clock::now().to_rfc3339().into(),
                id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(stale_or_missing(&txn, id).await);
    }
    let updated = load_line(&txn, id).await?;
    txn.commit().await?;
    Ok(updated)
}

/// Removes a line from the list (deactivation, replicated).
pub async fn remove_asset_spare_part(db: &DatabaseConnection, id: i64, expected_row_version: i64) -> AppResult<()> {
    let txn = db.begin().await?;
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_spare_parts SET is_active = 0, updated_at = ?, row_version = row_version + 1 \
             WHERE id = ? AND row_version = ? AND is_active = 1",
            [
                crate::clock::now().to_rfc3339().into(),
                id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(stale_or_missing(&txn, id).await);
    }
    txn.commit().await?;
    Ok(())
}

async fn stale_or_missing(db: &impl ConnectionTrait, id: i64) -> AppError {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_spare_parts WHERE id = ? AND is_active = 1",
            [id.into()],
        ))
        .await;
    match exists {
        Ok(Some(_)) => org_validation_failed(
            "ASSET_STALE_VERSION",
            "This line was changed by someone else. Reload it and try again.",
            issue_params(&[]),
        ),
        Ok(None) => AppError::NotFound {
            entity: "equipment_spare_parts".into(),
            id: id.to_string(),
        },
        Err(e) => AppError::from(e),
    }
}
