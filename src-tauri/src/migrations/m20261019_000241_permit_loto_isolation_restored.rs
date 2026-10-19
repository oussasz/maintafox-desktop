//! Migration 241 — the shipped LOTO permit type requires an energy isolation again, wherever a
//! switch-off received from another PC before migration 240 left it off.
//!
//! Before 240 a received switch-off of the LOTO type's energy-isolation rule was applied as
//! received; since 240 it is refused, here and on arrival, but a PC that had already applied one
//! kept the LOTO type off with no conflict and no warning, and its start gate then let LOTO permits
//! start work without isolation points. This step switches it back on, once
//! (`permit::queries::restore_shipped_loto_isolation`): only the shipped LOTO type (its fixed sync
//! identity or the code "loto", as the guard identifies it), only while it is off, audited as a
//! system change, with the rule's record saying the update did it (nobody, the time, a fixed
//! reason). The capture of changes is off while migrations run, so the correction is not sent:
//! every PC makes it itself when it upgrades, and on arrival that record is read as no change by
//! anyone (`permit::sync_check`), so rows of PCs that needed it and of PCs that did not pass both
//! ways.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000241_permit_loto_isolation_restored"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here (the change and its
        // audit event commit together).
        let txn = manager.get_connection().begin().await?;
        crate::permit::queries::restore_shipped_loto_isolation(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // A safety correction: never undone.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;
    use crate::permit::queries::{restore_shipped_loto_isolation, LOTO_ISOLATION_RESTORE_REASON};

    const NAME: &str = "m20261019_000241_permit_loto_isolation_restored";

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("text"))
    }

    /// A database migrated up to 240 (before this step).
    async fn before_241() -> DatabaseConnection {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 240");
        db
    }

    async fn type_state(
        db: &DatabaseConnection,
        code: &str,
    ) -> (String, Option<String>, Option<String>, Option<String>, String) {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT CAST(requires_energy_isolation AS TEXT) AS iso, \
                            CAST(isolation_changed_by_id AS TEXT) AS by_id, isolation_changed_at AS at, \
                            isolation_change_reason AS why, CAST(row_version AS TEXT) AS rv \
                     FROM permit_types WHERE code = '{code}'"
                ),
            ))
            .await
            .expect("query")
            .expect("type");
        (
            row.try_get("", "iso").expect("iso"),
            row.try_get("", "by_id").expect("by"),
            row.try_get("", "at").expect("at"),
            row.try_get("", "why").expect("why"),
            row.try_get("", "rv").expect("rv"),
        )
    }

    async fn restore_events(db: &DatabaseConnection) -> String {
        text(
            db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM audit_events \
             WHERE action_code = 'permit_type.energy_isolation.restored' AND auth_context = 'system' \
               AND actor_id IS NULL AND target_type = 'permit_type'",
        )
        .await
        .expect("count")
    }

    /// A LOTO type left off by a switch-off received before 240 is switched back on, audited, with
    /// the update's record; the other types are untouched; a second run changes nothing.
    #[tokio::test]
    async fn the_shipped_loto_type_requires_an_isolation_again() {
        let db = before_241().await;
        exec(
            &db,
            "UPDATE permit_types SET requires_energy_isolation = 0 WHERE code IN ('loto', 'hot_work')",
        )
        .await;
        let (_, _, _, _, rv_before) = type_state(&db, "loto").await;
        let hot_before = type_state(&db, "hot_work").await;
        Migrator::up(&db, None).await.expect("241");

        let (iso, by, at, why, rv) = type_state(&db, "loto").await;
        assert_eq!(iso, "1");
        assert_eq!(by, None, "the update, not a person");
        assert!(
            at.as_deref().and_then(crate::settings::site::instant).is_some(),
            "{at:?}"
        );
        assert_eq!(why.as_deref(), Some(LOTO_ISOLATION_RESTORE_REASON));
        assert_eq!(
            rv.parse::<i64>().expect("rv"),
            rv_before.parse::<i64>().expect("rv") + 1
        );
        assert_eq!(type_state(&db, "hot_work").await, hot_before, "only the LOTO type");
        assert_eq!(restore_events(&db).await, "1");
        let details = text(
            &db,
            "SELECT details_json FROM audit_events WHERE action_code = 'permit_type.energy_isolation.restored'",
        )
        .await
        .expect("details");
        assert!(details.contains("\"code\":\"loto\""), "{details}");

        // Idempotent: nothing more to do, nothing written.
        assert_eq!(restore_shipped_loto_isolation(&db).await.expect("again"), 0);
        assert_eq!(restore_events(&db).await, "1");
        assert_eq!(type_state(&db, "loto").await.4, rv);
    }

    /// A LOTO type that kept its rule on is left alone (no record, no event).
    #[tokio::test]
    async fn a_loto_type_still_on_is_left_alone() {
        let db = before_241().await;
        let before = type_state(&db, "loto").await;
        assert_eq!(before.0, "1");
        Migrator::up(&db, None).await.expect("241");
        assert_eq!(type_state(&db, "loto").await, before);
        assert_eq!(restore_events(&db).await, "0");
    }
}
