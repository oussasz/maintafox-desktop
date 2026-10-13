//! Unit conversions (Phase 6.8): `1 from_unit = factor × to_unit` between units of the stock unit
//! domain, for every article or for one article. A conversion for the article wins over the
//! general one; the inverse direction uses `1 / factor`.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Lookup domain of stock units.
pub const UNIT_DOMAIN_KEY: &str = "inventory.unit_of_measure";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnitConversion {
    pub id: i64,
    pub entity_sync_id: String,
    pub from_unit_value_id: i64,
    pub from_unit_code: String,
    pub from_unit_label: String,
    pub to_unit_value_id: i64,
    pub to_unit_code: String,
    pub to_unit_label: String,
    pub article_id: Option<i64>,
    pub article_code: Option<String>,
    pub factor: f64,
    pub notes: Option<String>,
    pub is_active: bool,
    pub row_version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateUnitConversionInput {
    pub from_unit_value_id: i64,
    pub to_unit_value_id: i64,
    #[serde(default)]
    pub article_id: Option<i64>,
    pub factor: f64,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUnitConversionInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub factor: f64,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConvertedQuantity {
    pub quantity: f64,
    /// Multiplier applied to the input quantity.
    pub factor: f64,
    /// `identity`, `direct` or `inverse`.
    pub path: String,
    pub conversion_id: Option<i64>,
}

const SELECT: &str = "SELECT c.id, c.entity_sync_id, c.from_unit_value_id, fu.code AS from_code, \
     fu.label AS from_label, c.to_unit_value_id, tu.code AS to_code, tu.label AS to_label, c.article_id, \
     a.article_code, c.factor, c.notes, c.is_active, c.row_version, c.updated_at \
     FROM unit_conversions c \
     JOIN lookup_values fu ON fu.id = c.from_unit_value_id \
     JOIN lookup_values tu ON tu.id = c.to_unit_value_id \
     LEFT JOIN articles a ON a.id = c.article_id";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("unit_conversions decode '{field}': {err}"))
}

fn map_row(r: &sea_orm::QueryResult) -> AppResult<UnitConversion> {
    let active: i64 = r.try_get("", "is_active").map_err(|e| decode_err("is_active", e))?;
    Ok(UnitConversion {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        from_unit_value_id: r
            .try_get("", "from_unit_value_id")
            .map_err(|e| decode_err("from_unit_value_id", e))?,
        from_unit_code: r.try_get("", "from_code").map_err(|e| decode_err("from_code", e))?,
        from_unit_label: r.try_get("", "from_label").map_err(|e| decode_err("from_label", e))?,
        to_unit_value_id: r
            .try_get("", "to_unit_value_id")
            .map_err(|e| decode_err("to_unit_value_id", e))?,
        to_unit_code: r.try_get("", "to_code").map_err(|e| decode_err("to_code", e))?,
        to_unit_label: r.try_get("", "to_label").map_err(|e| decode_err("to_label", e))?,
        article_id: r.try_get("", "article_id").map_err(|e| decode_err("article_id", e))?,
        article_code: r
            .try_get("", "article_code")
            .map_err(|e| decode_err("article_code", e))?,
        factor: r.try_get("", "factor").map_err(|e| decode_err("factor", e))?,
        notes: r.try_get("", "notes").map_err(|e| decode_err("notes", e))?,
        is_active: active != 0,
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        updated_at: r.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_unit_conversions(
    conn: &impl ConnectionTrait,
    article_id: Option<i64>,
    include_inactive: bool,
) -> AppResult<Vec<UnitConversion>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{SELECT} WHERE (? = 1 OR c.is_active = 1) \
                 AND (? IS NULL OR c.article_id IS NULL OR c.article_id = ?) \
                 ORDER BY fu.code, tu.code, c.article_id"
            ),
            [i64::from(include_inactive).into(), article_id.into(), article_id.into()],
        ))
        .await?;
    rows.iter().map(map_row).collect()
}

pub async fn get_unit_conversion(conn: &impl ConnectionTrait, id: i64) -> AppResult<UnitConversion> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SELECT} WHERE c.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "unit_conversion".into(),
            id: id.to_string(),
        })?;
    map_row(&row)
}

fn validate_factor(factor: f64) -> AppResult<()> {
    if factor.is_finite() && factor > 0.0 {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![
            "factor must be a positive number.".into()
        ]))
    }
}

async fn ensure_stock_unit(conn: &impl ConnectionTrait, value_id: i64, field: &str) -> AppResult<()> {
    let ok = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE lv.id = ? AND ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [value_id.into(), UNIT_DOMAIN_KEY.into()],
        ))
        .await?
        .is_some();
    if ok {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "{field} must be an active stock unit."
        )]))
    }
}

/// Active conversions between the two units (either direction) in one article scope:
/// `(id, from_unit_value_id, factor)`.
async fn active_pair(
    conn: &impl ConnectionTrait,
    a: i64,
    b: i64,
    article_id: Option<i64>,
) -> AppResult<Vec<(i64, i64, f64)>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, from_unit_value_id, factor FROM unit_conversions \
             WHERE is_active = 1 AND ((from_unit_value_id = ? AND to_unit_value_id = ?) \
                OR (from_unit_value_id = ? AND to_unit_value_id = ?)) \
               AND ((? IS NULL AND article_id IS NULL) OR article_id = ?) ORDER BY id",
            [
                a.into(),
                b.into(),
                b.into(),
                a.into(),
                article_id.into(),
                article_id.into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                r.try_get("", "from_unit_value_id")
                    .map_err(|e| decode_err("from_unit_value_id", e))?,
                r.try_get("", "factor").map_err(|e| decode_err("factor", e))?,
            ))
        })
        .collect()
}

