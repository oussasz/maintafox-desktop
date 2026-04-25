//! Migration 070 — Qualification schema (PRD §6.20, gap sprint 01).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260618_000070_qualification_schema"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS certification_types (
                id                       INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id           TEXT NOT NULL UNIQUE,
                code                     TEXT NOT NULL UNIQUE,
                name                     TEXT NOT NULL,
                default_validity_months  INTEGER NULL,
                renewal_lead_days        INTEGER NULL,
                row_version              INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_certifications (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id          TEXT NOT NULL UNIQUE,
                personnel_id            INTEGER NOT NULL REFERENCES personnel(id),
                certification_type_id   INTEGER NOT NULL REFERENCES certification_types(id),
                issued_at               TEXT NULL,
                expires_at              TEXT NULL,
                issuing_body            TEXT NULL,
                certificate_ref         TEXT NULL,
                verification_status     TEXT NOT NULL,
                row_version             INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_personnel_certifications_personnel_id
             ON personnel_certifications(personnel_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS qualification_requirement_profiles (
                id                                INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id                    TEXT NOT NULL UNIQUE,
                profile_name                      TEXT NOT NULL,
                required_certification_type_ids_json TEXT NOT NULL DEFAULT '[]',
                applies_to_permit_type_codes_json    TEXT NOT NULL DEFAULT '[]',
                row_version                       INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS qualification_requirement_profiles")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_certifications").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS certification_types").await?;
        Ok(())
    }
}
