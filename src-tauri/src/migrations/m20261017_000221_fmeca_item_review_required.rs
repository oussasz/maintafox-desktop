//! Migration 221 — FMECA rows to review (`fmeca_items.review_required`).
//!
//! The root-cause (Ishikawa) diagram used to promote a cause to an FMECA row with placeholder
//! ratings (severity = occurrence = detection = 5, English action "Address RCA cause: …") that no
//! person chose. The promotion now opens the study's FMECA form, where the analyst rates the row.
//!
//! Rows created that way are flagged, not changed or deleted: their ratings stay as stored and the
//! worksheet shows them "to review" until a person saves the row. Only rows with the exact
//! placeholder signature are flagged. Additive synced column (the row-sync engine sends every
//! column); every device runs the same backfill, so replicas agree.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000221_fmeca_item_review_required"
    }
}

/// The placeholder row written by the former Ishikawa promotion (alias-free, `fmeca_items`).
pub const PLACEHOLDER_SIGNATURE_SQL: &str = "source_ram_ishikawa_diagram_id IS NOT NULL \
     AND severity = 5 AND occurrence = 5 AND detectability = 5 \
     AND recommended_action LIKE 'Address RCA cause: %'";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let has_column = db
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "SELECT 1 FROM pragma_table_info('fmeca_items') WHERE name = 'review_required'".to_string(),
            ))
            .await?
            .is_some();
        if !has_column {
            db.execute_unprepared(
                "ALTER TABLE fmeca_items ADD COLUMN review_required INTEGER NOT NULL DEFAULT 0 \
                 CHECK (review_required IN (0, 1))",
            )
            .await?;
        }
        db.execute_unprepared(&format!(
            "UPDATE fmeca_items SET review_required = 1 WHERE review_required = 0 AND {PLACEHOLDER_SIGNATURE_SQL}"
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE fmeca_items DROP COLUMN review_required")
            .await?;
        Ok(())
    }
}
