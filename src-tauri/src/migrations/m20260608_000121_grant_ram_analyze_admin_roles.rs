//! Migration 121 — Ensure `Administrator` and `Superadmin` roles grant RAM analyze permissions.
//!
//! `ram.analyze`, `ram.manage`, and `ram.export` were introduced in migration 029; existing
//! databases can miss `role_permissions` links for admin roles even though those users expect
//! full access (same drift pattern as sync.* in migration 104).

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260608_000121_grant_ram_analyze_admin_roles"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
             SELECT r.id, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
             FROM roles r
             CROSS JOIN permissions p
             WHERE r.deleted_at IS NULL
               AND r.name IN ('Administrator', 'Superadmin')
               AND p.name IN (
                 'ram.analyze',
                 'ram.manage',
                 'ram.export'
               )"
            .to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
