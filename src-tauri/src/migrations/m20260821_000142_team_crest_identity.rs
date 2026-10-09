//! Migration 142 — Crew identity: icon_key + accent_color on teams.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260821_000142_team_crest_identity"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (step, sql) in [
            ("icon_key", "ALTER TABLE teams ADD COLUMN icon_key TEXT NULL"),
            (
                "accent_color",
                "ALTER TABLE teams ADD COLUMN accent_color TEXT NULL",
            ),
        ] {
            if let Err(e) = db.execute_unprepared(sql).await {
                let msg = e.to_string().to_lowercase();
                if msg.contains("duplicate column") {
                    continue;
                }
                return Err(DbErr::Custom(format!(
                    "team_crest_identity step '{step}' failed: {e}"
                )));
            }
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
