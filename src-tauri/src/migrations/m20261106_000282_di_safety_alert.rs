//! Migration 282 — the notification category "request flagged for safety" (`di_safety_alert`,
//! `settings::received_notice::announce_di_safety_alert`, CMMS Advisor decision on L4-11).
//!
//! A request flagged "sécurité" is told at once, critical, to the request reviewers (`di.review`)
//! and the permit-issue right holders (`ptw.issue`) of its site, else to the responsibles of its
//! node. Like the other locked safety notices (migration 246): users may not switch it off
//! (`is_user_configurable = 0`), it always goes to its fixed audiences
//! (`notifications::catalog::ALWAYS_DELIVERED`), its rule cannot be edited, each recipient
//! acknowledges it, and these rows stay on each PC (never synchronised: row filters of
//! `sync::row_spec`). The start-up seed puts them back to these values
//! (`settings::received_notice::ensure_notice_rows`), with the same seeded ids.

use sea_orm::{DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261106_000282_di_safety_alert"
    }
}

const CATEGORY: &str = crate::settings::received_notice::DI_SAFETY_ALERT_CATEGORY;

/// The fixed audiences of the category (`notifications::catalog::ALWAYS_DELIVERED`), as stored.
fn audiences_json() -> String {
    let audiences: Vec<&str> = crate::notifications::catalog::ALWAYS_DELIVERED
        .iter()
        .filter(|(c, _)| *c == CATEGORY)
        .map(|(_, a)| *a)
        .collect();
    serde_json::to_string(&audiences).unwrap_or_else(|_| "[]".to_string())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, 'Request flagged for safety', 'warning', 0, 0, 'system', 1)",
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
             SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, ? \
             WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
            [
                seeded_entity_sync_id("notification_rules", CATEGORY).into(),
                CATEGORY.into(),
                audiences_json().into(),
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

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    #[tokio::test]
    async fn the_category_and_its_locked_rule_are_seeded() {
        let db = crate::db::test_db::migrated().await;
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT c.is_user_configurable, c.module_code, r.audiences_json, r.is_active \
                   FROM notification_categories c JOIN notification_rules r ON r.category_code = c.code \
                  WHERE c.code = 'di_safety_alert'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("category and rule seeded");
        assert_eq!(row.try_get::<i64>("", "is_user_configurable").unwrap(), 0);
        assert_eq!(row.try_get::<String>("", "module_code").unwrap(), "system");
        assert_eq!(row.try_get::<i64>("", "is_active").unwrap(), 1);
        assert_eq!(
            row.try_get::<String>("", "audiences_json").unwrap(),
            r#"["permission:di.review","permission:ptw.issue"]"#
        );
    }
}
