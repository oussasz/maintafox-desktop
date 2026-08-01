use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260505_000060_sync_conflicts_and_replay_workflows"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_conflicts (
                id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                conflict_key           TEXT NOT NULL UNIQUE,
                source_scope           TEXT NOT NULL,
                source_batch_id        TEXT NULL,
                linked_outbox_id       INTEGER NULL REFERENCES sync_outbox(id) ON DELETE SET NULL,
                linked_inbox_id        INTEGER NULL REFERENCES sync_inbox(id) ON DELETE SET NULL,
                entity_type            TEXT NOT NULL,
                entity_sync_id         TEXT NOT NULL,
                operation              TEXT NOT NULL,
                conflict_type          TEXT NOT NULL,
                local_payload_json     TEXT NULL,
                inbound_payload_json   TEXT NULL,
                authority_side         TEXT NOT NULL,
                checkpoint_token       TEXT NULL,
                auto_resolution_policy TEXT NOT NULL,
                requires_operator_review INTEGER NOT NULL DEFAULT 1,
                recommended_action     TEXT NOT NULL,
                status                 TEXT NOT NULL DEFAULT 'new',
                resolution_action      TEXT NULL,
                resolution_note        TEXT NULL,
                resolved_by_id         INTEGER NULL,
                resolved_at            TEXT NULL,
                row_version            INTEGER NOT NULL DEFAULT 1,
                created_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_conflicts_status
             ON sync_conflicts(status, requires_operator_review, updated_at DESC)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_conflicts_entity
             ON sync_conflicts(entity_type, entity_sync_id, operation)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_conflicts_batch
             ON sync_conflicts(source_batch_id, conflict_type)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_replay_runs (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                replay_key            TEXT NOT NULL UNIQUE,
                mode                  TEXT NOT NULL,
                status                TEXT NOT NULL,
                reason                TEXT NOT NULL,
                requested_by_id       INTEGER NOT NULL,
                scope_json            TEXT NULL,
                pre_replay_checkpoint TEXT NULL,
                post_replay_checkpoint TEXT NULL,
                result_json           TEXT NULL,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                started_at            TEXT NULL,
                finished_at           TEXT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_replay_runs_status
             ON sync_replay_runs(status, created_at DESC)",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (name, description, category, is_dangerous, requires_step_up) in [
            ("sync.view", "View sync health, conflicts, and replay history", "sync", 0_i64, 0_i64),
            ("sync.manage", "Apply sync batches and stage sync envelopes", "sync", 1_i64, 0_i64),
            (
                "sync.resolve",
                "Resolve sync conflicts and change conflict lifecycle states",
                "sync",
                1_i64,
                0_i64,
            ),
            (
                "sync.replay",
                "Run sync replay and checkpoint rollback workflows",
                "sync",
                1_i64,
                1_i64,
            ),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO permissions
                 (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
                 VALUES (?, ?, ?, ?, ?, 1, ?)",
                [
                    name.into(),
                    description.into(),
                    category.into(),
                    is_dangerous.into(),
                    requires_step_up.into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }
        for (permission_name, required_permission_name) in [
            ("sync.manage", "sync.view"),
            ("sync.resolve", "sync.view"),
            ("sync.replay", "sync.resolve"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO permission_dependencies
                 (permission_name, required_permission_name, dependency_type, created_at)
                 VALUES (?, ?, 'hard', ?)",
                [
                    permission_name.into(),
                    required_permission_name.into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS sync_replay_runs").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS sync_conflicts").await?;
        Ok(())
    }
}
