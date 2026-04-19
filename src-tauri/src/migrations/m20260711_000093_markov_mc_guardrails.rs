//! Migration 093 — Monte Carlo runs, Markov models, RAM advanced guardrails (PRD §6.10).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260711_000093_markov_mc_guardrails"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS mc_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                graph_json TEXT NOT NULL DEFAULT '{}',
                trials INTEGER NOT NULL DEFAULT 10000,
                seed INTEGER NULL,
                result_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_mc_models_equipment ON mc_models(equipment_id)")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS markov_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                graph_json TEXT NOT NULL DEFAULT '{}',
                result_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_markov_models_equipment ON markov_models(equipment_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS ram_advanced_guardrails (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                flags_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO ram_advanced_guardrails (id, flags_json, updated_at) VALUES (1,
            '{\"monte_carlo_enabled\":true,\"markov_enabled\":true,\"mc_max_trials\":1000000,\"markov_max_states\":128}',
            datetime('now'))",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS ram_advanced_guardrails")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS markov_models").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS mc_models").await?;
        Ok(())
    }
}
