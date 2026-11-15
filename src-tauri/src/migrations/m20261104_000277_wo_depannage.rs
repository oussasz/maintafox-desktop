//! Migration 277 — the "Dépannage" (breakdown) path of a work order (CMMS Advisor decision,
//! 2026-11-04; `wo::depannage`).
//!
//! A dépannage is a corrective work order created and started in one step by the person who goes to
//! the breakdown: unscheduled, it never enters a weekly schedule, and its planning is validated
//! after the fact. The work order records:
//! - `is_depannage` (0 / 1): raised by the dépannage path;
//! - `breakdown_at` ("Panne constatée à", UTC): when the breakdown was found, required for a
//!   dépannage, never in the future;
//! - `machine_stopped` (0 / 1, "Machine arrêtée"): required for a dépannage, no default;
//! - `validated_by_id`, `validated_at`, `validation_auto_approved`: the after-the-fact validation
//!   of the dépannage (who validated it and when, or that it was approved automatically). Written
//!   by the validation step, not by this version: the columns exist so the dépannage needs one
//!   migration.
//!
//! All the new columns sync with the row (the layout reads the table's columns; `validated_by_id`
//! travels as a user reference). Existing work orders are untouched: `is_depannage` 0, the others
//! NULL.
//!
//! Two notification categories and their rules:
//! - `wo_depannage_started` (module `ot`): the planners of the work order's site (`plan.edit` there)
//!   and the responsibles of its node are told that a dépannage started, or was created and waits
//!   for its permit, in-app;
//! - `wo_depannage_permit_needed` ("permis à établir — dépannage", CMMS Advisor 2026-11-05): the
//!   permit-issue right holders of the work order's site are told at once that a dépannage waits for
//!   its permit. It cannot be muted or re-routed (`notifications::catalog::ALWAYS_DELIVERED`, with
//!   its fallback); its rule row mirrors that fixed audience.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000277_wo_depannage"
    }
}

const CATEGORY: &str = crate::wo::depannage::NOTIFICATION_CATEGORY;
const PERMIT_NEEDED: &str = crate::wo::depannage::PERMIT_NEEDED_CATEGORY;

/// `(code, label, default severity, user configurable, module, audiences)` of each category.
fn categories() -> [(&'static str, &'static str, &'static str, i64, &'static str, String); 2] {
    let permission = crate::notifications::catalog::PERMISSION_AUDIENCE_PREFIX;
    [
        (
            CATEGORY,
            "Dépannage started or waiting for its permit",
            "warning",
            1,
            "ot",
            serde_json::json!([
                format!("{permission}{}", crate::rbac::permissions::PLAN_EDIT),
                "entity_manager",
            ])
            .to_string(),
        ),
        (
            PERMIT_NEEDED,
            "Permit to issue for a dépannage",
            "critical",
            0,
            "system",
            serde_json::json!([format!("{permission}{}", crate::rbac::permissions::PTW_ISSUE)]).to_string(),
        ),
    ]
}

