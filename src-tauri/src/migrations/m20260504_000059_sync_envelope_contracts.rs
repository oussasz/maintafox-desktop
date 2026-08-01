use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260504_000059_sync_envelope_contracts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_outbox (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                idempotency_key   TEXT NOT NULL,
                entity_type       TEXT NOT NULL,
                entity_sync_id    TEXT NOT NULL,
                operation         TEXT NOT NULL,
                row_version       INTEGER NOT NULL,
                payload_json      TEXT NOT NULL,
                payload_hash      TEXT NOT NULL,
                status            TEXT NOT NULL DEFAULT 'pending',
                acknowledged_at   TEXT NULL,
                rejection_code    TEXT NULL,
                rejection_message TEXT NULL,
                origin_machine_id TEXT NULL,
                created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(idempotency_key, entity_type, entity_sync_id, operation, payload_hash)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_outbox_status_created
             ON sync_outbox(status, created_at, id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_outbox_idempotency
             ON sync_outbox(idempotency_key, entity_sync_id, operation)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_inbox (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                server_batch_id   TEXT NOT NULL,
                checkpoint_token  TEXT NOT NULL,
                entity_type       TEXT NOT NULL,
                entity_sync_id    TEXT NOT NULL,
                operation         TEXT NOT NULL,
                row_version       INTEGER NOT NULL,
                payload_json      TEXT NOT NULL,
                payload_hash      TEXT NOT NULL,
                apply_status      TEXT NOT NULL DEFAULT 'pending',
                rejection_code    TEXT NULL,
                rejection_message TEXT NULL,
                created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(server_batch_id, entity_type, entity_sync_id, operation, payload_hash)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_inbox_checkpoint
             ON sync_inbox(checkpoint_token, id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_inbox_status_created
             ON sync_inbox(apply_status, created_at, id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_checkpoint (
                id                   INTEGER PRIMARY KEY CHECK(id = 1),
                checkpoint_token     TEXT NULL,
                last_idempotency_key TEXT NULL,
                protocol_version     TEXT NOT NULL DEFAULT 'v1',
                policy_metadata_json TEXT NULL,
                last_sync_at         TEXT NULL,
                updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_rejections (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                source            TEXT NOT NULL,
                linked_record_id  INTEGER NULL,
                idempotency_key   TEXT NULL,
                entity_type       TEXT NULL,
                entity_sync_id    TEXT NOT NULL,
                operation         TEXT NOT NULL,
                rejection_code    TEXT NOT NULL,
                rejection_message TEXT NOT NULL,
                rejected_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_rejections_scope
             ON sync_rejections(source, rejection_code, rejected_at)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS sync_rejections")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS sync_checkpoint")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS sync_inbox").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS sync_outbox")
            .await?;
        Ok(())
    }
}
