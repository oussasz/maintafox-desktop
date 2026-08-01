//! Analytics contract registry (gap 06 sprint 03).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260705_000087_analytics_contract_versions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS analytics_contract_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                row_version INTEGER NOT NULL DEFAULT 1,
                contract_id TEXT NOT NULL,
                version_semver TEXT NOT NULL,
                content_sha256 TEXT NOT NULL,
                activated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_analytics_contract_contract
             ON analytics_contract_versions(contract_id, activated_at DESC)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO analytics_contract_versions \
             (entity_sync_id, row_version, contract_id, version_semver, content_sha256) \
             VALUES \
             ('analytics_contract:closeout_to_reliability_v1:1.0.0', 1, \
              'closeout_to_reliability_v1', '1.0.0', \
              '002c71278185b9eaac69b56c026d8722246c3f7f01a036e47a684bc458a8372f')"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS analytics_contract_versions")
            .await?;
        Ok(())
    }
}
