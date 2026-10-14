//! Migration 185 — asset spare-parts list and criticality assessments.
//!
//! - `equipment_spare_parts`: the spare parts (stock articles) an asset needs, with the quantity
//!   installed and where they sit on the asset (SAP PM equipment BOM / Maximo asset spare parts).
//!   Removing a line sets `is_active = 0` so the removal replicates like any other edit.
//!   No unique index on (equipment, article): two devices may add the same line offline and a
//!   unique index would stop the second one arriving; the service refuses duplicates locally.
//! - `equipment_criticality_assessments`: append-only history of criticality ratings made with a
//!   documented method (`assets::criticality`): consequence scores, likelihood, the risk score and
//!   the resulting A–D class that was written to the asset.
//!
//! Both tables are synchronised (entity types `equipment_spare_parts`,
//! `equipment_criticality_assessments`).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000185_asset_spares_criticality"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS equipment_spare_parts (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id    TEXT NOT NULL UNIQUE,
                equipment_id      INTEGER NOT NULL REFERENCES equipment(id),
                article_id        INTEGER NOT NULL REFERENCES articles(id),
                quantity          REAL NOT NULL CHECK (quantity > 0),
                position_label    TEXT NULL,
                is_critical_spare INTEGER NOT NULL DEFAULT 0,
                notes             TEXT NULL,
                is_active         INTEGER NOT NULL DEFAULT 1,
                row_version       INTEGER NOT NULL DEFAULT 1,
                created_by_id     INTEGER NULL REFERENCES user_accounts(id),
                created_at        TEXT NOT NULL,
                updated_at        TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_spare_parts_equipment \
             ON equipment_spare_parts(equipment_id, is_active)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_spare_parts_article \
             ON equipment_spare_parts(article_id, is_active)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS equipment_criticality_assessments (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id    TEXT NOT NULL UNIQUE,
                equipment_id      INTEGER NOT NULL REFERENCES equipment(id),
                method_code       TEXT NOT NULL,
                safety_score      INTEGER NOT NULL CHECK (safety_score BETWEEN 1 AND 5),
                environment_score INTEGER NOT NULL CHECK (environment_score BETWEEN 1 AND 5),
                production_score  INTEGER NOT NULL CHECK (production_score BETWEEN 1 AND 5),
                quality_score     INTEGER NOT NULL CHECK (quality_score BETWEEN 1 AND 5),
                cost_score        INTEGER NOT NULL CHECK (cost_score BETWEEN 1 AND 5),
                likelihood_score  INTEGER NOT NULL CHECK (likelihood_score BETWEEN 1 AND 5),
                risk_score        INTEGER NOT NULL,
                resulting_class   TEXT NOT NULL CHECK (resulting_class IN ('A','B','C','D')),
                previous_class    TEXT NULL,
                rationale         TEXT NULL,
                assessed_by_id    INTEGER NULL REFERENCES user_accounts(id),
                assessed_at       TEXT NOT NULL,
                created_at        TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_criticality_assessments_equipment \
             ON equipment_criticality_assessments(equipment_id, assessed_at)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS equipment_criticality_assessments")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS equipment_spare_parts")
            .await?;
        Ok(())
    }
}
