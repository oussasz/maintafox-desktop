//! Migration 201 — units of measure under reference-data governance.
//!
//! Before: a fixed lookup `inventory.unit_of_measure` (8 values created at start-up in
//! `lookup_values`, no way for a plant to add a unit), linked by `lookup_values.id` from the stock
//! unit of an article, the price unit of a supplier price and both units of a conversion; free
//! texts for the unit of an equipment meter (`equipment_meters.unit`) and of an inspection
//! measurement (`inspection_checkpoints.measurement_unit`, copied into the checkpoint package of a
//! published checklist version).
//!
//! After: the COMPANY list `MEASURE.UNITS` (category C, draft → validate → publish) and the SYSTEM
//! list of physical dimensions `MEASURE.DIMENSIONS` (`reference::units`). Every link holds a
//! `reference_values.id`:
//!
//! | link                                           | change                                      |
//! |------------------------------------------------|---------------------------------------------|
//! | `articles.unit_value_id`                       | remapped by code, FK → `reference_values`   |
//! | `inventory_supplier_prices.price_unit_value_id`| remapped by code, FK added                  |
//! | `unit_conversions.from/to_unit_value_id`       | remapped by code, FK → `reference_values`   |
//! | `equipment_meters.unit` (text)                 | → new `unit_value_id`, text column dropped  |
//! | `inspection_checkpoints.measurement_unit` (text)| → new `measurement_unit_id`, text dropped  |
//! | checkpoint package JSON `measurement_unit`     | → `measurement_unit_id`                     |
//!
//! - A lookup code the list does not know becomes a unit of the plant, flagged
//!   `review_required` (never dropped). Two lookup codes that only differ by case keep two units.
//! - A free text is matched on the code, symbol, French or English label of a unit (case, spaces,
//!   ² / ³ and a plural `s` ignored); an unmatched text becomes a unit of the plant flagged
//!   `review_required`. **Why the text columns are dropped:** removing them loses nothing — every
//!   text becomes the link to a unit whose label or symbol is that text, and a text that matched a
//!   unit under another spelling is kept as a `legacy` alias of that unit (`reference_aliases`), so
//!   the original spelling stays searchable. Keeping the column would leave a second, retyped copy
//!   of the unit that the reference data could not govern.
//! - An active conversion between units that measure different things (mass ↔ length…) is not
//!   changed but gets a note asking to review it; the conversion engine refuses to use it.
//! - The lookup values, their aliases and the domain are deleted.
//!
//! **Safety.** One transaction (sea-orm-migration does not open one for SQLite); each part is
//! guarded (lookup gone, text column gone, foreign key already retargeted), so a repeated run
//! changes nothing. The foreign keys are retargeted in the stored table definitions (procedure of
//! migrations 164 / 198), then the schema is checked. Capture is suspended during migrations: the
//! startup retirement pass (`reference::retirement`) retires the lookup again, with capture on, if
//! a replayed history brings it back. New units carry seeded identities (`MEASURE.UNITS:code`),
//! so every device creates the same units with the same identity.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::reference::units::{self, LegacyLinkMode, UNIT_LINK_COLUMNS};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000201_governed_units_of_measure"
    }
}

fn db_err(e: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(e.to_string())
}

async fn table_exists<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// Table a column's declared foreign key points at, if any.
async fn fk_target<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<Option<String>, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT \"table\" AS target FROM pragma_foreign_key_list(?) WHERE \"from\" = ?",
        [table.into(), column.into()],
    ))
    .await?
    .map(|r| r.try_get::<String>("", "target"))
    .transpose()
}

// ── Retargeting a column's foreign key in the stored table definition ────────────────────────

