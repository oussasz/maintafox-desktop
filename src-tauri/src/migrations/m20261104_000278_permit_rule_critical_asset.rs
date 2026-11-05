//! Migration 278 — work on a critical machine needs a consignation permit (ux-fix L4-10, CMMS
//! Advisor decision L4 Q7).
//!
//! The plant's permit rules (`PERMIT.REQUIREMENT_RULES`, `permit::requirement_rules`) can now name
//! an equipment criticality. This step ships one rule, `CRITICITE_A`: every work order on an
//! equipment of criticality A requires the `loto` permit type (else the first type that requires an
//! energy isolation). The work order is flagged when it is created and its open work orders when the
//! equipment becomes critical; the flag is protected like any rule flag: the general edit cannot
//! untick it, clearing it needs the permit-issue right at the work order's site and a reason of 10
//! characters or more, and is audited (`wo::workflow::actions::permit_requirement`).
//!
//! The rule is a normal value of the list, visible in Données de référence. Seeded once, here
//! (`permit::requirement_rules::seed_critical_asset_rule`): never over a value with its identity or
//! its code, so a rule the plant changed or removed later is not brought back. Work orders already
//! open get it from the start-up catch-up (`permit::requirement_rules::catch_up`), each PC deriving
//! the same from the same list. The seeded value is written with change capture suspended: each PC
//! makes it with the same identity, so nothing is sent.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000278_permit_rule_critical_asset"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        // Made by this migration on every PC with the same identity: nothing is sent.
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
        crate::permit::requirement_rules::seed_critical_asset_rule(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // A safety rule the plant may have edited since: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;
    use crate::permit::requirement_rules::{parse_rule, seed_critical_asset_rule, CRITICAL_ASSET_RULE_CODE, DOMAIN};

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    /// The shipped rule's (is_active, metadata_json), when it is in the list.
    async fn shipped_rule(db: &DatabaseConnection) -> Option<(i64, String)> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.is_active, rv.metadata_json FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE d.code = ? AND rv.code = ?",
            [DOMAIN.into(), CRITICAL_ASSET_RULE_CODE.into()],
        ))
        .await
        .expect("read")
        .map(|r| {
            (
                r.try_get::<i64>("", "is_active").expect("active"),
                r.try_get::<String>("", "metadata_json").expect("meta"),
            )
        })
    }

    /// A database migrated up to the step before this one.
    async fn before_278() -> DatabaseConnection {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == "m20261104_000278_permit_rule_critical_asset")
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 277");
        db
    }

    /// The list holds the rule "criticality A → loto", active and readable.
    #[tokio::test]
    async fn the_list_gets_the_critical_asset_rule() {
        let db = before_278().await;
        let journal = |db: &DatabaseConnection| {
            let db = db.clone();
            async move {
                db.query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT COUNT(*) AS n FROM sync_capture_journal".to_string(),
                ))
                .await
                .expect("journal")
                .map_or(0, |r| r.try_get::<i64>("", "n").expect("n"))
            }
        };
        let sent_before = journal(&db).await;
        Migrator::up(&db, None).await.expect("278");
        // Made on every PC with the same identity: nothing is sent.
        assert_eq!(journal(&db).await, sent_before, "nothing is sent");
        let (active, meta) = shipped_rule(&db).await.expect("seeded");
        assert_eq!(active, 1);
        let rule = parse_rule(Some(&meta)).expect("reads");
        assert_eq!(rule.criticality_code.as_deref(), Some("A"));
        assert_eq!(rule.permit_type_code, "loto");
        assert!(rule.equipment_class_code.is_none() && rule.work_type_code.is_none());
    }

    /// Seeding again never brings back a rule the plant deactivated, nor writes over it.
    #[tokio::test]
    async fn seeding_again_keeps_the_plants_choice() {
        let db = before_278().await;
        Migrator::up(&db, None).await.expect("278");
        exec(
            &db,
            &format!("UPDATE reference_values SET is_active = 0 WHERE code = '{CRITICAL_ASSET_RULE_CODE}'"),
        )
        .await;
        assert!(!seed_critical_asset_rule(&db).await.expect("seed"));
        assert_eq!(shipped_rule(&db).await.expect("kept").0, 0);
    }

    /// Without the `loto` type, the first type requiring an isolation is named; with none, nothing.
    #[tokio::test]
    async fn the_rule_names_an_isolation_type_the_plant_has() {
        let db = before_278().await;
        exec(&db, "UPDATE permit_types SET code = 'consignation' WHERE code = 'loto'").await;
        Migrator::up(&db, None).await.expect("278");
        let (_, meta) = shipped_rule(&db).await.expect("seeded");
        assert_eq!(parse_rule(Some(&meta)).expect("reads").permit_type_code, "consignation");

        let empty = before_278().await;
        exec(&empty, "UPDATE permit_types SET requires_energy_isolation = 0").await;
        Migrator::up(&empty, None).await.expect("278");
        assert!(shipped_rule(&empty).await.is_none());
    }
}
