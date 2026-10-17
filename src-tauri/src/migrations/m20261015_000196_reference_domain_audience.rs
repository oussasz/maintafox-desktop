//! Migration 196 — audience of each reference domain (owner rule on Données de référence).
//!
//! `reference_domains.audience`:
//! - `system`: a list shipped and maintained by Maintafox that the program's logic depends on.
//!   Hidden from the Données de référence screens and read-only for users; pickers still read it.
//! - `company`: a list that differs from one company to the next, configured by the plant.
//!
//! Backfill: every existing domain is `company` except the Maintafox lists below. Domains created
//! by users later are always `company` (`reference::domains::create_reference_domain`); seeders
//! declare the audience of the domains they create. `reference_domains` is synchronised; the new
//! column travels with the row like the other columns.
//!
//! `reference_sets.impact_previewed_at`: when the publish impact preview last ran for the set on
//! this device. Publishing a protected domain requires it. It replaces a process-wide memory keyed by
//! set id, which was lost on restart and leaked between databases that share ids. The triggers below
//! clear it whenever a value of the set is inserted, changed or deleted (manual edits, imports,
//! merges, inbound sync), so a preview always describes the values being published. The column is
//! device-local in sync (`sync::row_spec`): a preview made on one device does not count for another.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000196_reference_domain_audience"
    }
}

/// Domains maintained by Maintafox at the time of this migration:
/// - `EQUIPMENT.STATUS`: status codes drive commissioning / decommissioning logic;
/// - `EQUIPMENT.CRITICALITY`: the criticality scale (A/B/C) of the risk matrix;
/// - `DI.DISPOSITION`: closure dispositions of a request, each one tied to a rule of the request
///   workflow (converted to WO, duplicate needs the related request, other needs notes…).
pub const SYSTEM_DOMAIN_CODES: &[&str] = &["EQUIPMENT.STATUS", "EQUIPMENT.CRITICALITY", "DI.DISPOSITION"];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// Any change to a value of a set invalidates the impact preview of that set.
const CLEAR_IMPACT_PREVIEW_TRIGGERS: &[&str] = &[
    "CREATE TRIGGER IF NOT EXISTS trg_reference_values_clear_impact_preview_ai \
     AFTER INSERT ON reference_values BEGIN \
       UPDATE reference_sets SET impact_previewed_at = NULL \
        WHERE id = NEW.set_id AND impact_previewed_at IS NOT NULL; \
     END",
    "CREATE TRIGGER IF NOT EXISTS trg_reference_values_clear_impact_preview_au \
     AFTER UPDATE ON reference_values BEGIN \
       UPDATE reference_sets SET impact_previewed_at = NULL \
        WHERE id IN (NEW.set_id, OLD.set_id) AND impact_previewed_at IS NOT NULL; \
     END",
    "CREATE TRIGGER IF NOT EXISTS trg_reference_values_clear_impact_preview_ad \
     AFTER DELETE ON reference_values BEGIN \
       UPDATE reference_sets SET impact_previewed_at = NULL \
        WHERE id = OLD.set_id AND impact_previewed_at IS NOT NULL; \
     END",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        if !column_exists(db, "reference_domains", "audience").await? {
            db.execute_unprepared(
                "ALTER TABLE reference_domains ADD COLUMN audience TEXT NOT NULL DEFAULT 'company' \
                 CHECK (audience IN ('system', 'company'))",
            )
            .await?;
        }

        let placeholders = vec!["?"; SYSTEM_DOMAIN_CODES.len()].join(", ");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE reference_domains \
                    SET audience = CASE WHEN UPPER(TRIM(code)) IN ({placeholders}) THEN 'system' ELSE 'company' END"
            ),
            SYSTEM_DOMAIN_CODES.iter().map(|c| sea_orm::Value::from(*c)),
        ))
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_reference_domains_audience ON reference_domains(audience)",
        )
        .await?;

        if !column_exists(db, "reference_sets", "impact_previewed_at").await? {
            db.execute_unprepared("ALTER TABLE reference_sets ADD COLUMN impact_previewed_at TEXT NULL")
                .await?;
        }
        for ddl in CLEAR_IMPACT_PREVIEW_TRIGGERS {
            db.execute_unprepared(ddl).await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