/// Byte span of the definition of `column` in a `CREATE TABLE` statement: from the column name to
/// the next comma or closing parenthesis at the top level of the column list.
fn column_def_span(sql: &str, column: &str) -> Option<(usize, usize)> {
    let open = sql.find('(')?;
    let mut depth = 0i32;
    let mut item_start = open + 1;
    let mut quote: Option<char> = None;
    for (i, c) in sql.char_indices().skip_while(|(i, _)| *i < open) {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' if depth >= 1 => quote = Some(c),
            '(' => depth += 1,
            ')' | ',' if depth == 1 => {
                let item = &sql[item_start..i];
                let lead = item.len() - item.trim_start().len();
                let body = &item[lead..];
                let name_end = body.find(|ch: char| ch.is_whitespace()).unwrap_or(body.len());
                let name = body[..name_end].trim_matches(|ch| matches!(ch, '"' | '`' | '[' | ']'));
                if name.eq_ignore_ascii_case(column) {
                    return Some((item_start + lead, i));
                }
                if c == ')' {
                    return None;
                }
                item_start = i + 1;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The statement with the foreign key of `column` pointing at `target(id)`: an existing
/// `REFERENCES x(…)` clause is replaced, otherwise one is appended to the column definition.
pub(crate) fn retarget_column_reference(sql: &str, column: &str, target: &str) -> Option<String> {
    let (start, end) = column_def_span(sql, column)?;
    let def = &sql[start..end];
    let clause = format!("REFERENCES {target}(id)");
    let new_def = match def.to_ascii_uppercase().find("REFERENCES") {
        Some(at) => {
            let close = def[at..].find(')')? + at + 1;
            format!("{}{clause}{}", &def[..at], &def[close..])
        }
        None => format!("{} {clause}", def.trim_end()),
    };
    let trailing = if def.ends_with(char::is_whitespace) && !new_def.ends_with(char::is_whitespace) {
        &def[def.trim_end().len()..]
    } else {
        ""
    };
    Some(format!("{}{new_def}{trailing}{}", &sql[..start], &sql[end..]))
}

/// Points the foreign keys of the unit link columns at `reference_values(id)` in the stored table
/// definitions. Only columns not already pointing there; `writable_schema` is switched off
/// whatever happens, then the schema is checked.
async fn retarget_unit_link_fks<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let mut edits: Vec<(String, String)> = Vec::new();
    let mut tables: Vec<&str> = UNIT_LINK_COLUMNS.iter().map(|(t, _)| *t).collect();
    tables.dedup();
    for table in tables {
        if !table_exists(db, table).await? {
            continue;
        }
        let sql: String = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
                [table.into()],
            ))
            .await?
            .ok_or_else(|| DbErr::Custom(format!("{table}: no table definition")))?
            .try_get("", "sql")?;
        let mut new_sql = sql.clone();
        for (_, column) in UNIT_LINK_COLUMNS.iter().filter(|(t, _)| *t == table) {
            if !column_exists(db, table, column).await?
                || fk_target(db, table, column).await?.as_deref() == Some("reference_values")
            {
                continue;
            }
            new_sql = retarget_column_reference(&new_sql, column, "reference_values")
                .ok_or_else(|| DbErr::Custom(format!("{table}.{column}: column definition not found")))?;
        }
        if new_sql != sql {
            edits.push((table.to_string(), new_sql));
        }
    }
    if edits.is_empty() {
        return Ok(());
    }

    let version: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA schema_version".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("schema_version unavailable".into()))?
        .try_get_by_index(0)?;
    db.execute_unprepared("PRAGMA writable_schema = ON").await?;
    let edited = async {
        for (table, sql) in &edits {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sqlite_master SET sql = ? WHERE type = 'table' AND name = ?",
                [sql.clone().into(), table.clone().into()],
            ))
            .await?;
        }
        db.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
            .await
    }
    .await;
    let off = db.execute_unprepared("PRAGMA writable_schema = OFF").await;
    edited?;
    off?;

    let check: String = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA quick_check".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("quick_check returned nothing".into()))?
        .try_get_by_index(0)?;
    if check != "ok" {
        return Err(DbErr::Custom(format!(
            "schema check after retargeting the unit foreign keys: {check}"
        )));
    }
    for (table, column) in UNIT_LINK_COLUMNS {
        if table_exists(db, table).await?
            && column_exists(db, table, column).await?
            && fk_target(db, table, column).await?.as_deref() != Some("reference_values")
        {
            return Err(DbErr::Custom(format!(
                "{table}.{column}: the foreign key could not be pointed at reference_values"
            )));
        }
    }
    Ok(())
}

// ── Free texts → links ─────────────────────────────────────────────────────────────────────

/// Drops the triggers of `table` (sync capture triggers, the only triggers on the tables whose
/// text column is dropped here; startup installs them again). Startup suspends capture before
/// migrations, this only covers a database where they were left in place.
async fn drop_table_triggers<C: ConnectionTrait>(db: &C, table: &str) -> Result<(), DbErr> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ? AND name LIKE 'sync_cap_%'",
            [table.into()],
        ))
        .await?;
    for r in rows {
        let name: String = r.try_get("", "name")?;
        db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS \"{}\"", name.replace('"', "\"\"")))
            .await?;
    }
    Ok(())
}

