//! Migration 207 — the database refuses an impossible planned crew size.
//!
//! `planned_crew_size` (migration 206) is a whole number of people from 1 to 50
//! (`planning::load::MAX_CREW_SIZE`), or NULL (not estimated). SQLite cannot add a CHECK to an
//! existing column, so triggers enforce it on insert and update of the three tables that carry it —
//! including rows that arrive by sync.

use sea_orm::{ConnectionTrait, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000207_crew_size_bounds"
    }
}

const TABLES: [&str; 3] = ["work_orders", "pm_plan_versions", "schedule_candidates"];

/// Kept equal to `planning::load::MAX_CREW_SIZE`.
const MAX_CREW_SIZE: i64 = 50;

fn guard(table: &str, event: &str) -> String {
    let on = if event == "UPDATE" {
        format!("UPDATE OF planned_crew_size ON {table}")
    } else {
        format!("INSERT ON {table}")
    };
    format!(
        "CREATE TRIGGER IF NOT EXISTS trg_{table}_crew_size_{suffix} BEFORE {on} \
         WHEN NEW.planned_crew_size IS NOT NULL AND (typeof(NEW.planned_crew_size) <> 'integer' \
              OR NEW.planned_crew_size < 1 OR NEW.planned_crew_size > {MAX_CREW_SIZE}) \
         BEGIN SELECT RAISE(ABORT, 'planned_crew_size must be a whole number from 1 to {MAX_CREW_SIZE}'); END",
        suffix = event.to_ascii_lowercase(),
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for table in TABLES {
            for event in ["INSERT", "UPDATE"] {
                txn.execute_unprepared(&guard(table, event)).await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for table in TABLES {
            for suffix in ["insert", "update"] {
                db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS trg_{table}_crew_size_{suffix}"))
                    .await?;
            }
        }
        Ok(())
    }
}
