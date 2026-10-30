//! Notice "a phone was enrolled" (owner-approved 2026-10-29, admin-issued QR enrolment of
//! technician phones, `commands::mobile_enrolment`): category `mobile_phone_enrolled`, sent to the
//! holders of the user-administration right. Its rule is the plant's, like every other rule
//! (Données de référence → Notifications).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261029_000266_mobile_phone_enrolled_notice"
    }
}

const CATEGORY: &str = crate::commands::mobile_enrolment::ENROLMENT_NOTICE_CATEGORY;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
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
        let audiences = serde_json::json!([format!(
            "{}{}",
            crate::notifications::catalog::PERMISSION_AUDIENCE_PREFIX,
            crate::rbac::permissions::ADM_USERS
        )])
        .to_string();
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, ?, 'info', 0, 1, 'system', 1)",
            [
                seeded_entity_sync_id("notification_categories", CATEGORY).into(),
                CATEGORY.into(),
                "Phone enrolled".into(),
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
                audiences.into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
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