fn clean_notes(notes: Option<String>) -> Option<String> {
    notes.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())
}

pub async fn create_unit_conversion<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    actor_id: i64,
    input: CreateUnitConversionInput,
) -> AppResult<UnitConversion> {
    validate_factor(input.factor)?;
    if input.from_unit_value_id == input.to_unit_value_id {
        return Err(AppError::ValidationFailed(
            vec!["from and to units must differ.".into()],
        ));
    }
    let txn = db.begin().await?;
    ensure_stock_unit(&txn, input.from_unit_value_id, "from_unit_value_id").await?;
    ensure_stock_unit(&txn, input.to_unit_value_id, "to_unit_value_id").await?;
    if let Some(article_id) = input.article_id {
        let exists = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM articles WHERE id = ?",
                [article_id.into()],
            ))
            .await?
            .is_some();
        if !exists {
            return Err(AppError::NotFound {
                entity: "article".into(),
                id: article_id.to_string(),
            });
        }
    }
    if !active_pair(&txn, input.from_unit_value_id, input.to_unit_value_id, input.article_id)
        .await?
        .is_empty()
    {
        return Err(AppError::ValidationFailed(vec![
            "A conversion between these units already exists for this scope (the inverse is derived).".into(),
        ]));
    }
    let sync_id = crate::sync::identity::new_entity_sync_id();
    let now = chrono::Utc::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO unit_conversions (entity_sync_id, from_unit_value_id, to_unit_value_id, article_id, factor, \
         notes, is_active, row_version, created_by_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 1, 1, ?, ?, ?)",
        [
            sync_id.clone().into(),
            input.from_unit_value_id.into(),
            input.to_unit_value_id.into(),
            input.article_id.into(),
            input.factor.into(),
            clean_notes(input.notes).into(),
            actor_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let id: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM unit_conversions WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("unit_conversions row missing after insert")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;
    let created = get_unit_conversion(&txn, id).await?;
    txn.commit().await?;
    Ok(created)
}

/// Optimistic update of one row (`row_version` must match).
async fn bump(
    conn: &impl ConnectionTrait,
    id: i64,
    expected_row_version: i64,
    set_sql: &str,
    mut values: Vec<sea_orm::Value>,
) -> AppResult<()> {
    values.push(chrono::Utc::now().to_rfc3339().into());
    values.push(id.into());
    values.push(expected_row_version.into());
    let res = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE unit_conversions SET {set_sql}, row_version = row_version + 1, updated_at = ? \
                 WHERE id = ? AND row_version = ?"
            ),
            values,
        ))
        .await?;
    if res.rows_affected() == 0 {
        get_unit_conversion(conn, id).await?;
        return Err(AppError::ValidationFailed(vec![
            "The conversion was changed meanwhile; reload it.".into(),
        ]));
    }
    Ok(())
}

pub async fn update_unit_conversion(
    conn: &impl ConnectionTrait,
    input: UpdateUnitConversionInput,
) -> AppResult<UnitConversion> {
    validate_factor(input.factor)?;
    bump(
        conn,
        input.id,
        input.expected_row_version,
        "factor = ?, notes = ?",
        vec![input.factor.into(), clean_notes(input.notes).into()],
    )
    .await?;
    get_unit_conversion(conn, input.id).await
}

pub async fn deactivate_unit_conversion(
    conn: &impl ConnectionTrait,
    id: i64,
    expected_row_version: i64,
) -> AppResult<UnitConversion> {
    bump(conn, id, expected_row_version, "is_active = 0", vec![]).await?;
    get_unit_conversion(conn, id).await
}

/// Converts `quantity` from one unit to another: the article's own conversion first, then the
/// general one; direct or inverse. Fails when no conversion exists, or when two active
/// conversions of the same scope disagree (possible after two devices defined the pair offline).
pub async fn convert_quantity(
    conn: &impl ConnectionTrait,
    quantity: f64,
    from_unit_value_id: i64,
    to_unit_value_id: i64,
    article_id: Option<i64>,
) -> AppResult<ConvertedQuantity> {
    if !quantity.is_finite() {
        return Err(AppError::ValidationFailed(vec!["quantity must be a number.".into()]));
    }
    if from_unit_value_id == to_unit_value_id {
        return Ok(ConvertedQuantity {
            quantity,
            factor: 1.0,
            path: "identity".into(),
            conversion_id: None,
        });
    }
    let scopes: Vec<Option<i64>> = match article_id {
        Some(a) => vec![Some(a), None],
        None => vec![None],
    };
    let multiplier = |from: i64, factor: f64| {
        if from == from_unit_value_id {
            factor
        } else {
            1.0 / factor
        }
    };
    for scope in scopes {
        let found = active_pair(conn, from_unit_value_id, to_unit_value_id, scope).await?;
        let Some(&(id, from, factor)) = found.first() else {
            continue;
        };
        let m = multiplier(from, factor);
        if found
            .iter()
            .any(|&(_, f, x)| (multiplier(f, x) - m).abs() > 1e-9 * m.abs().max(1.0))
        {
            return Err(AppError::ValidationFailed(vec![
                "Several active conversions between these units disagree; deactivate the wrong one.".into(),
            ]));
        }
        return Ok(ConvertedQuantity {
            quantity: quantity * m,
            factor: m,
            path: if from == from_unit_value_id {
                "direct"
            } else {
                "inverse"
            }
            .into(),
            conversion_id: Some(id),
        });
    }
    Err(AppError::ValidationFailed(vec![
        "No conversion is defined between these units.".into(),
    ]))
}

#[cfg(test)]
#[path = "unit_conversions_tests.rs"]
mod tests;
