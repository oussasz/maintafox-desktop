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
//! - `failure_events.symptom_id` (new, references `reference_values(id)`) records the observed
//!   symptom on each RAMS failure event; existing events created from a work order take it from
//!   the work order's close-out coding.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
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

async fn work_symptom_id<C: ConnectionTrait>(db: &C, code: &str) -> Result<Option<i64>, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT rv.id FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND LOWER(TRIM(rv.code)) = LOWER(TRIM(?)) \
              ORDER BY CASE rs.status WHEN 'published' THEN 0 ELSE 1 END, rs.version_no DESC LIMIT 1",
        [DOMAIN_SYMPTOMS.into(), code.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id"))
    .transpose()
}

async fn merge_request_symptoms<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let target_set: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
            [DOMAIN_SYMPTOMS.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom(format!("{DOMAIN_SYMPTOMS} has no published set")))?
        .try_get("", "id")?;

    // Published set first, so a plant value present in several versions keeps its published row.
    let values = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.id, rv.code FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? \
              ORDER BY CASE rs.status WHEN 'published' THEN 0 ELSE 1 END, rs.version_no DESC, rv.id",
            [RETIRED_SYMPTOM_DOMAIN.into()],
        ))
        .await?;

    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for v in values {
        let id: i64 = v.try_get("", "id")?;
        let code: String = v.try_get("", "code")?;
        match work_symptom_id(db, &code).await? {
            Some(target) => pairs.push((id, target)),
            None => {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE reference_values SET set_id = ?, parent_id = NULL WHERE id = ?",
                    [target_set.into(), id.into()],
                ))
                .await?;
            }
        }
    }

    crate::reference::retirement::remap_value_links(db, &pairs)
        .await
        .map_err(db_err)?;
    crate::reference::retirement::delete_domain(db, RETIRED_SYMPTOM_DOMAIN)
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
        let db = manager.get_connection();
        ensure_closeout_reference_catalogs(db).await.map_err(db_err)?;
        merge_request_symptoms(db).await?;
        add_failure_event_symptom(db).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Lists merged and links moved: not reversible.
        Ok(())
    }
}
