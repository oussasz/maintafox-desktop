//! Data integrity findings + repair audit (gap 06 sprint 02).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260704_000086_data_integrity_findings"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS data_integrity_findings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                row_version INTEGER NOT NULL DEFAULT 1,
                severity TEXT NOT NULL,
                domain TEXT NOT NULL,
                record_class TEXT NOT NULL,
                record_id INTEGER NOT NULL,
                finding_code TEXT NOT NULL,
                details_json TEXT NOT NULL DEFAULT '{}',
                detected_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                cleared_at TEXT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                waiver_reason TEXT NULL,
                waiver_approver_id INTEGER NULL REFERENCES user_accounts(id)
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_di_findings_status_domain
             ON data_integrity_findings(status, domain, detected_at DESC)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_di_findings_record
             ON data_integrity_findings(record_class, record_id)"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS data_integrity_repair_actions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                row_version INTEGER NOT NULL DEFAULT 1,
                finding_id INTEGER NOT NULL REFERENCES data_integrity_findings(id),
                action TEXT NOT NULL,
                actor_id INTEGER NOT NULL REFERENCES user_accounts(id),
                performed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                before_json TEXT NOT NULL DEFAULT '{}',
                after_json TEXT NOT NULL DEFAULT '{}',
                sync_batch_id TEXT NULL
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_di_repair_finding ON data_integrity_repair_actions(finding_id)"
            .to_string(),
        ))
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permissions
             (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
             VALUES ('integrity.repair', 'Apply data integrity waivers and repairs', 'integrity', 1, 1, 1, ?)",
            [now.clone().into()],
        ))
        .await?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permission_dependencies
             (permission_name, required_permission_name, dependency_type, created_at)
             VALUES ('integrity.repair', 'sync.manage', 'hard', ?)",
            [now.clone().into()],
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at) \
             SELECT r.id, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now') \
             FROM roles r, permissions p \
             WHERE r.name = 'Administrator' AND r.deleted_at IS NULL AND p.name = 'integrity.repair'"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS data_integrity_repair_actions")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS data_integrity_findings")
            .await?;
        db.execute_unprepared(
            "DELETE FROM permission_dependencies WHERE permission_name = 'integrity.repair'",
        )
        .await
        .ok();
        db.execute_unprepared("DELETE FROM permissions WHERE name = 'integrity.repair'")
            .await
            .ok();
        Ok(())
    }
}
