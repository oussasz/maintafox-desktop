use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260412_000031_rbac_settings_and_lockout"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── rbac_settings: key-value configuration for RBAC and auth policies ──
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS rbac_settings (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL,
                description TEXT NULL
            )",
        )
        .await?;

        // Seed lockout configuration defaults
        db.execute_unprepared(
            "INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
                ('lockout_max_attempts', '5', 'Failed login attempts before account lockout'),
                ('lockout_base_minutes', '15', 'Base lockout duration in minutes'),
                ('lockout_progressive', '1', '1 = double lockout on repeated lockouts, capped at 24h')",
        )
        .await?;

        // ── consecutive_lockouts column on user_accounts ──
        // Tracks how many times a user has been locked out (for progressive lockout).
        // Resets on successful login or admin unlock.
        db.execute_unprepared(
            "ALTER TABLE user_accounts ADD COLUMN consecutive_lockouts INTEGER NOT NULL DEFAULT 0",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS rbac_settings")
            .await?;
        // SQLite does not support DROP COLUMN; consecutive_lockouts will remain.
        Ok(())
    }
}
