//! Migration 200 — owner decisions of 2026-10-15 (answers c–e).
//!
//! **PM follow-up work orders are deferred corrective work (EN 13306).** A fault found during
//! preventive work is repaired as corrective maintenance, so the OT counts as a failure found by
//! PM. New follow-up OTs get the deferred corrective type (`pm::queries`). Existing ones are
//! re-tagged only when the PM link identifies them without ambiguity:
//! - the OT is the follow-up of exactly one PM finding (`pm_findings.follow_up_work_order_id`);
//! - it is not also the OT of a PM occurrence or execution (`pm_occurrences.linked_work_order_id`,
//!   `pm_executions.work_order_id`);
//! - it still has the type the former code gave every follow-up, the shipped `preventive` type
//!   (the screens never offered another type for a follow-up).
//!
//! Such an OT gets the shipped type of deferred corrective work (`corrective`) and loses its
//! sub-type, which belonged to the preventive nature. Any other OT is left as it is.
//!
//! **Request types without an OT nature.** The shipped request types that name no kind of work
//! (repair, observation, other) carried the placeholder tag `di_request_type`; the tag of a
//! request type is now the EN 13306 nature of the OT it becomes, checked on the server, so the
//! placeholder becomes empty (corrective work, as before).
//!
//! **OT sub-types are governed through draft → publish** (`WORK.ORDER_SUBTYPES`, category C).
//!
//! One transaction; every part is guarded and can run again without changing anything.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000200_pm_follow_up_corrective_and_governed_lists"
    }
}

/// Id of the shipped OT type `code` (`WORK.ORDER_TYPES`), if present.
async fn type_id<C: ConnectionTrait>(db: &C, code: &str) -> Result<Option<i64>, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id AND s.status = 'published' \
           JOIN reference_domains d ON d.id = s.domain_id \
          WHERE UPPER(TRIM(d.code)) = 'WORK.ORDER_TYPES' AND v.code = ? \
          ORDER BY s.version_no DESC LIMIT 1",
        [code.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id"))
    .transpose()
}

/// Re-tags the unambiguous PM follow-up OTs as deferred corrective work. Returns how many.
pub(crate) async fn retag_pm_follow_ups<C: ConnectionTrait>(db: &C) -> Result<u64, DbErr> {
    let (Some(preventive), Some(corrective)) = (type_id(db, "preventive").await?, type_id(db, "corrective").await?)
    else {
        return Ok(0);
    };
    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE work_orders SET type_id = ?, subtype_id = NULL \
              WHERE type_id = ? \
                AND id IN (SELECT pf.follow_up_work_order_id FROM pm_findings pf \
                            WHERE pf.follow_up_work_order_id IS NOT NULL \
                            GROUP BY pf.follow_up_work_order_id HAVING COUNT(*) = 1) \
                AND id NOT IN (SELECT po.linked_work_order_id FROM pm_occurrences po \
                                WHERE po.linked_work_order_id IS NOT NULL) \
                AND id NOT IN (SELECT pe.work_order_id FROM pm_executions pe WHERE pe.work_order_id IS NOT NULL)",
            [corrective.into(), preventive.into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

async fn run<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let retagged = retag_pm_follow_ups(db).await?;
    if retagged > 0 {
        tracing::info!(
            retagged,
            "migration 200: PM follow-up work orders re-tagged as deferred corrective"
        );
    }

    db.execute_unprepared(
        "UPDATE reference_values SET semantic_tag = NULL \
          WHERE semantic_tag = 'di_request_type' \
            AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
                            WHERE UPPER(TRIM(d.code)) = 'DI.REQUEST_TYPE')",
    )
    .await?;

    db.execute_unprepared(
        "UPDATE reference_domains SET governance_category = 'controlled_catalog' \
          WHERE UPPER(TRIM(code)) = 'WORK.ORDER_SUBTYPES'",
    )
    .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        run(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Re-tagged work orders cannot be told apart afterwards: not reversible.
        Ok(())
    }
}
