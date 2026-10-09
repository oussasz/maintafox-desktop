//! Per-user executive dashboard layouts for the Analytics hub (device-local).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260826_000150_user_analytics_dashboard_layouts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS user_analytics_dashboard_layouts (
                user_id INTEGER PRIMARY KEY NOT NULL REFERENCES user_accounts(id) ON DELETE CASCADE,
                layout_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS user_analytics_dashboard_layouts")
            .await?;
        Ok(())
    }
}
