use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260509_000064_license_enforcement_matrix_and_actions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS license_enforcement_state (
                id                     INTEGER PRIMARY KEY,
                policy_sync_pending    INTEGER NOT NULL DEFAULT 0,
                last_transition_at     TEXT NULL,
                last_transition_reason TEXT NULL,
                updated_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO license_enforcement_state
             (id, policy_sync_pending, last_transition_at, last_transition_reason, updated_at)
             VALUES (1, 0, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS license_admin_actions (
                id                        TEXT PRIMARY KEY,
                action                    TEXT NOT NULL,
                reason                    TEXT NOT NULL,
                entitlement_state_before  TEXT NOT NULL,
                entitlement_state_after   TEXT NOT NULL,
                activation_state_before   TEXT NOT NULL,
                activation_state_after    TEXT NOT NULL,
                pending_local_writes      INTEGER NOT NULL DEFAULT 0,
                actor_user_id             INTEGER NULL,
                applied_at                TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_license_admin_actions_applied
             ON license_admin_actions(applied_at DESC)",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (name, description, category, dangerous, step_up) in [
            (
                "lic.view",
                "View license enforcement matrix status and reconciliation diagnostics",
                "license",
                0_i64,
                0_i64,
            ),
            (
                "lic.manage",
                "Apply admin license actions and reconcile local enforcement state",
                "license",
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
            ["lic.manage".into(), "lic.view".into(), now.into()],
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS license_admin_actions")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS license_enforcement_state")
            .await?;
        Ok(())
    }
}
