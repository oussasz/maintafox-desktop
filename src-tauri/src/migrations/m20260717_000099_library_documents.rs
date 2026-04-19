//! Migration 099 — Tenant library documents (PRD §6.15).
//!
//! Files stored under app data `library_documents/`, same pattern as WO/DI attachments.

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260717_000099_library_documents"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            r"CREATE TABLE IF NOT EXISTS library_documents (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                category         TEXT    NOT NULL,
                equipment_id     INTEGER NULL REFERENCES equipment(id),
                title            TEXT    NOT NULL DEFAULT '',
                file_name        TEXT    NOT NULL,
                relative_path    TEXT    NOT NULL UNIQUE,
                mime_type        TEXT    NOT NULL DEFAULT 'application/octet-stream',
                size_bytes       INTEGER NOT NULL DEFAULT 0,
                uploaded_by_id   INTEGER NULL REFERENCES user_accounts(id),
                uploaded_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                notes            TEXT    NULL,
                CHECK (category IN (
                    'technical_manuals',
                    'sops',
                    'safety_protocols',
                    'compliance_certificates'
                ))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_library_documents_category \
             ON library_documents(category)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_library_documents_equipment \
             ON library_documents(equipment_id)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS library_documents".to_string(),
        ))
        .await?;
        Ok(())
    }
}
