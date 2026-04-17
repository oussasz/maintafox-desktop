use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260503_000058_budget_controls_alerting_reporting"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_alert_configs (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id     INTEGER NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id        INTEGER NULL REFERENCES cost_centers(id) ON DELETE CASCADE,
                budget_bucket         TEXT NULL,
                alert_type            TEXT NOT NULL,
                threshold_pct         REAL NULL,
                threshold_amount      REAL NULL,
                recipient_user_id     INTEGER NULL REFERENCES user_accounts(id),
                recipient_role_id     INTEGER NULL REFERENCES roles(id),
                labor_template        TEXT NULL,
                dedupe_window_minutes INTEGER NOT NULL DEFAULT 240,
                requires_ack          INTEGER NOT NULL DEFAULT 1,
                is_active             INTEGER NOT NULL DEFAULT 1,
                row_version           INTEGER NOT NULL DEFAULT 1,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_alert_configs_scope
             ON budget_alert_configs(budget_version_id, cost_center_id, alert_type, is_active)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_alert_events (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                alert_config_id     INTEGER NULL REFERENCES budget_alert_configs(id) ON DELETE SET NULL,
                budget_version_id   INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id      INTEGER NOT NULL REFERENCES cost_centers(id) ON DELETE CASCADE,
                period_month        INTEGER NULL,
                budget_bucket       TEXT NOT NULL,
                alert_type          TEXT NOT NULL,
                severity            TEXT NOT NULL DEFAULT 'warning',
                title               TEXT NOT NULL,
                message             TEXT NOT NULL,
                dedupe_key          TEXT NOT NULL,
                current_value       REAL NOT NULL,
                threshold_value     REAL NULL,
                variance_amount     REAL NULL,
                currency_code       TEXT NOT NULL,
                payload_json        TEXT NULL,
                notification_event_id INTEGER NULL REFERENCES notification_events(id) ON DELETE SET NULL,
                notification_id     INTEGER NULL REFERENCES notifications(id) ON DELETE SET NULL,
                acknowledged_at     TEXT NULL,
                acknowledged_by_id  INTEGER NULL REFERENCES user_accounts(id),
                acknowledgement_note TEXT NULL,
                row_version         INTEGER NOT NULL DEFAULT 1,
                created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_alert_events_scope
             ON budget_alert_events(budget_version_id, cost_center_id, period_month, alert_type)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_alert_events_dedupe
             ON budget_alert_events(dedupe_key, created_at)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO notification_categories
                (code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES
                ('budget_control_alert', 'Budget Control Alert', 'warning', 1, 0)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, is_active)
             SELECT 'budget_control_alert', 'manual', 1, 240, 1
             WHERE NOT EXISTS (
               SELECT 1 FROM notification_rules WHERE category_code = 'budget_control_alert'
             )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS budget_alert_events")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_alert_configs")
            .await?;
        Ok(())
    }
}
