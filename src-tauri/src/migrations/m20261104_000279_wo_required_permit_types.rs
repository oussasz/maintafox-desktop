//! Migration 279 — every permit type the plant's rules give a work order is required to start it
//! (ux-fix L4-10b, CMMS Advisor 2026-11-04; closes KNOWN_LIMITS C7-N1).
//!
//! `work_orders.permit_required_types_json` holds the SET of permit types the work order requires
//! (`permit::required_set`): one element per type, with the rule that gave it and its clearing.
//! The start and the resume need a live permit of every type not cleared (`permit::wo_gate`); a
//! type is lifted one at a time, with the permit-issue right at the work order's site and a reason.
//!
//! Backfill: a work order whose plant rule names a permit type gets a set of that one type, with the
//! clearing its rule carried (the person by sync id). Each PC derives the same from the same row
//! (migrations run with the capture triggers dropped: nothing is sent). A work order without a rule
//! keeps NULL, read as its legacy columns. The legacy columns (`permit_rule_code`,
//! `permit_rule_permit_type`, `permit_rule_cleared_*`) keep the primary rule, as before.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000279_wo_required_permit_types"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let exists = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM pragma_table_info('work_orders') WHERE name = 'permit_required_types_json'",
            ))
            .await?
            .is_some();
        if !exists {
            txn.execute_unprepared("ALTER TABLE work_orders ADD COLUMN permit_required_types_json TEXT NULL")
                .await?;
        }
        // The canonical form of `permit::required_set` (every key written, in that order).
        txn.execute_unprepared(
            "UPDATE work_orders SET permit_required_types_json = ( \
                 SELECT json_array(json_object( \
                     'permit_type_code', TRIM(work_orders.permit_rule_permit_type), \
                     'rule_code', TRIM(work_orders.permit_rule_code), \
                     'cleared_by_sync_id', CASE WHEN work_orders.permit_rule_cleared_at IS NOT NULL \
                                                THEN ua.entity_sync_id END, \
                     'cleared_at', work_orders.permit_rule_cleared_at, \
                     'clear_reason', CASE WHEN work_orders.permit_rule_cleared_at IS NOT NULL \
                                          THEN work_orders.permit_rule_clear_reason END)) \
                   FROM (SELECT 1) one \
                   LEFT JOIN user_accounts ua ON ua.id = work_orders.permit_rule_cleared_by_id) \
              WHERE permit_required_types_json IS NULL \
                AND TRIM(COALESCE(permit_rule_code, '')) <> '' \
                AND TRIM(COALESCE(permit_rule_permit_type, '')) <> ''",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The permits a work order requires are safety history: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn before_279() -> DatabaseConnection {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261104_000279_wo_required_permit_types")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 278");
        db
    }

    async fn set_of(db: &DatabaseConnection, code: &str) -> Option<String> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT permit_required_types_json AS j FROM work_orders WHERE code = ?",
            [code.into()],
        ))
        .await
        .expect("read")
        .and_then(|r| r.try_get::<Option<String>>("", "j").expect("j"))
    }

    /// A work order with a rule gets a set of its one type, with the clearing it carried (by sync
    /// id); one without a rule keeps NULL. The set reads as `permit::required_set` writes it.
    #[tokio::test]
    async fn the_rule_of_each_work_order_becomes_its_set() {
        let db = before_279().await;
        exec(
            &db,
            "INSERT INTO user_accounts (id, entity_sync_id, username, identity_mode, password_hash, is_active, \
                 is_admin, force_password_change, failed_login_attempts, created_at, updated_at, row_version) \
             VALUES (9001, 'user-sync-279', 'issuer279', 'local', 'x', 1, 0, 0, 0, '2026-01-01T00:00:00Z', \
                 '2026-01-01T00:00:00Z', 1)",
        )
        .await;
        for (code, rule, permit_type, cleared) in [
            ("OT-279-A", Some("CRITICITE_A"), Some("loto"), false),
            ("OT-279-B", Some("SOUDURE"), Some("hot_work"), true),
            ("OT-279-C", None, None, false),
        ] {
            exec(
                &db,
                &format!(
                    "INSERT INTO work_orders (code, type_id, status_id, title, requires_permit, permit_rule_code, \
                         permit_rule_permit_type, permit_rule_cleared_by_id, permit_rule_cleared_at, \
                         permit_rule_clear_reason, row_version, created_at, updated_at) \
                     SELECT '{code}', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
                              JOIN reference_domains d ON d.id = s.domain_id \
                             WHERE d.code = 'WORK.ORDER_TYPES' ORDER BY v.id LIMIT 1), \
                            (SELECT id FROM work_order_statuses WHERE code = 'draft'), 'T', {req}, {rule}, {pt}, \
                            {by}, {at}, {why}, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'",
                    req = i32::from(!cleared),
                    rule = rule.map_or("NULL".into(), |r| format!("'{r}'")),
                    pt = permit_type.map_or("NULL".into(), |t| format!("'{t}'")),
                    by = if cleared { "9001" } else { "NULL" },
                    at = if cleared { "'2026-11-01T08:00:00Z'" } else { "NULL" },
                    why = if cleared { "'travail sans risque'" } else { "NULL" },
                ),
            )
            .await;
        }
        Migrator::up(&db, None).await.expect("279");
        assert_eq!(
            set_of(&db, "OT-279-A").await.as_deref(),
            Some(
                r#"[{"permit_type_code":"loto","rule_code":"CRITICITE_A","cleared_by_sync_id":null,"cleared_at":null,"clear_reason":null}]"#
            )
        );
        let b = set_of(&db, "OT-279-B").await.expect("set");
        let parsed = crate::permit::required_set::parse(&b).expect("reads");
        let clearing = parsed[0].clearing.clone().expect("cleared");
        assert_eq!(clearing.by_sync_id.as_deref(), Some("user-sync-279"));
        assert_eq!(clearing.at, "2026-11-01T08:00:00Z");
        assert_eq!(clearing.reason.as_deref(), Some("travail sans risque"));
        assert_eq!(set_of(&db, "OT-279-C").await, None);
    }
}
