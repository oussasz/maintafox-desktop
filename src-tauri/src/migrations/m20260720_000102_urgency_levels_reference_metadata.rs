//! Migration 102 — `urgency_levels` reference metadata (`code`, `is_system`, `is_active`) for
//! Données de référence management and IPC listing.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260720_000102_urgency_levels_reference_metadata"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE urgency_levels ADD COLUMN code TEXT".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE urgency_levels ADD COLUMN is_system INTEGER NOT NULL DEFAULT 1".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE urgency_levels ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1".to_string(),
        ))
        .await?;

        // Canonical codes + French labels (5-level scale; includes Basse → Urgente progression).
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE urgency_levels SET \
                code = 'very_low', \
                label = 'Very Low', \
                label_fr = 'Très basse', \
                hex_color = '#64748B', \
                is_system = 1 \
             WHERE level = 1"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE urgency_levels SET \
                code = 'low', \
                label = 'Low', \
                label_fr = 'Basse', \
                hex_color = '#3B82F6', \
                is_system = 1 \
             WHERE level = 2"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE urgency_levels SET \
                code = 'medium', \
                label = 'Medium', \
                label_fr = 'Normale', \
                hex_color = '#F59E0B', \
                is_system = 1 \
             WHERE level = 3"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE urgency_levels SET \
                code = 'high', \
                label = 'High', \
                label_fr = 'Haute', \
                hex_color = '#F97316', \
                is_system = 1 \
             WHERE level = 4"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE urgency_levels SET \
                code = 'critical', \
                label = 'Critical', \
                label_fr = 'Urgente', \
                hex_color = '#DC2626', \
                is_system = 1 \
             WHERE level = 5"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_urgency_levels_code ON urgency_levels(code)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot drop columns cleanly; metadata is backward-compatible.
        Ok(())
    }
}
