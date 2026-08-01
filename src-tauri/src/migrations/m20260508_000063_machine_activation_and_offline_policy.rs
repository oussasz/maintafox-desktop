use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260508_000063_machine_activation_and_offline_policy"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS machine_activation_contracts (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                contract_id          TEXT NOT NULL UNIQUE,
                machine_id           TEXT NOT NULL,
                device_fingerprint   TEXT NOT NULL,
                slot_assignment_id   TEXT NOT NULL,
                slot_number          INTEGER NOT NULL,
                slot_limit           INTEGER NOT NULL,
                trust_score          INTEGER NOT NULL,
                vps_version          INTEGER NOT NULL,
                response_nonce       TEXT NOT NULL UNIQUE,
                issued_at            TEXT NOT NULL,
                expires_at           TEXT NOT NULL,
                offline_grace_until  TEXT NOT NULL,
                revocation_state     TEXT NOT NULL,
                revocation_reason    TEXT NULL,
                anchor_hashes_json   TEXT NOT NULL,
                policy_snapshot_json TEXT NOT NULL,
                created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                row_version          INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_activation_slot_consistency
             ON machine_activation_contracts(slot_assignment_id, revocation_state, machine_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_activation_vps_version
             ON machine_activation_contracts(vps_version DESC, created_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS machine_activation_state (
                id                          INTEGER PRIMARY KEY,
                active_contract_id          INTEGER NULL REFERENCES machine_activation_contracts(id),
                last_reconnect_at           TEXT NULL,
                last_revocation_applied_at  TEXT NULL,
                last_offline_check_at       TEXT NULL,
                last_offline_denial_code    TEXT NULL,
                last_offline_denial_message TEXT NULL,
                updated_at                  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO machine_activation_state (
                id, active_contract_id, last_reconnect_at, last_revocation_applied_at, last_offline_check_at,
                last_offline_denial_code, last_offline_denial_message, updated_at
             ) VALUES (
                1, NULL, NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now')
             )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS machine_activation_lineage (
                id                 TEXT PRIMARY KEY,
                event_code         TEXT NOT NULL,
                contract_id        TEXT NULL,
                slot_assignment_id TEXT NULL,
                detail_json        TEXT NOT NULL,
                occurred_at        TEXT NOT NULL,
                actor_user_id      INTEGER NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_machine_activation_lineage_occurred
             ON machine_activation_lineage(occurred_at DESC)",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (name, description, category, dangerous, step_up) in [
            (
                "act.view",
                "View machine activation state and offline policy diagnostics",
                "activation",
                0_i64,
                0_i64,
            ),
            (
                "act.manage",
                "Manage machine activation contracts and secret rotation",
                "activation",
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
                    dangerous.into(),
                    step_up.into(),
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
            ["act.manage".into(), "act.view".into(), now.into()],
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS machine_activation_lineage")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS machine_activation_state")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS machine_activation_contracts")
            .await?;
        Ok(())
    }
}
