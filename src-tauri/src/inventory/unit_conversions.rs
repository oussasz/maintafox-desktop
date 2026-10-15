//! Unit conversions (Phase 6.8): `1 from_unit = factor × to_unit` between units of the governed
//! list `MEASURE.UNITS` (`reference::units`), for every article or for one article. A conversion
//! for the article wins over the general one; the inverse direction uses `1 / factor`.
//!
//! Both units must measure the same physical dimension (a packaging unit converts to any unit):
//! a conversion between, say, kilograms and metres is refused with `GATE_UNIT_DIMENSION_MISMATCH`,
//! when it is created and when it would be used.
//!
//! Which conversion applies ([`convert_quantity`], see also `reference::units`):
//! 1. the article's own conversion (a row with `article_id`: "this cable comes in 50 m rolls");
//! 2. the general factor of a plant unit, set on the unit in Données de référence
//!    ("1 bag = 25 kg"), through its base standard unit;
//! 3. a general row between two standard units.
//!
//! A general row for a plant unit is refused (`GATE_UNIT_GENERAL_FACTOR_ON_UNIT`): its general
//! factor lives on the unit, once. A conversion that needs a plant unit without factor (migrated,
//! to review) is refused (`GATE_UNIT_FACTOR_MISSING`); a factor is never taken as 1.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

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
     JOIN reference_values fu ON fu.id = c.from_unit_value_id \
     JOIN reference_values tu ON tu.id = c.to_unit_value_id \
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

