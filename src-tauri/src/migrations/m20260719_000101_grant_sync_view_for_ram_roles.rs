//! Migration 101 — Grant `sync.view` to every role that already has `ram.view`.
//!
//! Sync health UI (`sync.view`) was added with sync infrastructure; operators and other RAM
//! viewers need read access without a separate admin grant.

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260719_000101_grant_sync_view_for_ram_roles"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
             SELECT DISTINCT r.id, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
             FROM roles r
             JOIN permissions p ON p.name = 'sync.view'
             JOIN role_permissions rp ON rp.role_id = r.id
             JOIN permissions pr ON pr.id = rp.permission_id AND pr.name = 'ram.view'
             WHERE r.deleted_at IS NULL"
                .to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentional no-op: revoking sync.view could surprise sites that granted it manually.
        Ok(())
    }
}