/// Replaces the text column `text_col` of `table` by the link `link_col` to a unit.
async fn link_free_text_units<C: ConnectionTrait>(
    db: &C,
    table: &str,
    text_col: &str,
    link_col: &str,
) -> Result<(), DbErr> {
    if !table_exists(db, table).await? {
        return Ok(());
    }
    if !column_exists(db, table, link_col).await? {
        db.execute_unprepared(&format!(
            "ALTER TABLE {table} ADD COLUMN {link_col} INTEGER NULL REFERENCES reference_values(id)"
        ))
        .await?;
    }
    db.execute_unprepared(&format!(
        "CREATE INDEX IF NOT EXISTS idx_{table}_{link_col} ON {table}({link_col})"
    ))
    .await?;
    if !column_exists(db, table, text_col).await? {
        return Ok(());
    }
    let texts: Vec<String> = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT DISTINCT {text_col} AS t FROM {table} \
                  WHERE {text_col} IS NOT NULL AND TRIM({text_col}) <> '' ORDER BY {text_col}"
            ),
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<String>("", "t"))
        .collect::<Result<_, _>>()?;
    let map = units::map_unit_texts(db, &texts, &format!("{table}.{text_col}"))
        .await
        .map_err(db_err)?;
    for text in &texts {
        if let Some(id) = map.get(text) {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("UPDATE {table} SET {link_col} = ? WHERE {text_col} = ? AND {link_col} IS NULL"),
                [(*id).into(), text.clone().into()],
            ))
            .await?;
        }
    }
    drop_table_triggers(db, table).await?;
    db.execute_unprepared(&format!("ALTER TABLE {table} DROP COLUMN {text_col}"))
        .await?;
    Ok(())
}

/// Checkpoint packages of published checklist versions: `measurement_unit` (text) becomes
/// `measurement_unit_id`, mapped like the checkpoints. Packages without a text are not touched.
async fn link_package_units<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if !table_exists(db, "inspection_template_versions").await? {
        return Ok(());
    }
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, checkpoint_package_json FROM inspection_template_versions \
              WHERE checkpoint_package_json LIKE '%\"measurement_unit\"%' ORDER BY id"
                .to_string(),
        ))
        .await?;
    let mut packages: Vec<(i64, serde_json::Value)> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for r in rows {
        let id: i64 = r.try_get("", "id")?;
        let raw: String = r.try_get("", "checkpoint_package_json")?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            tracing::warn!(
                version_id = id,
                "migration 201: checkpoint package is not JSON, left as is"
            );
            continue;
        };
        if let Some(entries) = value.as_array() {
            for e in entries {
                if let Some(t) = e.get("measurement_unit").and_then(serde_json::Value::as_str) {
                    if !t.trim().is_empty() && !texts.iter().any(|x| x == t) {
                        texts.push(t.to_string());
                    }
                }
            }
        }
        packages.push((id, value));
    }
    texts.sort();
    let map = units::map_unit_texts(db, &texts, "inspection_template_versions.checkpoint_package_json")
        .await
        .map_err(db_err)?;
    for (id, mut value) in packages {
        let Some(entries) = value.as_array_mut() else {
            continue;
        };
        for e in entries.iter_mut() {
            let Some(obj) = e.as_object_mut() else {
                continue;
            };
            let Some(old) = obj.remove("measurement_unit") else {
                continue;
            };
            let unit_id = old.as_str().and_then(|t| map.get(t)).copied();
            obj.insert("measurement_unit_id".into(), unit_id.into());
        }
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE inspection_template_versions SET checkpoint_package_json = ? WHERE id = ?",
            [value.to_string().into(), id.into()],
        ))
        .await?;
    }
    Ok(())
}

// ── Conversions between different dimensions ─────────────────────────────────────────────────

const REVIEW_NOTE: &str = "[Migration 201] Les deux unités ne mesurent pas la même grandeur : conversion \
     refusée, à revoir. / The two units do not measure the same quantity: conversion refused, to review.";

