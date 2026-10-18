//! Migration 222 — a reliability decision keeps the site it was proposed for.
//!
//! Four-eyes on reliability decisions follows the decision's site (per-site opt-out,
//! `settings::site`). The site is stored when the decision is proposed (`site_node_id`, the planning
//! site of its equipment; NULL when it could not be resolved) and `site_recorded` = 1 says it was.
//! At approval the stricter of the stored site and the equipment's current site applies, so moving
//! equipment to an opted-out site never lifts four-eyes on a decision already proposed. Decisions
//! recorded before this version keep `site_recorded` = 0 and are judged on their current site.
//! Additive: devices on an older version ignore the columns.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000222_decision_site"
    }
}

async fn has_column<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        if !has_column(db, "reliability_study_decisions", "site_node_id").await? {
            db.execute_unprepared(
                "ALTER TABLE reliability_study_decisions ADD COLUMN site_node_id INTEGER NULL REFERENCES org_nodes(id)",
            )
            .await?;
        }
        if !has_column(db, "reliability_study_decisions", "site_recorded").await? {
            db.execute_unprepared(
                "ALTER TABLE reliability_study_decisions ADD COLUMN site_recorded INTEGER NOT NULL DEFAULT 0 \
                 CHECK (site_recorded IN (0, 1))",
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite drops columns from 3.35; the additive columns are harmless when left in place.
        Ok(())
    }
}
