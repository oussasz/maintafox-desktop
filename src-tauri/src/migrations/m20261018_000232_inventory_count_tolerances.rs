//! Migration 232 — stock count and reconciliation tolerances are tenant settings (owner-approved
//! Step 7).
//!
//! Until now each count session and each reconciliation run took its tolerance from the screen,
//! which always sent the same values. They are now plant parameters (Settings > Inventory),
//! validated against their catalog bounds and read by the count and the reconciliation. Seeded
//! with exactly the values the screens used, stated here:
//! - `inventory.count_critical_variance` = 5: a counted line whose variance is 5 or more (in the
//!   article's unit) is critical and needs approval.
//! - `inventory.reconciliation_drift_break` = 0.01: a ledger / balance gap of 0.01 or more is a
//!   break.
//!
//! Existing count sessions keep the tolerance they were opened with. A value already present (a
//! tenant that set it on another PC) is kept. The rows are seeded on every PC with capture
//! suppressed: each PC writes the same row with the same seeded sync identity
//! (`setting_key:setting_scope`), so nothing is sent and nothing conflicts.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000232_inventory_count_tolerances"
    }
}

/// The seeded values (also the catalog's shipped values).
const SEEDED: &[(&str, &str)] = &[
    ("inventory.count_critical_variance", "5"),
    ("inventory.reconciliation_drift_break", "0.01"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        let capture = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        for (key, value) in SEEDED {
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO app_settings \
                 (setting_key, category, setting_scope, setting_value_json, setting_risk, validation_status, \
                  last_modified_at) \
                 VALUES (?, 'inventory', 'tenant', ?, 'low', 'valid', strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                [(*key).into(), (*value).into()],
            ))
            .await?;
        }
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The tolerances may already be set by the plant: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    /// The seeded values equal the catalog's shipped values (a key retired since is out of the
    /// catalog, migration 259), a plant's own value is kept, and a second run changes nothing.
    #[tokio::test]
    async fn seeds_the_screen_values_and_keeps_a_plant_value() {
        for (key, value) in super::SEEDED {
            if crate::settings::catalog::RETIRED_PLANT_PARAMETERS.contains(key) {
                assert!(
                    crate::settings::catalog::definition(key).is_none(),
                    "{key}: retired, out of the catalog"
                );
                continue;
            }
            let def = crate::settings::catalog::definition(key).expect("in the catalog");
            assert_eq!(def.shipped_value_json, *value, "{key}: migration and catalog agree");
        }

        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261018_000232_inventory_count_tolerances")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 231");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO app_settings (setting_key, category, setting_scope, setting_value_json, setting_risk, \
             validation_status, last_modified_at) \
             VALUES ('inventory.count_critical_variance', 'inventory', 'tenant', '3', 'low', 'valid', '2026-01-01')"
                .to_string(),
        ))
        .await
        .expect("plant value");
        // 232 alone (259 later retires the count tolerance).
        Migrator::up(&db, Some(1)).await.expect("232");
        Migrator::down(&db, Some(1)).await.expect("down 232");
        Migrator::up(&db, Some(1)).await.expect("232 again");

        let value = |key: &'static str| {
            let db = &db;
            async move {
                db.query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT setting_value_json FROM app_settings WHERE setting_key = ? AND setting_scope = 'tenant'",
                    [key.into()],
                ))
                .await
                .expect("q")
                .expect("row")
                .try_get_by_index::<String>(0)
                .expect("value")
            }
        };
        assert_eq!(
            value("inventory.count_critical_variance").await,
            "3",
            "a plant value is kept"
        );
        assert_eq!(value("inventory.reconciliation_drift_break").await, "0.01");
        assert!(
            (crate::settings::catalog::decimal_setting(&db, "inventory.reconciliation_drift_break")
                .await
                .expect("read")
                - 0.01)
                .abs()
                < f64::EPSILON
        );
    }
}
