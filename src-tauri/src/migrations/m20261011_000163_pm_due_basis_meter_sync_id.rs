//! Migration 163 — Device-independent meter reference in PM occurrence due bases (Phase 2.4).
//!
//! Meter-triggered occurrences had `due_basis = "meter:<equipment_meters.id>:<threshold>"`, a local
//! id that differs on every device. The generator now writes the meter's `entity_sync_id`, so the
//! same occurrence has the same due basis (part of its natural key) everywhere. Existing values
//! are rewritten the same way; a value whose meter no longer exists is left unchanged.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000163_pm_due_basis_meter_sync_id"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, due_basis FROM pm_occurrences WHERE due_basis LIKE 'meter:%'".to_string(),
            ))
            .await?;
        for row in rows {
            let id: i64 = row.try_get("", "id")?;
            let due_basis: String = row.try_get("", "due_basis")?;
            let mut parts = due_basis.splitn(3, ':');
            let (Some(_), Some(meter), Some(threshold)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let Ok(meter_id) = meter.parse::<i64>() else {
                // Already a sync identity.
                continue;
            };
            let meter_sync_id: Option<String> = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT entity_sync_id FROM equipment_meters WHERE id = ?",
                    [meter_id.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<Option<String>>("", "entity_sync_id").ok().flatten());
            let Some(meter_sync_id) = meter_sync_id else {
                continue;
            };
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE pm_occurrences SET due_basis = ? WHERE id = ?",
                [format!("meter:{meter_sync_id}:{threshold}").into(), id.into()],
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
