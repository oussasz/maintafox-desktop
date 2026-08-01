use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260507_000062_entitlement_envelopes_and_cache"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS entitlement_envelopes (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                envelope_id           TEXT NOT NULL UNIQUE,
                previous_envelope_id  TEXT NULL,
                lineage_version       INTEGER NOT NULL,
                issuer                TEXT NOT NULL,
                key_id                TEXT NOT NULL,
                signature_alg         TEXT NOT NULL,
                tier                  TEXT NOT NULL,
                state                 TEXT NOT NULL,
                channel               TEXT NOT NULL,
                machine_slots         INTEGER NOT NULL,
                feature_flags_json    TEXT NOT NULL,
                capabilities_json     TEXT NOT NULL,
                policy_json           TEXT NOT NULL,
                issued_at             TEXT NOT NULL,
                valid_from            TEXT NOT NULL,
                valid_until           TEXT NOT NULL,
                offline_grace_until   TEXT NOT NULL,
                payload_hash          TEXT NOT NULL,
                signature             TEXT NOT NULL,
                verified_at           TEXT NULL,
                verification_result   TEXT NOT NULL,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_entitlement_lineage
             ON entitlement_envelopes(lineage_version DESC, created_at DESC)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_entitlement_verification
             ON entitlement_envelopes(verification_result, verified_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS entitlement_cache_state (
                id                 INTEGER PRIMARY KEY,
                active_envelope_id INTEGER NULL REFERENCES entitlement_envelopes(id),
                last_refresh_at    TEXT NULL,
                last_refresh_error TEXT NULL,
                updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO entitlement_cache_state (id, active_envelope_id, last_refresh_at, last_refresh_error, updated_at)
             VALUES (1, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (name, description, category, is_dangerous, requires_step_up) in [
            (
                "ent.view",
                "View entitlement summary and diagnostics",
                "entitlement",
                0_i64,
                0_i64,
            ),
            (
                "ent.manage",
                "Apply and rotate signed entitlement envelopes",
                "entitlement",
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
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permission_dependencies
             (permission_name, required_permission_name, dependency_type, created_at)
             VALUES (?, ?, 'hard', ?)",
            ["ent.manage".into(), "ent.view".into(), now.into()],
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS entitlement_cache_state")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS entitlement_envelopes")
            .await?;
        Ok(())
    }
}
