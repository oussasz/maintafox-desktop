//! Migration 054 - Planning break-ins, audit discipline, and notification support.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260429_000054_planning_breakins_notifications"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_break_ins (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_commitment_id       INTEGER NOT NULL REFERENCES schedule_commitments(id) ON DELETE CASCADE,
                break_in_reason              TEXT NOT NULL,
                approved_by_user_id          INTEGER NULL REFERENCES users(id),
                approved_by_personnel_id     INTEGER NULL REFERENCES personnel(id),
                override_reason              TEXT NULL,
                old_slot_start               TEXT NOT NULL,
                old_slot_end                 TEXT NOT NULL,
                new_slot_start               TEXT NOT NULL,
                new_slot_end                 TEXT NOT NULL,
                old_assignee_id              INTEGER NULL REFERENCES personnel(id),
                new_assignee_id              INTEGER NULL REFERENCES personnel(id),
                cost_impact_delta            REAL NULL,
                notification_dedupe_key      TEXT NULL,
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_by_id                INTEGER NULL REFERENCES users(id),
                created_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_break_ins_commitment
             ON schedule_break_ins(schedule_commitment_id, created_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_break_ins_reason
             ON schedule_break_ins(break_in_reason, created_at)",
        )
        .await?;

        let _ = db.execute_unprepared(
            "ALTER TABLE schedule_change_log
             ADD COLUMN field_changed TEXT NULL",
        ).await;
        let _ = db.execute_unprepared(
            "ALTER TABLE schedule_change_log
             ADD COLUMN old_value TEXT NULL",
        ).await;
        let _ = db.execute_unprepared(
            "ALTER TABLE schedule_change_log
             ADD COLUMN new_value TEXT NULL",
        ).await;
        let _ = db.execute_unprepared(
            "ALTER TABLE schedule_change_log
             ADD COLUMN reason_code TEXT NULL",
        ).await;
        let _ = db.execute_unprepared(
            "ALTER TABLE schedule_change_log
             ADD COLUMN reason_note TEXT NULL",
        ).await;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_break_ins")
            .await?;
        Ok(())
    }
}