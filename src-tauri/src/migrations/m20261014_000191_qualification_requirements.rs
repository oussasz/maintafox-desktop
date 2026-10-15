//! Migration 191 — qualification requirements, enforcement overrides, certification categories.
//!
//! - `certification_types` gains `category` (electrical habilitation, LOTO authority, confined
//!   space, work at height, welding, lifting equipment, first aid, hot work, other),
//!   `description` and `is_active`, so the catalogue reads like the plant's safety training plan.
//! - `qualification_requirements`: what a job needs from the people doing it (SAP PM task-list
//!   qualifications / Maximo craft-skill and qualification requirements). A requirement targets
//!   one work order (`work_order_id`), every work order of a type (`work_order_type_id`, a value
//!   of the `WORK.ORDER_TYPES` reference list, like `work_orders.type_id` since migration 198), every
//!   permit of a type (`permit_type_id`) — by id, so renaming a type cannot disable it — or a LOTO
//!   role (`target_code` = `isolation_apply` / `isolation_verify` / `removal_verify`), and asks for a
//!   certification type or a skill at a minimum level. `coverage` says whether every person
//!   assigned needs it (`each`) or at least one member of the crew (`any`).
//! - `qualification_overrides`: append-only record of an authorised supervisor letting a person
//!   work although a requirement was not met (who, on what, which gaps, why, when). The person
//!   is a personnel record, or — on a work order — a login that has no personnel record.
//!
//! Synchronised entity types: `qualification_requirements`, `qualification_overrides`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000191_qualification_requirements"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for (column, definition) in [
            ("category", "TEXT NOT NULL DEFAULT 'other'"),
            ("description", "TEXT NULL"),
            ("is_active", "INTEGER NOT NULL DEFAULT 1"),
        ] {
            if !has_column(db, "certification_types", column).await? {
                db.execute_unprepared(&format!(
                    "ALTER TABLE certification_types ADD COLUMN {column} {definition}"
                ))
                .await?;
            }
        }

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS qualification_requirements (
                id                       INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id           TEXT NOT NULL UNIQUE,
                target_kind              TEXT NOT NULL CHECK (target_kind IN
                                            ('work_order', 'work_order_type', 'permit_type', 'loto_role')),
                work_order_id            INTEGER NULL REFERENCES work_orders(id),
                work_order_type_id       INTEGER NULL REFERENCES reference_values(id),
                permit_type_id           INTEGER NULL REFERENCES permit_types(id),
                target_code              TEXT NULL,
                certification_type_id    INTEGER NULL REFERENCES certification_types(id),
                skill_reference_value_id INTEGER NULL REFERENCES reference_values(id),
                min_proficiency_level    INTEGER NULL CHECK (min_proficiency_level BETWEEN 1 AND 5),
                coverage                 TEXT NOT NULL DEFAULT 'each' CHECK (coverage IN ('each', 'any')),
                notes                    TEXT NULL,
                is_active                INTEGER NOT NULL DEFAULT 1,
                row_version              INTEGER NOT NULL DEFAULT 1,
                created_by_id            INTEGER NULL REFERENCES user_accounts(id),
                created_at               TEXT NOT NULL,
                updated_at               TEXT NOT NULL,
                CHECK ((certification_type_id IS NOT NULL) + (skill_reference_value_id IS NOT NULL) = 1),
                CHECK ((target_kind = 'work_order' AND work_order_id IS NOT NULL)
                    OR (target_kind = 'work_order_type' AND work_order_type_id IS NOT NULL)
                    OR (target_kind = 'permit_type' AND permit_type_id IS NOT NULL)
                    OR (target_kind = 'loto_role' AND target_code IS NOT NULL AND trim(target_code) <> ''))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qualification_requirements_target \
             ON qualification_requirements(target_kind, work_order_type_id, permit_type_id, target_code, is_active)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qualification_requirements_wo \
             ON qualification_requirements(work_order_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS qualification_overrides (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id  TEXT NOT NULL UNIQUE,
                personnel_id    INTEGER NULL REFERENCES personnel(id),
                user_id         INTEGER NULL REFERENCES user_accounts(id),
                context_kind    TEXT NOT NULL CHECK (context_kind IN ('work_order', 'permit_role')),
                work_order_id   INTEGER NULL REFERENCES work_orders(id),
                permit_id       INTEGER NULL REFERENCES work_permits(id),
                role_code       TEXT NULL,
                gaps_json       TEXT NOT NULL,
                reason          TEXT NOT NULL CHECK (length(trim(reason)) >= 10),
                approved_by_id  INTEGER NOT NULL REFERENCES user_accounts(id),
                approved_at     TEXT NOT NULL,
                created_at      TEXT NOT NULL,
                CHECK ((context_kind = 'work_order' AND work_order_id IS NOT NULL)
                    OR (context_kind = 'permit_role' AND permit_id IS NOT NULL AND role_code IS NOT NULL)),
                -- A personnel record, or a login that has none (work orders only).
                CHECK ((personnel_id IS NULL) <> (user_id IS NULL)),
                CHECK (user_id IS NULL OR context_kind = 'work_order')
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qualification_overrides_person \
             ON qualification_overrides(personnel_id, context_kind)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qualification_overrides_wo \
             ON qualification_overrides(work_order_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qualification_overrides_user \
             ON qualification_overrides(user_id, work_order_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS qualification_overrides")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS qualification_requirements")
            .await?;
        Ok(())
    }
}
