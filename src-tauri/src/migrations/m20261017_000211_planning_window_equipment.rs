//! Migration 211 — planning windows per equipment, and their types as a SYSTEM list.
//!
//! - `planning_windows.equipment_id` (optional): a window can concern one equipment of its site
//!   (a machine released to maintenance, a line stop) instead of the whole site.
//! - `PLANNING.WINDOW_TYPES`: the window types (production, maintenance, shutdown, freeze) become a
//!   SYSTEM reference list (`planning::windows`), so the window type of every window is a governed
//!   code with translated labels.
//! - `planning_windows.scope_review_required`: until this migration the only screen that created a
//!   window with a site (the production-window dialog) stored the selected *team id* in
//!   `entity_id` (an org-node FK); the schedule freeze always stored none. A team id is a small
//!   integer that usually also exists as an org-node id, so the foreign key does not catch it: such
//!   a row points at an unrelated node. Every window that already has a site is therefore made
//!   plant-wide (never a looser lock: a plant-wide window applies to every job) and flagged so a
//!   planner re-enters its site. Done once, when the flag column is added.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000211_planning_window_equipment"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        if !has_column(&txn, "planning_windows", "equipment_id").await? {
            txn.execute_unprepared(
                "ALTER TABLE planning_windows ADD COLUMN equipment_id INTEGER NULL REFERENCES equipment(id)",
            )
            .await?;
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_planning_windows_equipment ON planning_windows(equipment_id)",
        )
        .await?;
        if !has_column(&txn, "planning_windows", "scope_review_required").await? {
            txn.execute_unprepared(
                "ALTER TABLE planning_windows ADD COLUMN scope_review_required INTEGER NOT NULL DEFAULT 0 \
                 CHECK (scope_review_required IN (0, 1))",
            )
            .await?;
            // Sites written by the old dialog are team ids: made plant-wide and flagged.
            txn.execute_unprepared(
                "UPDATE planning_windows SET entity_id = NULL, scope_review_required = 1, \
                        row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                  WHERE entity_id IS NOT NULL",
            )
            .await?;
        }
        crate::planning::windows::ensure_window_type_catalog(&txn)
            .await
            .map_err(|e| DbErr::Custom(format!("planning window types: {e}")))?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_planning_windows_equipment")
            .await?;
        // The sites cleared on the way up are not restored (they were team ids, not sites).
        for column in ["scope_review_required", "equipment_id"] {
            if has_column(db, "planning_windows", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE planning_windows DROP COLUMN {column}"))
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use sea_orm_migration::prelude::*;

    use super::Migration;

    async fn window(db: &sea_orm::DatabaseConnection, id: i64) -> (Option<i64>, i64) {
        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("SELECT entity_id, scope_review_required FROM planning_windows WHERE id = {id}"),
            ))
            .await
            .expect("query")
            .expect("window");
        (
            row.try_get("", "entity_id").expect("entity"),
            row.try_get("", "scope_review_required").expect("flag"),
        )
    }

    /// A window saved by the old dialog (its "site" was the team id) becomes plant-wide and is
    /// flagged for review, once; windows created afterwards keep their site.
    #[tokio::test]
    async fn legacy_window_sites_are_cleared_and_flagged_once() {
        let db = crate::db::test_db::migrated().await;
        // The table as it was before migration 211, with a legacy row whose "site" is a team id
        // that happens to be an org-node id (node 7101 here).
        for sql in [
            "INSERT INTO org_structure_models (entity_sync_id, version_number, status, description, created_at, updated_at) \
             VALUES ('m211-structure', 1, 'active', 'm211 test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, \
                can_carry_cost_center, can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at) \
             SELECT 'm211-type', id, 'SITE', 'Site', 0, 1, 0, 1, 0, 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z' \
               FROM org_structure_models WHERE entity_sync_id = 'm211-structure'",
            "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, \
                created_at, updated_at, row_version, structure_model_id) \
             SELECT 7101, 'm211-node', 'SITE-M211', 'Site', t.id, NULL, '/', 0, 'active', '2026-01-01T00:00:00Z', \
                    '2026-01-01T00:00:00Z', 1, t.structure_model_id FROM org_node_types t WHERE t.entity_sync_id = 'm211-type'",
            "DROP INDEX IF EXISTS idx_planning_windows_equipment",
            "ALTER TABLE planning_windows DROP COLUMN scope_review_required",
            "ALTER TABLE planning_windows DROP COLUMN equipment_id",
            "INSERT INTO planning_windows (id, entity_id, window_type, start_datetime, end_datetime, is_locked) \
             VALUES (9001, 7101, 'production', '2026-05-04T06:00:00Z', '2026-05-04T18:00:00Z', 1), \
                    (9002, NULL, 'freeze', '2026-05-05T00:00:00Z', '2026-05-06T00:00:00Z', 1)",
        ] {
            db.execute_unprepared(sql).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.expect("migration 211");
        assert_eq!(window(&db, 9001).await, (None, 1), "team id cleared, flagged");
        assert_eq!(window(&db, 9002).await, (None, 0), "a plant-wide freeze is untouched");

        // Re-running does not touch windows given a site since.
        db.execute_unprepared(
            "UPDATE planning_windows SET entity_id = 7101, scope_review_required = 0 \
             WHERE id = 9001",
        )
        .await
        .expect("site re-entered");
        Migration.up(&manager).await.expect("migration 211 again");
        assert_eq!(window(&db, 9001).await.1, 0);
        assert!(window(&db, 9001).await.0.is_some(), "the re-entered site stays");
    }
}
