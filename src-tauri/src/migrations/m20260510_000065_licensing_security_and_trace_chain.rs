use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260510_000065_licensing_security_and_trace_chain"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS licensing_trust_keys (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                issuer            TEXT NOT NULL,
                key_id            TEXT NOT NULL,
                purpose           TEXT NOT NULL,
                is_active         INTEGER NOT NULL DEFAULT 1,
                is_compromised    INTEGER NOT NULL DEFAULT 0,
                last_rotated_at   TEXT NULL,
                compromised_at    TEXT NULL,
                compromise_reason TEXT NULL,
                created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(issuer, key_id)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_licensing_trust_keys_purpose
             ON licensing_trust_keys(purpose, is_active, is_compromised)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS license_api_exchange_guard (
                id                  TEXT PRIMARY KEY,
                channel             TEXT NOT NULL,
                purpose             TEXT NOT NULL,
                exchange_id         TEXT NOT NULL UNIQUE,
                request_nonce       TEXT NULL,
                response_nonce      TEXT NULL UNIQUE,
                signed_at           TEXT NOT NULL,
                received_at         TEXT NOT NULL,
                payload_hash        TEXT NOT NULL,
                signer_key_id       TEXT NULL,
                verification_result TEXT NOT NULL,
                correlation_id      TEXT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_license_api_exchange_received
             ON license_api_exchange_guard(received_at DESC, purpose)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS license_event_traces (
                id             TEXT PRIMARY KEY,
                correlation_id TEXT NOT NULL,
                event_type     TEXT NOT NULL,
                source         TEXT NOT NULL,
                subject_type   TEXT NOT NULL,
                subject_id     TEXT NULL,
                reason_code    TEXT NULL,
                outcome        TEXT NOT NULL,
                occurred_at    TEXT NOT NULL,
                payload_hash   TEXT NOT NULL,
                previous_hash  TEXT NULL,
                event_hash     TEXT NOT NULL UNIQUE
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_license_event_traces_corr
             ON license_event_traces(correlation_id, occurred_at DESC)",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (issuer, key_id, purpose) in [
            ("maintafox-vps", "key-v1", "entitlement_signature"),
            ("maintafox-vps", "key-rotated-v2", "entitlement_signature"),
            ("maintafox-vps", "activation-v1", "activation_signature"),
            ("maintafox-vps", "admin-policy-v1", "admin_policy_action"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO licensing_trust_keys
                 (issuer, key_id, purpose, is_active, is_compromised, created_at, updated_at)
                 VALUES (?, ?, ?, 1, 0, ?, ?)",
                [
                    issuer.into(),
                    key_id.into(),
                    purpose.into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS license_event_traces")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS license_api_exchange_guard")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS licensing_trust_keys")
            .await?;
        Ok(())
    }
}
