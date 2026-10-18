//! Migration 231 — part lines are "required" or "optional" (owner-approved Step 7).
//!
//! Until now every short planned part blocked the plan. A part line now says whether it is
//! required (a short required part blocks the commitment, as today) or optional (a short
//! optional part warns). Existing lines become REQUIRED — exactly today's behaviour, stated here:
//! - `work_order_parts.is_required` (0 / 1) is added; every existing line is set to 1.
//! - Each entry of `pm_plan_versions.required_parts_json` that has no `"required"` flag gets
//!   `"required": true` (a plain code becomes `{"code", "required": true}`). The rewrite runs with
//!   sync capture suppressed: every device applies the same rewrite to the same rows, so nothing is
//!   sent and no device sees a conflict.
//!
//! New lines carry the flag explicitly: the API requires it (`wo::parts::AddPartInput`,
//! `pm::wo_content::validate_required_parts_json`).

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000231_part_lines_required"
    }
}

async fn has_column(db: &impl ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// `required_parts_json` with `"required": true` on every entry that has no flag (`None` when
/// nothing changes or the text is not a JSON list: such text is left as stored).
pub(crate) fn with_required_flags(raw: &str) -> Option<String> {
    let serde_json::Value::Array(items) = serde_json::from_str::<serde_json::Value>(raw.trim()).ok()? else {
        return None;
    };
    let mut changed = false;
    let out: Vec<serde_json::Value> = items
        .into_iter()
        .map(|item| match item {
            serde_json::Value::String(code) => {
                changed = true;
                serde_json::json!({ "code": code, "required": true })
            }
            serde_json::Value::Object(mut obj) => {
                if !obj.get("required").is_some_and(serde_json::Value::is_boolean) {
                    obj.insert("required".into(), serde_json::Value::Bool(true));
                    changed = true;
                }
                serde_json::Value::Object(obj)
            }
            other => other,
        })
        .collect();
    changed.then(|| serde_json::Value::Array(out).to_string())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        if !has_column(&txn, "work_order_parts", "is_required").await? {
            // Every existing line is required (today's behaviour); new lines always say so.
            txn.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE work_order_parts ADD COLUMN is_required INTEGER NOT NULL DEFAULT 1 \
                 CHECK (is_required IN (0, 1))"
                    .to_string(),
            ))
            .await?;
        }

        let capture = has_column(&txn, "sync_capture_suppression", "id").await?
            || txn
                .query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                        .to_string(),
                ))
                .await?
                .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        let rows = txn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, required_parts_json FROM pm_plan_versions \
                  WHERE required_parts_json IS NOT NULL AND TRIM(required_parts_json) <> ''"
                    .to_string(),
            ))
            .await?;
        for r in &rows {
            let id: i64 = r.try_get("", "id")?;
            let raw: String = r.try_get("", "required_parts_json")?;
            if let Some(rewritten) = with_required_flags(&raw) {
                txn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE pm_plan_versions SET required_parts_json = ? WHERE id = ?",
                    [rewritten.into(), id.into()],
                ))
                .await?;
            }
        }
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The flags may already be set by planners: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::with_required_flags;

    #[test]
    fn every_legacy_entry_becomes_required_and_flagged_entries_are_kept() {
        let out =
            with_required_flags(r#"["FLT-01", {"article_code":"BRG","quantity":2}, {"code":"X","required":false}]"#)
                .expect("changed");
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(v[0]["code"], "FLT-01");
        assert_eq!(v[0]["required"], true);
        assert_eq!(v[1]["required"], true);
        assert_eq!(v[1]["quantity"], 2);
        assert_eq!(v[2]["required"], false, "an explicit flag is kept");
        assert_eq!(with_required_flags(&out), None, "idempotent");
        assert_eq!(with_required_flags("not json"), None, "free text left as stored");
    }
}

#[cfg(test)]
mod chain_tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn text(db: &DatabaseConnection, sql: &str) -> String {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .unwrap_or_else(|| panic!("{sql}: no row"))
            .try_get_by_index::<String>(0)
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    /// On a real database at migration 230: the column is added with every existing line required,
    /// a legacy PM part list gets its flags, an explicit one is kept; running 231 again changes
    /// nothing.
    #[tokio::test]
    async fn the_real_migration_adds_the_flag_and_reruns_without_change() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261018_000231_part_lines_required")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 230");
        exec(&db, "PRAGMA foreign_keys = OFF").await;
        exec(
            &db,
            "INSERT INTO work_order_parts (work_order_id, article_ref, quantity_planned) VALUES (1, 'OLD-LINE', 2)",
        )
        .await;
        exec(
            &db,
            r#"INSERT INTO pm_plan_versions (pm_plan_id, version_no, effective_from, trigger_definition_json, required_parts_json)
               VALUES (1, 1, '2026-01-01T00:00:00Z', '{}', '["FLT-01", {"code":"X","required":false}]'),
                      (1, 2, '2026-01-01T00:00:00Z', '{}', 'free text kept')"#,
        )
        .await;
        exec(&db, "PRAGMA foreign_keys = ON").await;

        Migrator::up(&db, None).await.expect("231");
        assert_eq!(
            text(
                &db,
                "SELECT CAST(is_required AS TEXT) FROM work_order_parts WHERE article_ref = 'OLD-LINE'"
            )
            .await,
            "1",
            "an existing line is required"
        );
        let rewritten = text(
            &db,
            "SELECT required_parts_json FROM pm_plan_versions WHERE version_no = 1",
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&rewritten).expect("json");
        assert_eq!(v[0], serde_json::json!({ "code": "FLT-01", "required": true }));
        assert_eq!(v[1]["required"], false, "an explicit flag is kept");
        assert_eq!(
            text(
                &db,
                "SELECT required_parts_json FROM pm_plan_versions WHERE version_no = 2"
            )
            .await,
            "free text kept"
        );

        // 231 again (its down keeps the data): nothing changes.
        Migrator::down(&db, Some(1)).await.expect("down 231");
        Migrator::up(&db, None).await.expect("231 again");
        assert_eq!(
            text(
                &db,
                "SELECT required_parts_json FROM pm_plan_versions WHERE version_no = 1"
            )
            .await,
            rewritten
        );
    }
}
