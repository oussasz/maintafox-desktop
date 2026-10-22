//! Two PCs that number a version of the same PM plan alike (0.9.0 test finding 4, rule 1,
//! CMMS Advisor decision 2026-10-22: only the display number changes).
//!
//! A version is numbered on the PC that creates it (`MAX + 1` of the plan there). Two PCs that each
//! add a version to a plan before receiving the other's give both the same number. The stored
//! number is kept exactly as its PC set it, on every PC (nothing is moved or sent); two versions of
//! a plan may share it (migration 254). The number shown everywhere a version is listed or named is
//! its display number (`pm_plan_version_numbers`, migration 254): computed from the versions of the
//! plan by (stored number, sync id), so every PC holding the same versions shows the same distinct
//! numbers. The plan's current version is the one it points to (published = current); nothing here
//! changes it, and only that version drives scheduling (`pm::generation`).
//!
//! The PM planners (`pm.edit`) of each PC are told, on a locked notice they acknowledge
//! (`settings::received_notice::PM_CONCURRENT_VERSIONS_CATEGORY`), when a received version shares
//! its stored number with another version of the plan ([`after_received_version`]), and when a
//! received occurrence has the same plan, asset and due date as an occurrence of another version
//! ([`after_received_occurrence`]): both are kept, nothing is cancelled.
//!
//! A received version refused before this update on the unique number (an open `MATERIALIZE_ERROR`
//! conflict with that database text) is applied again at the next synchronisation, before the rows
//! waiting for it are retried ([`retry_refused_number_clashes`]): its conflict is closed when it
//! applies; when it still cannot be applied, the new error is recorded in place of the old one (the
//! conflict stays open with its real reason, and is not tried again by this pass).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::NotificationMessage;
use crate::sync::materialize::ApplyOutcome;

/// The database text of the unique number that refused a version before this update.
const VERSION_NUMBER_TAKEN: &str = "UNIQUE constraint failed: pm_plan_versions.pm_plan_id, pm_plan_versions.version_no";

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("pm version clash: {e}"))
}

/// Received versions refused before this update because their number was taken here: each is
/// applied again once (see the module documentation). Returns how many applied.
pub(crate) async fn retry_refused_number_clashes(db: &DatabaseConnection) -> AppResult<i64> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT i.id AS inbox_id, c.id AS conflict_id
             FROM sync_inbox i JOIN sync_conflicts c ON c.linked_inbox_id = i.id
             WHERE i.entity_type = 'pm_plan_versions' AND i.apply_status = 'conflict'
               AND i.rejection_code = 'MATERIALIZE_ERROR' AND instr(i.rejection_message, ?) > 0
               AND c.conflict_type = 'MATERIALIZE_ERROR' AND c.status IN ('new', 'triaged', 'escalated')
             ORDER BY i.id",
            [VERSION_NUMBER_TAKEN.into()],
        ))
        .await?;
    let mut applied = 0;
    for row in rows {
        let inbox_id: i64 = row.try_get("", "inbox_id").map_err(err)?;
        let conflict_id: i64 = row.try_get("", "conflict_id").map_err(err)?;
        let Some(item) = crate::sync::materialize::load_inbox_row(db, inbox_id).await? else {
            continue;
        };
        let tx = db.begin().await?;
        crate::sync::capture::drain_capture_journal(&tx).await?;
        let outcome = crate::sync::materialize::apply_inbox_row_in(&tx, &item).await;
        crate::sync::row_codec::clear_ref_cache();
        let still = match outcome {
            Ok(ApplyOutcome::Applied | ApplyOutcome::Unchanged) => {
                tx.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_conflicts
                     SET status = 'resolved_remote', resolution_action = 'applied_after_update',
                         resolution_note = 'Applied after the update: two versions of a plan may now share a number.',
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE id = ?",
                    [conflict_id.into()],
                ))
                .await?;
                tx.commit().await?;
                applied += 1;
                continue;
            }
            Ok(ApplyOutcome::Deferred { message, .. } | ApplyOutcome::Conflict { message, .. }) => message,
            Err(e) => e.to_string(),
        };
        tx.rollback().await?;
        // Still not applicable: its real reason now, in place of the number refused before.
        let tx = db.begin().await?;
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_inbox SET rejection_message = ?, last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [still.clone().into(), inbox_id.into()],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_conflicts SET detail_message = ?, row_version = row_version + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [still.into(), conflict_id.into()],
        ))
        .await?;
        tx.commit().await?;
    }
    Ok(applied)
}