/// Both units of a new conversion: active units of `MEASURE.UNITS` that measure the same thing.
/// A general conversion (no article) is only stored between standard units: the general factor
/// of a plant unit is the one set on the unit.
async fn ensure_convertible_units(
    conn: &impl ConnectionTrait,
    from: i64,
    to: i64,
    article_id: Option<i64>,
) -> AppResult<()> {
    use crate::reference::units;
    let from = units::ensure_unit(conn, from, "from_unit_value_id", true).await?;
    let to = units::ensure_unit(conn, to, "to_unit_value_id", true).await?;
    units::assert_convertible(&from, &to, true)?;
    if article_id.is_none() {
        if let Some(plant) = [&from, &to].into_iter().find(|u| !u.is_standard) {
            return Err(AppError::ValidationFailed(vec![format!(
                "{}: le facteur général de l'unité '{}' se règle sur l'unité, dans les données de référence ; \
                 une conversion ici doit viser un article.",
                units::GATE_UNIT_GENERAL_FACTOR_ON_UNIT,
                plant.code
            )]));
        }
    }
    Ok(())
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
    ensure_convertible_units(&txn, input.from_unit_value_id, input.to_unit_value_id, input.article_id).await?;
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
/// general factors of plant units, then a general row between standard units; rows apply direct
/// or inverse. Fails when no conversion exists, when it needs a plant unit without factor, or
/// when two active conversions of the same scope disagree (two devices defined the pair offline).
pub async fn convert_quantity(
    conn: &impl ConnectionTrait,
    quantity: f64,
    from_unit_value_id: i64,
    to_unit_value_id: i64,
    article_id: Option<i64>,
) -> AppResult<ConvertedQuantity> {
    use crate::reference::units;
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
    let from = units::ensure_unit(conn, from_unit_value_id, "from_unit_value_id", false).await?;
    let to = units::ensure_unit(conn, to_unit_value_id, "to_unit_value_id", false).await?;
    // Units that measure different things never convert, whatever conversion row exists (one
    // created before the units had a dimension is flagged by migration 201).
    units::assert_convertible(&from, &to, false)?;
    let scaled = |(id, m, path): (Option<i64>, f64, &str)| ConvertedQuantity {
        quantity: quantity * m,
        factor: m,
        path: path.into(),
        conversion_id: id,
    };

    // 1. The article's own conversion.
    if let Some(article) = article_id {
        if let Some(found) = row_factor(conn, from.id, to.id, Some(article)).await? {
            return Ok(scaled(found));
        }
    }

    // 2. General factors of plant units, through their base standard units.
    if !(from.is_standard && to.is_standard) {
        let reduce = |u: &units::UnitOfMeasure| {
            if u.is_standard {
                Some((u.id, 1.0))
            } else {
                u.base_unit_id.zip(u.base_factor)
            }
        };
        match (reduce(&from), reduce(&to)) {
            (Some((base_from, f_from)), Some((base_to, f_to))) => {
                let between = if base_from == base_to {
                    Some(1.0)
                } else {
                    row_factor(conn, base_from, base_to, None).await?.map(|(_, m, _)| m)
                };
                if let Some(between) = between {
                    return Ok(scaled((None, f_from * between / f_to, "unit_factor")));
                }
            }
            _ => {
                // A general row stored for this pair before the factor lived on the unit is data,
                // not an assumption: it still applies. Otherwise the conversion is refused.
                if let Some(found) = row_factor(conn, from.id, to.id, None).await? {
                    return Ok(scaled(found));
                }
                let missing = if from.is_standard || from.base_factor.is_some() {
                    &to
                } else {
                    &from
                };
                return Err(AppError::ValidationFailed(vec![format!(
                    "{}: l'unité '{}' n'a pas encore d'unité de base ni de facteur (à revoir dans les données de référence).",
                    units::GATE_UNIT_FACTOR_MISSING,
                    missing.code
                )]));
            }
        }
    }

    // 3. A general row between the two units.
    if let Some(found) = row_factor(conn, from.id, to.id, None).await? {
        return Ok(scaled(found));
    }
    Err(AppError::ValidationFailed(vec![format!(
        "{}: aucune conversion n'est définie entre '{}' et '{}'.",
        units::GATE_UNIT_NO_CONVERSION,
        from.code,
        to.code
    )]))
}

/// The conversion rows of one scope between two units: `(row id, multiplier, direct|inverse)`.
/// Two active rows of the same scope that disagree (possible after two devices defined the pair
/// offline) are reported instead of picking one.
async fn row_factor(
    conn: &impl ConnectionTrait,
    from_unit_value_id: i64,
    to_unit_value_id: i64,
    scope: Option<i64>,
) -> AppResult<Option<(Option<i64>, f64, &'static str)>> {
    let multiplier = |from: i64, factor: f64| {
        if from == from_unit_value_id {
            factor
        } else {
            1.0 / factor
        }
    };
    let found = active_pair(conn, from_unit_value_id, to_unit_value_id, scope).await?;
    let Some(&(id, from, factor)) = found.first() else {
        return Ok(None);
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
    let path = if from == from_unit_value_id {
        "direct"
    } else {
        "inverse"
    };
    Ok(Some((Some(id), m, path)))
}

// ─── Quantities entered in another unit (receipts, issues) ──────────────────────────────────

/// Decimals kept on a stock quantity converted from another unit. The ledger and the balances
/// hold stock quantities; a converted quantity is rounded half away from zero to this precision
/// (one millionth of the stock unit), so the same entry gives the same stock quantity on every
/// device. Whole-unit articles (serial-tracked) refuse a converted quantity that is not whole.
pub const STOCK_QUANTITY_DECIMALS: i32 = 6;

/// Rounds a converted stock quantity ([`STOCK_QUANTITY_DECIMALS`]).
pub fn round_stock_quantity(quantity: f64) -> f64 {
    let scale = 10f64.powi(STOCK_QUANTITY_DECIMALS);
    (quantity * scale).round() / scale
}

/// A quantity entered by the storekeeper and the stock quantity it stands for.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StockQuantity {
    /// Quantity in the article's stock unit (what the ledger posts).
    pub stock_quantity: f64,
    pub stock_unit_value_id: i64,
    /// Unit and quantity as entered; `None` when entered in the stock unit.
    pub entered_unit_value_id: Option<i64>,
    pub entered_quantity: f64,
    /// Stock units per entered unit (1 in the stock unit).
    pub factor: f64,
    /// Rule that gave the factor: `identity`, `direct`, `inverse` (a conversion row) or
    /// `unit_factor` (the general factor of a plant unit).
    pub path: String,
}

impl StockQuantity {
    /// The entered quantity of a part of the stock quantity (one lot of a receipt or an issue).
    pub fn entered_part(&self, stock_part: f64) -> f64 {
        if self.factor == 0.0 {
            stock_part
        } else {
            round_stock_quantity(stock_part / self.factor)
        }
    }
}

/// Converts a quantity entered in `entered_unit_value_id` into the stock unit of the article, with
/// the conversion order of [`convert_quantity`] (the article's own conversion, the plant unit's
/// factor, a general row). `None` or the stock unit itself: no conversion. The entered unit must
/// be an active unit of `MEASURE.UNITS`.
pub async fn to_stock_quantity(
    conn: &impl ConnectionTrait,
    article_id: i64,
    entered_unit_value_id: Option<i64>,
    quantity: f64,
) -> AppResult<StockQuantity> {
    if !quantity.is_finite() {
        return Err(AppError::ValidationFailed(vec!["quantity must be a number.".into()]));
    }
    let stock_unit: i64 = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_value_id FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "article".into(),
            id: article_id.to_string(),
        })?
        .try_get("", "unit_value_id")
        .map_err(|e| decode_err("unit_value_id", e))?;
    let entered = entered_unit_value_id.filter(|u| *u != stock_unit);
    let Some(unit) = entered else {
        return Ok(StockQuantity {
            stock_quantity: quantity,
            stock_unit_value_id: stock_unit,
            entered_unit_value_id: None,
            entered_quantity: quantity,
            factor: 1.0,
            path: "identity".into(),
        });
    };
    crate::reference::units::ensure_unit(conn, unit, "entered_unit_value_id", true).await?;
    let one = convert_quantity(conn, 1.0, unit, stock_unit, Some(article_id)).await?;
    Ok(StockQuantity {
        stock_quantity: round_stock_quantity(quantity * one.factor),
        stock_unit_value_id: stock_unit,
        entered_unit_value_id: Some(unit),
        entered_quantity: quantity,
        factor: one.factor,
        path: one.path,
    })
}

