//! Migration 104 — Ensure `Administrator` and `Superadmin` roles grant all sync permissions.
//!
//! Sync permissions were introduced in migrations 060/061; `assign_all_permissions_to_role` in the
//! seeder only attaches permissions that exist when the role row is first populated. Existing
//! databases can miss `role_permissions` links for sync.* even though those users expect full access.

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260722_000104_grant_sync_permissions_admin_roles"
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
                 'sync.view',
                 'sync.manage',
                 'sync.resolve',
                 'sync.replay',
                 'sync.repair'
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