/// A version of a plan as the notice names it.
struct NamedVersion {
    sync_id: String,
    shown: i64,
    stored: i64,
    status: String,
    created_at: String,
}

async fn named_version(conn: &impl ConnectionTrait, version_id: i64) -> AppResult<Option<NamedVersion>> {
    conn.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT v.entity_sync_id, n.display_no, v.version_no, v.status, v.created_at
         FROM pm_plan_versions v JOIN pm_plan_version_numbers n ON n.id = v.id WHERE v.id = ?",
        [version_id.into()],
    ))
    .await?
    .map(|r| -> AppResult<NamedVersion> {
        Ok(NamedVersion {
            sync_id: r
                .try_get::<Option<String>>("", "entity_sync_id")
                .map_err(err)?
                .unwrap_or_default(),
            shown: r.try_get("", "display_no").map_err(err)?,
            stored: r.try_get("", "version_no").map_err(err)?,
            status: r.try_get("", "status").map_err(err)?,
            created_at: r.try_get("", "created_at").map_err(err)?,
        })
    })
    .transpose()
}

/// Code, title and sync id of a plan.
async fn plan_names(conn: &impl ConnectionTrait, plan_id: i64) -> AppResult<(String, String, String)> {
    let r = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT code, title, entity_sync_id FROM pm_plans WHERE id = ?",
            [plan_id.into()],
        ))
        .await?
        .ok_or_else(|| err(format!("plan {plan_id} not found")))?;
    Ok((
        r.try_get("", "code").map_err(err)?,
        r.try_get("", "title").map_err(err)?,
        r.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(err)?
            .unwrap_or_default(),
    ))
}

/// After a received version is written here (CMMS Advisor decision a): when another version of its
/// plan carries the same stored number, the PM planners are told, naming both versions with the
/// numbers shown, their status and creation dates (no author: none is recorded for a version).
pub(crate) async fn after_received_version(conn: &impl ConnectionTrait, version_id: i64) -> AppResult<()> {
    let others = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT o.id FROM pm_plan_versions v JOIN pm_plan_versions o
               ON o.pm_plan_id = v.pm_plan_id AND o.version_no = v.version_no AND o.id <> v.id
             WHERE v.id = ? ORDER BY o.entity_sync_id",
            [version_id.into()],
        ))
        .await?;
    let Some(first_other) = others.first() else {
        return Ok(());
    };
    let other_id: i64 = first_other.try_get("", "id").map_err(err)?;
    let (Some(received), Some(other)) = (
        named_version(conn, version_id).await?,
        named_version(conn, other_id).await?,
    ) else {
        return Ok(());
    };
    let plan_id = scalar_plan_of(conn, version_id).await?;
    let (code, title, plan_sync_id) = plan_names(conn, plan_id).await?;
    let (v, o) = (received.shown, other.shown);
    let stored = received.stored;
    crate::settings::received_notice::send_pm_concurrent_notice(
        conn,
        "pm_plan_versions",
        &received.sync_id,
        &plan_sync_id,
        format!(
            "Deux versions concurrentes du plan {code} — vérifier la version en vigueur / Two concurrent versions of \
             plan {code} — check the version in force"
        ),
        format!(
            "{title} : v{v} ({}, créée le {}) et v{o} ({}, créée le {}) ont été créées sur deux PC avant leur \
             synchronisation, toutes deux numérotées {stored} ; elles sont affichées v{v} et v{o} sur chaque PC. / \
             {title}: v{v} ({}, created {}) and v{o} ({}, created {}) were created on two PCs before they synchronised, \
             both numbered {stored}; every PC shows them as v{v} and v{o}.",
            received.status,
            received.created_at,
            other.status,
            other.created_at,
            received.status,
            received.created_at,
            other.status,
            other.created_at,
        ),
        NotificationMessage::new(
            "pm_concurrent_versions",
            json!({
                "plan": code,
                "title": title,
                "version": v,
                "version_status": received.status,
                "version_created_at": received.created_at,
                "other_version": o,
                "other_status": other.status,
                "other_created_at": other.created_at,
                "stored_number": stored,
                "others": others.len(),
            }),
        ),
    )
    .await
}

