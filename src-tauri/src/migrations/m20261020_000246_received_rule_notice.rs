//! Migration 246 — the notification category "safety rule changed on another PC"
//! (`settings::received_notice`, CMMS Advisor decisions A/B on KNOWN_LIMITS rows 30/32/33).
//!
//! When a site's four-eyes opt-out or a permit type's three-person lock-removal setting changed on
//! another PC is applied here, the users holding the settings-administration right (`adm.settings`)
//! are told in-app. The category is not one users may switch off (`is_user_configurable = 0`): it
//! is the administrators' check that a safety rule changed elsewhere was meant. It always goes to
//! `permission:adm.settings` (`notifications::catalog::ALWAYS_DELIVERED`); its rule cannot be edited,
//! and these rows stay on each PC (never synchronised).
//! (Number 245 is reserved by another change.)

use sea_orm::{DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000246_received_rule_notice"
    }
}

const CATEGORY: &str = crate::settings::received_notice::RECEIVED_RULE_CATEGORY;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, 'Safety rule changed on another PC', 'warning', 0, 0, 'system', 1)",
            [
                seeded_entity_sync_id("notification_categories", CATEGORY).into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO notification_rules \
             (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
              escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
             SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, '[\"permission:adm.settings\"]' \
             WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
            [
                seeded_entity_sync_id("notification_rules", CATEGORY).into(),
                CATEGORY.into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (table, column) in [
            ("notification_rules", "category_code"),
            ("notification_categories", "code"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("DELETE FROM {table} WHERE {column} = ?"),
                [CATEGORY.into()],
            ))
            .await?;
        }
        Ok(())
    }
}
