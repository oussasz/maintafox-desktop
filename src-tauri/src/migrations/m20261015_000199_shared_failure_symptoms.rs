//! Migration 199 — one failure-symptom list for requests, work orders and RAMS (ISO 14224).
//!
//! Before: two governed lists, `DI.SYMPTOM` (request intake: 5 values) and `WORK.SYMPTOMS` (OT
//! close-out: the ISO 14224 failure-mode codes observed on the equipment, with the five request
//! codes among them). After: `WORK.SYMPTOMS` alone, a COMPANY list (`wo::closeout_reference_catalog`)
//! seeded with the ISO 14224 observed failure modes (code, French label, English term, ISO code).
//!
//! - Every `DI.SYMPTOM` value whose code exists in `WORK.SYMPTOMS` (vibration, leakage,
//!   overheating, noise, performance_drop…) has its links moved to that value
//!   (`intervention_requests.symptom_code_id`, `work_order_failure_details.symptom_id` and every
//!   other column that references it, see `reference::retirement`).
//! - A `DI.SYMPTOM` value a plant added (code absent from `WORK.SYMPTOMS`) is moved, with its id,
//!   identity and links, into the published `WORK.SYMPTOMS` set.
//! - `DI.SYMPTOM` is then deleted. A link that could not be moved stops the migration.
//!   (`reference::retirement::merge_domain_into`; the startup retirement pass runs the same merge
//!   again, with sync capture on, if a replayed history brings the list back.)
//! - The whole step runs in one transaction and every part is guarded, so it can run again.
//! - `failure_events.symptom_id` (new, references `reference_values(id)`) records the observed
//!   symptom on each RAMS failure event; existing events created from a work order take it from
//!   the work order's close-out coding.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::wo::closeout_reference_catalog::{ensure_closeout_reference_catalogs, DOMAIN_SYMPTOMS};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000199_shared_failure_symptoms"
    }
}

pub const RETIRED_SYMPTOM_DOMAIN: &str = "DI.SYMPTOM";

fn db_err(e: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(e.to_string())
}

async fn merge_request_symptoms<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    crate::reference::retirement::merge_domain_into(db, RETIRED_SYMPTOM_DOMAIN, DOMAIN_SYMPTOMS)
        .await
        .map_err(db_err)?;
    Ok(())
}

async fn add_failure_event_symptom<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let has_column = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info('failure_events') WHERE name = 'symptom_id'".to_string(),
        ))
        .await?
        .is_some();
    if !has_column {
        db.execute_unprepared(
            "ALTER TABLE failure_events ADD COLUMN symptom_id INTEGER NULL REFERENCES reference_values(id)",
        )
        .await?;
    }
    db.execute_unprepared(
        "UPDATE failure_events \
            SET symptom_id = ( \
                SELECT fd.symptom_id FROM work_order_failure_details fd \
                 WHERE fd.work_order_id = failure_events.source_id \
                 ORDER BY fd.id DESC LIMIT 1) \
          WHERE source_type = 'work_order' AND symptom_id IS NULL",
    )
    .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration runs SQLite migrations outside a transaction: one transaction here, so
        // a failure leaves the database as it was and the next start runs the whole step again.
        // Every step is also guarded (no retired list / column already there: nothing to do).
        let txn = manager.get_connection().begin().await?;
        ensure_closeout_reference_catalogs(&txn).await.map_err(db_err)?;
        merge_request_symptoms(&txn).await?;
        add_failure_event_symptom(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Lists merged and links moved: not reversible.
        Ok(())
    }
}