async fn scalar_plan_of(conn: &impl ConnectionTrait, version_id: i64) -> AppResult<i64> {
    conn.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT pm_plan_id FROM pm_plan_versions WHERE id = ?",
        [version_id.into()],
    ))
    .await?
    .ok_or_else(|| err(format!("version {version_id} not found")))?
    .try_get("", "pm_plan_id")
    .map_err(err)
}

/// After a received occurrence is written here (CMMS Advisor decision c): when another version of
/// the same plan already has an occurrence for the same asset and due date, the PM planners are told
/// on the same notice category, naming both with their work orders. Nothing is cancelled.
pub(crate) async fn after_received_occurrence(conn: &impl ConnectionTrait, occurrence_id: i64) -> AppResult<()> {
    let twin = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT o.id, o.plan_version_id AS other_version_id, ow.code AS other_wo,
                    r.plan_version_id AS version_id, rw.code AS wo, r.pm_plan_id, r.entity_sync_id,
                    r.due_at, r.due_basis
             FROM pm_occurrences r
             JOIN pm_occurrences o ON o.pm_plan_id = r.pm_plan_id AND o.id <> r.id
                  AND o.plan_version_id <> r.plan_version_id AND o.equipment_id IS r.equipment_id
                  AND ((r.due_at IS NOT NULL AND o.due_at = r.due_at)
                       OR (r.due_at IS NULL AND o.due_at IS NULL AND o.due_basis = r.due_basis))
             LEFT JOIN work_orders rw ON rw.id = r.linked_work_order_id
             LEFT JOIN work_orders ow ON ow.id = o.linked_work_order_id
             WHERE r.id = ? ORDER BY o.id LIMIT 1",
            [occurrence_id.into()],
        ))
        .await?;
    let Some(t) = twin else {
        return Ok(());
    };
    let version_id: i64 = t.try_get("", "version_id").map_err(err)?;
    let other_version_id: i64 = t.try_get("", "other_version_id").map_err(err)?;
    let plan_id: i64 = t.try_get("", "pm_plan_id").map_err(err)?;
    let occurrence_sync_id: String = t
        .try_get::<Option<String>>("", "entity_sync_id")
        .map_err(err)?
        .unwrap_or_default();
    let due_at: Option<String> = t.try_get("", "due_at").map_err(err)?;
    let due_basis: Option<String> = t.try_get("", "due_basis").map_err(err)?;
    let wo: Option<String> = t.try_get("", "wo").map_err(err)?;
    let other_wo: Option<String> = t.try_get("", "other_wo").map_err(err)?;
    let (Some(version), Some(other)) = (
        named_version(conn, version_id).await?,
        named_version(conn, other_version_id).await?,
    ) else {
        return Ok(());
    };
    let (code, title, plan_sync_id) = plan_names(conn, plan_id).await?;
    let due_text = due_at
        .clone()
        .or_else(|| due_basis.clone())
        .unwrap_or_else(|| "?".to_string());
    let wo_text = |w: &Option<String>| w.clone().unwrap_or_else(|| "—".to_string());
    let (v, o) = (version.shown, other.shown);
    crate::settings::received_notice::send_pm_concurrent_notice(
        conn,
        "pm_occurrences",
        &occurrence_sync_id,
        &plan_sync_id,
        format!(
            "Deux versions concurrentes du plan {code} — échéance en double le {due_text} / Two concurrent versions \
             of plan {code} — due date {due_text} twice"
        ),
        format!(
            "{title} : les versions v{v} et v{o} ont chacune une échéance le {due_text} (OT {} et {}). Rien n'a été \
             annulé : gardez-en une. / {title}: versions v{v} and v{o} each have an occurrence due {due_text} (work \
             orders {} and {}). Nothing was cancelled: keep one.",
            wo_text(&wo),
            wo_text(&other_wo),
            wo_text(&wo),
            wo_text(&other_wo),
        ),
        NotificationMessage::new(
            "pm_duplicate_occurrence",
            json!({
                "plan": code,
                "title": title,
                "due_at": due_at,
                "due_basis": due_basis,
                "version": v,
                "other_version": o,
                "work_order": wo,
                "other_work_order": other_wo,
            }),
        ),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::domain::{
        ApplySyncBatchInput, SyncConflictFilter, SyncInboundItemInput, SYNC_PROTOCOL_VERSION_V2,
    };
    use crate::sync::queries::{apply_sync_batch, list_sync_conflicts, record_sync_conflict, NewSyncConflict};
    use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

    async fn plan_id(db: &DatabaseConnection, plan_sync_id: &str) -> i64 {
        scalar_i64(
            db,
            "SELECT id FROM pm_plans WHERE entity_sync_id = ?",
            vec![plan_sync_id.into()],
        )
        .await
    }

    /// A draft version created on this PC (captured like any local change).
    async fn add_version(db: &DatabaseConnection, plan_sync_id: &str, version_sync_id: &str, number: i64) {
        let plan = plan_id(db, plan_sync_id).await;
        exec(
            db,
            "INSERT INTO pm_plan_versions (entity_sync_id, pm_plan_id, version_no, status, effective_from,
                                           trigger_definition_json, change_reason)
             VALUES (?, ?, ?, 'draft', '2026-11-01', '{}', 'Concurrent revision')",
            vec![version_sync_id.into(), plan.into(), number.into()],
        )
        .await;
    }

    async fn stored_number(db: &DatabaseConnection, version_sync_id: &str) -> Option<i64> {
        scalar_opt_text(
            db,
            "SELECT CAST(version_no AS TEXT) FROM pm_plan_versions WHERE entity_sync_id = ?",
            vec![version_sync_id.into()],
        )
        .await
        .and_then(|n| n.parse().ok())
    }

    /// `(sync id, number shown)` of the versions of a plan, as the PM screens list them.
    async fn shown_numbers(db: &DatabaseConnection, plan_sync_id: &str) -> Vec<(String, i64)> {
        let plan = plan_id(db, plan_sync_id).await;
        let mut shown = Vec::new();
        for v in crate::pm::queries::list_pm_plan_versions(db, plan)
            .await
            .expect("versions")
        {
            let sync_id = scalar_opt_text(
                db,
                "SELECT entity_sync_id FROM pm_plan_versions WHERE id = ?",
                vec![v.id.into()],
            )
            .await
            .expect("sync id");
            shown.push((sync_id, v.version_no));
        }
        shown.sort();
        shown
    }

    async fn open_conflicts(db: &DatabaseConnection) -> usize {
        list_sync_conflicts(
            db,
            SyncConflictFilter {
                statuses: Some(vec!["new".into(), "triaged".into(), "escalated".into()]),
                ..SyncConflictFilter::default()
            },
        )
        .await
        .expect("conflicts")
        .len()
    }

    async fn inbox_id(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> i64 {
        scalar_i64(
            db,
            "SELECT id FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ?",
            vec![entity_type.into(), sync_id.into()],
        )
        .await
    }

    /// A plan known to A, B and C; then the versions `(pc, sync id, stored number)` are created
    /// offline, every PC synchronises twice in turn, and the versions are compared.
    async fn concurrent_versions(versions: &[(&str, &str, i64)]) -> Vec<(String, i64)> {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let c = started_device_db().await;
        let pcs = [("A", &a), ("B", &b), ("C", &c)];
        let plan = RowFactory::for_device(1).create(&a, "pm_plans").await.expect("plan");
        for (name, db) in pcs {
            server.exchange(name, db).await;
        }
        for (pc, sync_id, number) in versions {
            let db = pcs.iter().find(|(n, _)| n == pc).expect("pc").1;
            add_version(db, &plan.entity_sync_id, sync_id, *number).await;
        }
        for _ in 0..2 {
            for (name, db) in pcs {
                let report = server.exchange(name, db).await;
                assert_eq!(report.materialized.conflicts, 0, "{name}: {report:?}");
                assert_eq!(report.conflicts, 0, "{name}: {report:?}");
            }
        }
        let reference = shown_numbers(&a, &plan.entity_sync_id).await;
        for (name, db) in pcs {
            assert_eq!(open_conflicts(db).await, 0, "{name}");
            assert_eq!(
                shown_numbers(db, &plan.entity_sync_id).await,
                reference,
                "{name} shows the same numbers"
            );
            // The PM planners of every PC are told when two versions share a stored number.
            let mut stored: Vec<i64> = versions.iter().map(|(_, _, n)| *n).collect();
            stored.sort_unstable();
            let tie = stored.windows(2).any(|w| w[0] == w[1]);
            let (events, _) = notices(db, "pm_concurrent_versions").await;
            assert_eq!(events > 0, tie, "{name}: {events} notice(s)");
            // Nothing was moved: every version keeps the number its PC gave it.
            for (_, sync_id, number) in versions {
                assert_eq!(stored_number(db, sync_id).await, Some(*number), "{name}: {sync_id}");
            }
        }
        let mut shown: Vec<i64> = reference.iter().map(|(_, n)| *n).collect();
        shown.sort_unstable();
        shown.dedup();
        assert_eq!(shown.len(), versions.len(), "distinct numbers shown: {reference:?}");
        reference
    }

    const A1: &str = "1aaaaaaa-0000-4000-8000-000000000001";
    const A2: &str = "1aaaaaaa-0000-4000-8000-000000000002";
    const B1: &str = "2bbbbbbb-0000-4000-8000-000000000001";
    const B2: &str = "2bbbbbbb-0000-4000-8000-000000000002";

    #[tokio::test]
    async fn two_pcs_with_two_versions_each_show_the_same_numbers_when_the_first_holds_the_smaller_ids() {
        let shown = concurrent_versions(&[("A", A1, 1), ("A", A2, 2), ("B", B1, 1), ("B", B2, 2)]).await;
        // By (stored number, sync id): A1 1, B1 2, A2 3, B2 4.
        assert_eq!(
            shown,
            vec![(A1.into(), 1), (A2.into(), 3), (B1.into(), 2), (B2.into(), 4)]
        );
    }

    #[tokio::test]
    async fn two_pcs_with_two_versions_each_show_the_same_numbers_when_the_first_holds_the_greater_ids() {
        let shown = concurrent_versions(&[("A", B1, 1), ("A", B2, 2), ("B", A1, 1), ("B", A2, 2)]).await;
        assert_eq!(
            shown,
            vec![(A1.into(), 1), (A2.into(), 3), (B1.into(), 2), (B2.into(), 4)]
        );
    }

    #[tokio::test]
    async fn the_reviewers_sequence_shows_the_same_numbers_everywhere() {
        // vA < vB < vB2: B makes vB = 1 and vB2 = 2 offline, A makes vA = 1.
        let (va, vb, vb2) = (A1, B1, B2);
        let shown = concurrent_versions(&[("B", vb, 1), ("B", vb2, 2), ("A", va, 1)]).await;
        assert_eq!(shown, vec![(va.into(), 1), (vb.into(), 2), (vb2.into(), 3)]);
    }

    #[tokio::test]
    async fn without_equal_numbers_the_stored_numbers_are_shown_gaps_included() {
        let shown = concurrent_versions(&[("A", B1, 1), ("B", A2, 3)]).await;
        assert_eq!(shown, vec![(A2.into(), 3), (B1.into(), 1)]);
    }

    #[tokio::test]
    async fn a_version_refused_before_the_update_is_applied_with_the_rows_waiting_for_it() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let occurrence = RowFactory::for_device(1)
            .create(&a, "pm_occurrences")
            .await
            .expect("occurrence");
        let version = scalar_opt_text(
            &a,
            "SELECT v.entity_sync_id FROM pm_occurrences o JOIN pm_plan_versions v ON v.id = o.plan_version_id
             WHERE o.entity_sync_id = ?",
            vec![occurrence.entity_sync_id.clone().into()],
        )
        .await
        .expect("version");
        let plan = scalar_opt_text(
            &a,
            "SELECT p.entity_sync_id FROM pm_plan_versions v JOIN pm_plans p ON p.id = v.pm_plan_id
             WHERE v.entity_sync_id = ?",
            vec![version.clone().into()],
        )
        .await
        .expect("plan");
        let number = stored_number(&a, &version).await.expect("number");
        server.exchange("A", &a).await;
        // B receives the plan first, then adds its own version with the same number.
        let mut held_back = Vec::new();
        for (entity_type, sync_id) in [
            ("pm_plan_versions", version.clone()),
            ("pm_occurrences", occurrence.entity_sync_id.clone()),
        ] {
            let row = server
                .state
                .remove(&(entity_type.to_string(), sync_id.clone()))
                .expect("on the server");
            held_back.push((entity_type, sync_id, row));
        }
        server.exchange("B", &b).await;
        add_version(&b, &plan, "local-version-of-b", number).await;

        // What a PC before this update made of them: the version refused on its number, the
        // occurrence waiting for it.
        apply_sync_batch(
            &b,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
                server_batch_id: "batch_before_update".into(),
                checkpoint_token: "v2.before-update".into(),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: held_back
                    .iter()
                    .map(|(entity_type, sync_id, r)| SyncInboundItemInput {
                        entity_type: (*entity_type).to_string(),
                        entity_sync_id: sync_id.clone(),
                        operation: r.operation.clone(),
                        row_version: r.row_version,
                        payload_json: r.payload_json.clone(),
                        server_seq: Some(r.seq),
                        payload_hash: Some(r.payload_hash.clone()),
                        owner_user_sync_id: None,
                        deleted: false,
                    })
                    .collect(),
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
        let version_inbox = inbox_id(&b, "pm_plan_versions", &version).await;
        let occurrence_inbox = inbox_id(&b, "pm_occurrences", &occurrence.entity_sync_id).await;
        let message = format!(
            "Database error: Execution Error: error returned from database: (code: 2067) {VERSION_NUMBER_TAKEN}"
        );
        exec(
            &b,
            "UPDATE sync_inbox SET apply_status = 'conflict', rejection_code = 'MATERIALIZE_ERROR', rejection_message = ?
             WHERE id = ?",
            vec![message.clone().into(), version_inbox.into()],
        )
        .await;
        exec(
            &b,
            "UPDATE sync_inbox SET apply_status = 'deferred', rejection_code = 'MISSING_PARENT',
                    rejection_message = 'plan_version_id needs pm_plan_versions, not received yet'
             WHERE id = ?",
            vec![occurrence_inbox.into()],
        )
        .await;
        record_sync_conflict(
            &b,
            NewSyncConflict {
                source_scope: "inbound",
                source_batch_id: Some("batch_before_update".into()),
                linked_outbox_id: None,
                linked_inbox_id: Some(version_inbox),
                entity_type: "pm_plan_versions".into(),
                entity_sync_id: version.clone(),
                operation: held_back[0].2.operation.clone(),
                conflict_type: "MATERIALIZE_ERROR".into(),
                local_payload_json: None,
                inbound_payload_json: Some(held_back[0].2.payload_json.clone()),
                checkpoint_token: None,
                local_row_version: None,
                inbound_row_version: Some(held_back[0].2.row_version),
                detail_message: Some(message),
            },
        )
        .await
        .expect("conflict as recorded before the update");
        assert_eq!(open_conflicts(&b).await, 1);

        // The next synchronisation after the update; no one touches the conflict.
        crate::sync::materialize::materialize_inbox(&b, 200, true)
            .await
            .expect("materialize");

        assert_eq!(
            open_conflicts(&b).await,
            0,
            "the refused version is applied: {:?}",
            list_sync_conflicts(&b, SyncConflictFilter::default())
                .await
                .expect("conflicts")
                .iter()
                .map(|c| (
                    c.entity_type.clone(),
                    c.conflict_type.clone(),
                    c.status.clone(),
                    c.detail_message.clone()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            scalar_opt_text(
                &b,
                "SELECT resolution_action FROM sync_conflicts WHERE linked_inbox_id = ?",
                vec![version_inbox.into()],
            )
            .await
            .as_deref(),
            Some("applied_after_update")
        );
        // Both kept with the number their PC gave them, shown as two distinct numbers.
        assert_eq!(stored_number(&b, &version).await, Some(number));
        assert_eq!(stored_number(&b, "local-version-of-b").await, Some(number));
        let shown = shown_numbers(&b, &plan).await;
        assert_eq!(shown.len(), 2);
        assert_ne!(shown[0].1, shown[1].1, "{shown:?}");
        assert_eq!(
            scalar_opt_text(
                &b,
                "SELECT apply_status FROM sync_inbox WHERE id = ?",
                vec![occurrence_inbox.into()],
            )
            .await
            .as_deref(),
            Some("applied"),
            "the occurrence that waited for it follows"
        );
    }

    async fn notices(db: &DatabaseConnection, key: &str) -> (i64, i64) {
        let events = scalar_i64(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ? AND message_key = ?",
            vec![
                crate::settings::received_notice::PM_CONCURRENT_VERSIONS_CATEGORY.into(),
                key.into(),
            ],
        )
        .await;
        let delivered = scalar_i64(
            db,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id
             WHERE e.category_code = ? AND e.message_key = ?",
            vec![
                crate::settings::received_notice::PM_CONCURRENT_VERSIONS_CATEGORY.into(),
                key.into(),
            ],
        )
        .await;
        (events, delivered)
    }

    #[tokio::test]
    async fn the_planners_are_told_of_two_concurrent_versions_and_a_due_date_planned_twice() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let plan = RowFactory::for_device(1).create(&a, "pm_plans").await.expect("plan");
        server.exchange("A", &a).await;
        server.exchange("B", &b).await;
        // Offline, each PC adds version 1, publishes it as the plan's version in force and plans the
        // same due date with a work order.
        let mut occurrences = Vec::new();
        for (index, (db, version, occurrence)) in [(&a, A1, "occ-a"), (&b, B1, "occ-b")].into_iter().enumerate() {
            add_version(db, &plan.entity_sync_id, version, 1).await;
            let wo = RowFactory::for_device(i64::try_from(index).expect("index") + 10)
                .create(db, "work_orders")
                .await
                .expect("work order");
            exec(
                db,
                "INSERT INTO pm_occurrences (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status,
                                             linked_work_order_id)
                 SELECT ?, v.pm_plan_id, v.id, 'calendar', '2026-12-01T08:00:00Z', 'generated', ?
                 FROM pm_plan_versions v WHERE v.entity_sync_id = ?",
                vec![occurrence.into(), wo.rowid.into(), version.into()],
            )
            .await;
            occurrences.push(occurrence);
        }
        for (name, db) in [("A", &a), ("B", &b), ("A", &a)] {
            let report = server.exchange(name, db).await;
            assert_eq!(report.materialized.conflicts, 0, "{name}: {report:?}");
        }
        for (name, db) in [("A", &a), ("B", &b)] {
            let (events, delivered) = notices(db, "pm_concurrent_versions").await;
            assert_eq!(events, 1, "{name}: one notice of the two versions");
            assert!(delivered >= 1, "{name}: delivered to the PM planners");
            let (events, _) = notices(db, "pm_duplicate_occurrence").await;
            assert_eq!(events, 1, "{name}: one notice of the due date planned twice");
            // Both occurrences and their work orders are kept as they were.
            for occurrence in &occurrences {
                assert_eq!(
                    scalar_opt_text(
                        db,
                        "SELECT status FROM pm_occurrences WHERE entity_sync_id = ? AND linked_work_order_id IS NOT NULL",
                        vec![(*occurrence).into()],
                    )
                    .await
                    .as_deref(),
                    Some("generated"),
                    "{name}: {occurrence}"
                );
            }
        }
        let params = scalar_opt_text(
            &a,
            "SELECT message_params_json FROM notification_events WHERE message_key = 'pm_duplicate_occurrence'",
            vec![],
        )
        .await
        .expect("params");
        let params: serde_json::Value = serde_json::from_str(&params).expect("json");
        assert!(
            params["work_order"].is_string() && params["other_work_order"].is_string(),
            "{params}"
        );
        assert_ne!(params["version"], params["other_version"], "{params}");
    }
}