/// Refuses a converted quantity that is not a whole number of stock units for an article that
/// moves by whole units (serial-tracked): the entered unit does not fit the article.
pub fn ensure_whole_stock_units(
    tracking: &crate::inventory::domain::ArticleTracking,
    converted: &StockQuantity,
) -> AppResult<()> {
    if converted.entered_unit_value_id.is_some()
        && tracking.serial
        && (converted.stock_quantity - converted.stock_quantity.round()).abs() > 1e-6
    {
        return Err(AppError::ValidationFailed(vec![format!(
            "{}: {} × {} = {} unités de stock ; un article suivi par numéro de série se mouvemente par unités entières.",
            crate::reference::units::GATE_UNIT_WHOLE_UNITS,
            converted.entered_quantity,
            converted.factor,
            converted.stock_quantity
        )]));
    }
    Ok(())
}

/// Writes on a stock movement the unit and quantity the storekeeper entered (nothing for a
/// quantity entered in the stock unit).
pub async fn stamp_entered_unit(
    conn: &impl ConnectionTrait,
    movement_id: i64,
    converted: &StockQuantity,
    stock_part: f64,
) -> AppResult<()> {
    let Some(unit) = converted.entered_unit_value_id else {
        return Ok(());
    };
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_transactions SET entered_unit_value_id = ?, entered_quantity = ?,          unit_conversion_factor = ? WHERE id = ?",
        [
            unit.into(),
            converted.entered_part(stock_part).into(),
            converted.factor.into(),
            movement_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "unit_conversions_tests.rs"]
mod tests;