async fn flag_cross_dimension_conversions<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if !table_exists(db, "unit_conversions").await? {
        return Ok(());
    }
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, from_unit_value_id, to_unit_value_id FROM unit_conversions \
              WHERE is_active = 1 ORDER BY id"
                .to_string(),
        ))
        .await?;
    for r in rows {
        let id: i64 = r.try_get("", "id")?;
        let from = units::unit_by_id(db, r.try_get("", "from_unit_value_id")?)
            .await
            .map_err(db_err)?;
        let to = units::unit_by_id(db, r.try_get("", "to_unit_value_id")?)
            .await
            .map_err(db_err)?;
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        if units::assert_convertible(&from, &to, false).is_err() {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE unit_conversions \
                    SET notes = CASE WHEN notes IS NULL OR TRIM(notes) = '' THEN ? ELSE notes || char(10) || ? END \
                  WHERE id = ? AND (notes IS NULL OR notes NOT LIKE '%[Migration 201]%')",
                [REVIEW_NOTE.into(), REVIEW_NOTE.into(), id.into()],
            ))
            .await?;
            tracing::warn!(
                conversion_id = id,
                "migration 201: conversion between units of different dimensions, to review"
            );
        }
    }
    Ok(())
}

pub(crate) async fn run<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    units::ensure_unit_catalog(db).await.map_err(db_err)?;
    retarget_unit_link_fks(db).await?;
    units::retire_legacy_unit_lookup(db, LegacyLinkMode::Migration)
        .await
        .map_err(db_err)?;
    link_free_text_units(db, "equipment_meters", "unit", "unit_value_id").await?;
    link_free_text_units(db, "inspection_checkpoints", "measurement_unit", "measurement_unit_id").await?;
    link_package_units(db).await?;
    flag_cross_dimension_conversions(db).await?;
    report_units_to_review(db).await
}

/// Migration report: the units created for codes or texts Maintafox did not know, which the
/// reference data administrator completes (base standard unit and factor) in Données de référence.
async fn report_units_to_review<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let to_review = units::list_units_to_review(db).await.map_err(db_err)?;
    if to_review.is_empty() {
        tracing::info!("migration 201: every unit was mapped onto a standard unit, nothing to review");
        return Ok(());
    }
    let listed: Vec<String> = to_review
        .iter()
        .map(|u| format!("{} « {} »", u.code, u.label))
        .collect();
    tracing::warn!(
        count = to_review.len(),
        units = %listed.join(", "),
        "migration 201: units created to review in reference data (MEASURE.UNITS, filter « à revoir »)"
    );
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration runs SQLite migrations outside a transaction: one here, so a failure
        // leaves the database untouched and the next start runs it again.
        let txn = manager.get_connection().begin().await?;
        run(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Relinked data and dropped text columns: not reversible.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retargets_only_the_named_column() {
        let sql = "CREATE TABLE articles (\n id INTEGER PRIMARY KEY,\n unit_value_id        INTEGER NOT NULL REFERENCES lookup_values(id),\n criticality_value_id INTEGER NULL REFERENCES lookup_values(id),\n price_unit_value_id INTEGER NULL\n)";
        let out = retarget_column_reference(sql, "unit_value_id", "reference_values").expect("found");
        assert!(out.contains("unit_value_id        INTEGER NOT NULL REFERENCES reference_values(id),"));
        assert!(out.contains("criticality_value_id INTEGER NULL REFERENCES lookup_values(id)"));
        let out = retarget_column_reference(&out, "price_unit_value_id", "reference_values").expect("found");
        assert!(out.contains("price_unit_value_id INTEGER NULL REFERENCES reference_values(id)\n)"));
        assert!(retarget_column_reference(sql, "missing", "reference_values").is_none());
    }

    #[test]
    fn skips_table_constraints_and_quoted_text() {
        let sql = "CREATE TABLE unit_conversions (id INTEGER, from_unit_value_id INTEGER NOT NULL REFERENCES                    lookup_values(id), notes TEXT DEFAULT 'a, b', factor REAL CHECK (factor > 0),                    CHECK (from_unit_value_id <> 1))";
        let out = retarget_column_reference(sql, "from_unit_value_id", "reference_values").expect("found");
        assert!(out.contains("from_unit_value_id INTEGER NOT NULL REFERENCES reference_values(id),"));
        assert!(out.contains("CHECK (from_unit_value_id <> 1)"));
    }
}
