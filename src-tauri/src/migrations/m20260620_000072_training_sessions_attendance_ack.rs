//! Migration 072 — Training sessions, attendance, document acknowledgements (PRD §6.20 gap sprint 02).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260620_000072_training_sessions_attendance_ack"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS training_sessions (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id          TEXT NOT NULL UNIQUE,
                course_code             TEXT NOT NULL,
                scheduled_start         TEXT NOT NULL,
                scheduled_end           TEXT NOT NULL,
                location                TEXT NULL,
                instructor_id           INTEGER NULL REFERENCES personnel(id),
                certification_type_id   INTEGER NULL REFERENCES certification_types(id),
                min_pass_score          INTEGER NOT NULL DEFAULT 70,
                row_version             INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_training_sessions_course
             ON training_sessions(course_code)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS training_attendance (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id      TEXT NOT NULL UNIQUE,
                session_id          INTEGER NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
                personnel_id        INTEGER NOT NULL REFERENCES personnel(id),
                attendance_status   TEXT NOT NULL,
                completed_at        TEXT NULL,
                score               REAL NULL,
                row_version         INTEGER NOT NULL DEFAULT 1,
                UNIQUE(session_id, personnel_id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_training_attendance_personnel
             ON training_attendance(personnel_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS document_acknowledgements (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id        TEXT NOT NULL UNIQUE,
                personnel_id          INTEGER NOT NULL REFERENCES personnel(id),
                document_version_id   INTEGER NOT NULL,
                acknowledged_at       TEXT NOT NULL,
                row_version           INTEGER NOT NULL DEFAULT 1,
                UNIQUE(personnel_id, document_version_id)
            )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS document_acknowledgements")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS training_attendance").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS training_sessions").await?;
        Ok(())
    }
}
