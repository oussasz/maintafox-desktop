//! Migration 169 — LOTO removal verifier (PRD §6.23, WP-04 / P0-8).
//!
//! `permit_isolations.removal_verified_at` existed without recording *who* verified the
//! lock removal, so the two-person rule could not be enforced on removal. This adds
//! `removal_verified_by_id`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000169_permit_isolation_removal_verifier"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE permit_isolations ADD COLUMN removal_verified_by_id INTEGER NULL")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE permit_isolations DROP COLUMN removal_verified_by_id")
            .await?;
        Ok(())
    }
}
