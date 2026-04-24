//! Partial unique index on `user_scope_assignments`.
//!
//! The original `uidx_usa_user_role_scope` applied to **all** rows, including soft-deleted
//! ones. After `UPDATE ... SET deleted_at = ?` (revoke / replace-at-scope), inserting the
//! same `(user_id, role_id, scope_type, scope_reference)` still violated the unique index.
//!
//! This migration replaces that index with a partial unique index that only covers rows
//! where `deleted_at IS NULL`, matching runtime uniqueness semantics.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260725_000107_user_scope_assignments_unique_active_only"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS uidx_usa_user_role_scope")
            .await?;

        // Keep one active row per (user, role, scope, ref); same as migration 028 but scoped to live rows.
        db.execute_unprepared(
            "DELETE FROM user_scope_assignments \
             WHERE deleted_at IS NULL \
               AND id NOT IN ( \
                 SELECT MIN(id) FROM user_scope_assignments \
                 WHERE deleted_at IS NULL \
                 GROUP BY user_id, role_id, scope_type, COALESCE(scope_reference, '') \
               )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_usa_user_role_scope \
             ON user_scope_assignments(user_id, role_id, scope_type, COALESCE(scope_reference, '')) \
             WHERE deleted_at IS NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS uidx_usa_user_role_scope")
            .await?;

        // Deduplicate across all rows (including soft-deleted) before restoring the legacy index.
        db.execute_unprepared(
            "DELETE FROM user_scope_assignments \
             WHERE id NOT IN ( \
               SELECT MIN(id) FROM user_scope_assignments \
               GROUP BY user_id, role_id, scope_type, COALESCE(scope_reference, '') \
             )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_usa_user_role_scope \
             ON user_scope_assignments(user_id, role_id, scope_type, COALESCE(scope_reference, ''))",
        )
        .await?;

        Ok(())
    }
}