const COLUMNS: &[(&str, &str)] = &[
    ("is_depannage", "INTEGER NOT NULL DEFAULT 0"),
    ("breakdown_at", "TEXT NULL"),
    ("machine_stopped", "INTEGER NULL"),
    ("validated_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("validated_at", "TEXT NULL"),
    ("validation_auto_approved", "INTEGER NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('work_orders') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE work_orders ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        // Made by this migration on every PC with the same identities: nothing is sent.
        let capture = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
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
        for (code, label, severity, configurable, module, audiences) in categories() {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR IGNORE INTO notification_categories \
                 (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
                  module_code, is_offered) VALUES (?, ?, ?, ?, 0, ?, ?, 1)",
                [
                    seeded_entity_sync_id("notification_categories", code).into(),
                    code.into(),
                    label.into(),
                    severity.into(),
                    configurable.into(),
                    module.into(),
                ],
            ))
            .await?;
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO notification_rules \
                 (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
                  escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
                 SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, ? \
                 WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
                [
                    seeded_entity_sync_id("notification_rules", code).into(),
                    code.into(),
                    audiences.into(),
                    code.into(),
                ],
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The dépannage facts of a work order are part of its history: the columns are kept.
        let db = manager.get_connection();
        for code in [CATEGORY, PERMIT_NEEDED] {
            for (table, column) in [
                ("notification_rules", "category_code"),
                ("notification_categories", "code"),
            ] {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    format!("DELETE FROM {table} WHERE {column} = ?"),
                    [code.into()],
                ))
                .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    use crate::sync::row_layout::{Role, Target};
    use crate::sync::test_support::{scalar_i64, started_db, started_db_before_migration, RowFactory};

    /// An upgraded database: its work orders are kept as they were (not dépannages, every new fact
    /// empty), the category and its rule exist once, and a second run changes nothing.
    #[tokio::test]
    async fn an_upgraded_database_keeps_its_work_orders_untouched() {
        let db = started_db_before_migration("m20261104_000277_wo_depannage").await;
        let mut factory = RowFactory::new();
        factory.create(&db, "work_orders").await.expect("old work order");
        factory.create(&db, "work_orders").await.expect("old work order");
        let before = scalar_i64(&db, "SELECT COUNT(*) FROM work_orders", vec![]).await;
        assert!(before >= 2);

        crate::migrations::Migrator::up(&db, None).await.expect("migration 277");
        for _ in 0..2 {
            assert_eq!(
                scalar_i64(&db, "SELECT COUNT(*) FROM work_orders", vec![]).await,
                before
            );
            assert_eq!(
                scalar_i64(
                    &db,
                    "SELECT COUNT(*) FROM work_orders WHERE is_depannage = 0 AND breakdown_at IS NULL \
                       AND machine_stopped IS NULL AND validated_by_id IS NULL AND validated_at IS NULL \
                       AND validation_auto_approved IS NULL",
                    vec![],
                )
                .await,
                before,
                "existing work orders are not dépannages and carry no new fact"
            );
            for code in [super::CATEGORY, super::PERMIT_NEEDED] {
                for (table, column) in [
                    ("notification_categories", "code"),
                    ("notification_rules", "category_code"),
                ] {
                    assert_eq!(
                        scalar_i64(
                            &db,
                            &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?"),
                            vec![code.into()],
                        )
                        .await,
                        1,
                        "{table} {code}"
                    );
                }
            }
            // The issuers' notice cannot be muted; its rule mirrors its fixed audience.
            assert_eq!(
                scalar_i64(
                    &db,
                    "SELECT COUNT(*) FROM notification_categories WHERE code = ? AND is_user_configurable = 0 \
                       AND default_severity = 'critical'",
                    vec![super::PERMIT_NEEDED.into()],
                )
                .await,
                1
            );
            assert_eq!(
                crate::notifications::catalog::always_delivered_audience(super::PERMIT_NEEDED).map(|a| a.code()),
                Some(format!(
                    "{}{}",
                    crate::notifications::catalog::PERMISSION_AUDIENCE_PREFIX,
                    crate::rbac::permissions::PTW_ISSUE
                ))
            );
            // Run again: nothing changes.
            crate::migrations::Migrator::up(&db, None)
                .await
                .expect("migration 277 again");
        }

        // The validator travels as a user reference, the facts as values.
        let db = started_db().await;
        let layout = crate::sync::row_layout::layout(&db, "work_orders")
            .await
            .expect("layout");
        let validator = layout
            .columns
            .iter()
            .find(|c| c.name == "validated_by_id")
            .expect("validator in the layout");
        match &validator.role {
            Role::Ref {
                target: Target::Entity { entity_type, .. },
                ..
            } => assert_eq!(*entity_type, "user_accounts"),
            other => panic!("validator must travel as a user reference, got {other:?}"),
        }
        for name in [
            "is_depannage",
            "breakdown_at",
            "machine_stopped",
            "validated_at",
            "validation_auto_approved",
        ] {
            let column = layout
                .columns
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name} in the layout"));
            assert!(matches!(&column.role, Role::Value { .. }), "{name}: {:?}", column.role);
        }
    }
}
