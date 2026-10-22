//! Two PCs that number a version of the same PM plan alike (0.9.0 test finding 4, rule 1,
//! CMMS Advisor decision 2026-10-22).
//!
//! A version is numbered on the PC that creates it (`MAX + 1` of the plan there), and the number is
//! unique per plan. Two PCs that each add a version to a plan before receiving the other's give
//! both the same number. Before this rule the second one to arrive was refused (`MATERIALIZE_ERROR`)
//! and every row hanging off it waited for ever.
//!
//! Rule: when a received version holds the number of another version of the same plan here (a
//! different sync id), the version with the GREATER sync id takes the next free number of the plan
//! (the highest number here, plus one), on every PC: only sync ids are compared, never local ids
//! or times, so every PC moves the same version. Content, dates and authors are kept; the plan's
//! current version stays the one it points to (only the plan's current version drives
//! scheduling, `pm::generation`, so two concurrent versions never both do).
//! - The received version moves: it is stored here under the new number, without sending
//!   anything (its own PC sends the move, below).
//! - The version held here moves: the change is recorded like an edit and sent when the version was
//!   created on this PC (no received copy of it applied here); a copy received from a third PC is
//!   moved here only, its own PC sending the move. Every PC thus ends with the numbers the creating
//!   PCs sent.
//! - Each move is written to the audit trail ("renuméroté de v{a} en v{b} (conflit de
//!   synchronisation)", [`RENUMBERED_ACTION`]).
//!
//! A received version refused before this rule (an open `MATERIALIZE_ERROR` conflict on that unique
//! number) is applied again under the rule at the next synchronisation
//! ([`retry_refused_number_clashes`]); its conflict is closed when it applies and stays open with
//! its rows waiting when it still does not.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use crate::errors::{AppError, AppResult};

/// Audit action of a version renumbered by this rule.
pub const RENUMBERED_ACTION: &str = "pm.version.renumbered_sync_clash";

/// The database text of the unique number refused before this rule.
const VERSION_NUMBER_TAKEN: &str = "UNIQUE constraint failed: pm_plan_versions.pm_plan_id, pm_plan_versions.version_no";

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("pm version clash: {e}"))
}

struct Holder {
    id: i64,
    sync_id: String,
}

async fn scalar_i64(conn: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await?
        .map(|r| r.try_get_by_index::<Option<i64>>(0))
        .transpose()
        .map_err(err)
        .map(Option::flatten)
}

/// Suppresses capture (writes kept on this PC), unless the caller already suppressed it.
async fn locally<C: ConnectionTrait>(conn: &C, already: bool) -> AppResult<()> {
    if !already {
        crate::sync::capture::begin_suppression(conn).await?;
    }
    Ok(())
}

async fn end_locally<C: ConnectionTrait>(conn: &C, already: bool) -> AppResult<()> {
    if !already {
        crate::sync::capture::end_suppression(conn).await?;
    }
    Ok(())
}

async fn capture_suppressed(conn: &impl ConnectionTrait) -> AppResult<bool> {
    Ok(
        scalar_i64(conn, "SELECT 1 FROM sync_capture_suppression LIMIT 1", vec![])
            .await?
            .is_some(),
    )
}

/// Whether a received copy of this version was ever applied here (it was not created on this PC).
async fn received_here(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<bool> {
    Ok(scalar_i64(
        conn,
        "SELECT 1 FROM sync_inbox WHERE entity_type = 'pm_plan_versions' AND entity_sync_id = ?
           AND apply_status IN ('applied', 'superseded') LIMIT 1",
        vec![sync_id.into()],
    )
    .await?
    .is_some())
}

/// `version_id`: the local id of the version moved (none yet for a received version, written
/// after this; its sync id is in the details).
async fn audit_move(
    conn: &impl ConnectionTrait,
    version_id: Option<i64>,
    version_sync_id: &str,
    plan_id: i64,
    from: i64,
    to: i64,
    other_sync_id: &str,
) -> AppResult<()> {
    let plan_code = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT code FROM pm_plans WHERE id = ?",
            [plan_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "code"))
        .transpose()
        .map_err(err)?
        .flatten();
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: RENUMBERED_ACTION.to_string(),
            target_type: Some("pm_plan_version".to_string()),
            target_id: version_id.map(|id| id.to_string()),
            actor_id: None,
            auth_context: "system".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "operations".to_string(),
            details_json: Some(serde_json::json!({
                "summary": format!("renuméroté de v{from} en v{to} (conflit de synchronisation)"),
                "plan_code": plan_code,
                "from_version_no": from,
                "to_version_no": to,
                "version_sync_id": version_sync_id,
                "clashing_version_sync_id": other_sync_id,
            })),
        },
    )
    .await?;
    Ok(())
}

/// Before a received `pm_plan_versions` row is written (capture still on): applies the rule of the
/// module when its number is held here by another version of the plan. Returns the payload to
/// write when the received version moves; `None` to write it as received.
pub(crate) async fn settle_inbound(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    operation: &str,
    payload_json: &str,
) -> AppResult<Option<String>> {
    if operation == "delete" || operation == "archive" {
        return Ok(None);
    }
    let Ok(Value::Object(mut payload)) = serde_json::from_str::<Value>(payload_json) else {
        return Ok(None);
    };
    let (Some(plan_sync_id), Some(number)) = (
        payload
            .get("pm_plan_sync_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        payload.get("version_no").and_then(Value::as_i64),
    ) else {
        // Not readable here: the apply reports it.
        return Ok(None);
    };
    let Some(plan_id) = scalar_i64(
        conn,
        "SELECT id FROM pm_plans WHERE entity_sync_id = ?",
        vec![plan_sync_id.into()],
    )
    .await?
    else {
        // The plan is not here yet: the row waits for it.
        return Ok(None);
    };
    let holder = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_sync_id FROM pm_plan_versions
             WHERE pm_plan_id = ? AND version_no = ? AND (entity_sync_id IS NULL OR entity_sync_id <> ?)",
            [plan_id.into(), number.into(), entity_sync_id.into()],
        ))
        .await?
        .map(|r| -> AppResult<Holder> {
            Ok(Holder {
                id: r.try_get("", "id").map_err(err)?,
                sync_id: r
                    .try_get::<Option<String>>("", "entity_sync_id")
                    .map_err(err)?
                    .unwrap_or_default(),
            })
        })
        .transpose()?;
    let Some(holder) = holder else {
        return Ok(None);
    };

    if entity_sync_id > holder.sync_id.as_str() {
        // The received version moves: the next number after every other version of the plan here.
        let next = scalar_i64(
            conn,
            "SELECT COALESCE(MAX(version_no), 0) + 1 FROM pm_plan_versions
             WHERE pm_plan_id = ? AND (entity_sync_id IS NULL OR entity_sync_id <> ?)",
            vec![plan_id.into(), entity_sync_id.into()],
        )
        .await?
        .unwrap_or(number + 1);
        payload.insert("version_no".to_string(), Value::from(next));
        let already = capture_suppressed(conn).await?;
        locally(conn, already).await?;
        audit_move(conn, None, entity_sync_id, plan_id, number, next, &holder.sync_id).await?;
        end_locally(conn, already).await?;
        return Ok(Some(Value::Object(payload).to_string()));
    }

    // The version held here moves, after every version of the plan here and the received number.
    let next = scalar_i64(
        conn,
        "SELECT MAX(COALESCE(MAX(version_no), 0), ?) + 1 FROM pm_plan_versions
         WHERE pm_plan_id = ? AND id <> ?",
        vec![number.into(), plan_id.into(), holder.id.into()],
    )
    .await?
    .unwrap_or(number + 1);
    // Sent when created on this PC; a copy received from another PC is moved here only.
    let send = !received_here(conn, &holder.sync_id).await?;
    let already = capture_suppressed(conn).await?;
    if !send {
        locally(conn, already).await?;
    }
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE pm_plan_versions
         SET version_no = ?, row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [next.into(), holder.id.into()],
    ))
    .await?;
    audit_move(
        conn,
        Some(holder.id),
        &holder.sync_id,
        plan_id,
        number,
        next,
        entity_sync_id,
    )
    .await?;
    if !send {
        end_locally(conn, already).await?;
    }
    Ok(None)
}

/// Received versions refused before this rule because their number was taken here (open
/// `MATERIALIZE_ERROR` conflicts): each is applied again under the rule. One that applies is
/// recorded as applied and its conflict closed; one that still does not stays as it was.
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
        match outcome {
            Ok(crate::sync::materialize::ApplyOutcome::Applied | crate::sync::materialize::ApplyOutcome::Unchanged) => {
                tx.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_conflicts
                     SET status = 'resolved_remote', resolution_action = 'applied_after_update',
                         resolution_note = 'Applied after the update: the version took the next free number of its plan.',
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE id = ?",
                    [conflict_id.into()],
                ))
                .await?;
                tx.commit().await?;
                applied += 1;
            }
            Ok(_) => tx.rollback().await?,
            Err(e) => {
                tx.rollback().await?;
                tracing::warn!(
                    target: "maintafox::sync",
                    inbox_id,
                    error = %e,
                    "a PM plan version refused earlier still cannot be applied"
                );
            }
        }
        crate::sync::row_codec::clear_ref_cache();
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::domain::{
        ApplySyncBatchInput, SyncConflictFilter, SyncInboundItemInput, SYNC_PROTOCOL_VERSION_V2,
    };
    use crate::sync::queries::{apply_sync_batch, list_sync_conflicts, record_sync_conflict, NewSyncConflict};
    use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

    const LOW: &str = "11111111-1111-4111-8111-111111111111";
    const HIGH: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

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

    async fn number_of(db: &DatabaseConnection, version_sync_id: &str) -> Option<i64> {
        scalar_opt_text(
            db,
            "SELECT CAST(version_no AS TEXT) FROM pm_plan_versions WHERE entity_sync_id = ?",
            vec![version_sync_id.into()],
        )
        .await
        .and_then(|n| n.parse().ok())
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

    /// Two PCs each add version 1 to the same plan, `a_version` on A and `b_version` on B.
    async fn two_pcs_number_alike(a_version: &str, b_version: &str) {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let plan = RowFactory::for_device(1).create(&a, "pm_plans").await.expect("plan");
        server.exchange("A", &a).await;
        server.exchange("B", &b).await;
        add_version(&a, &plan.entity_sync_id, a_version, 1).await;
        add_version(&b, &plan.entity_sync_id, b_version, 1).await;
        let current_before = scalar_opt_text(
            &a,
            "SELECT v.entity_sync_id FROM pm_plans p LEFT JOIN pm_plan_versions v ON v.id = p.current_version_id
             WHERE p.entity_sync_id = ?",
            vec![plan.entity_sync_id.clone().into()],
        )
        .await;

        for (name, db) in [("A", &a), ("B", &b), ("A", &a), ("B", &b), ("A", &a), ("B", &b)] {
            let report = server.exchange(name, db).await;
            assert_eq!(
                report.materialized.conflicts,
                0,
                "{name}: {report:?} {:?}",
                list_sync_conflicts(db, SyncConflictFilter::default())
                    .await
                    .expect("conflicts")
                    .iter()
                    .map(|c| (c.entity_type.clone(), c.conflict_type.clone(), c.detail_message.clone()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(report.conflicts, 0, "{name}: no server conflict: {report:?}");
        }

        let (low, high) = if a_version < b_version {
            (a_version, b_version)
        } else {
            (b_version, a_version)
        };
        for (name, db) in [("A", &a), ("B", &b)] {
            assert_eq!(number_of(db, low).await, Some(1), "{name}: the smaller sync id keeps 1");
            assert_eq!(
                number_of(db, high).await,
                Some(2),
                "{name}: the greater sync id moves to 2"
            );
            assert_eq!(open_conflicts(db).await, 0, "{name}");
            let plan_here = plan_id(db, &plan.entity_sync_id).await;
            assert_eq!(
                scalar_i64(
                    db,
                    "SELECT COUNT(*) FROM pm_plan_versions WHERE pm_plan_id = ?",
                    vec![plan_here.into()],
                )
                .await,
                2,
                "{name}: both versions kept"
            );
            // The plan keeps the one current version it pointed to: a renumbering never changes it.
            assert_eq!(
                scalar_opt_text(
                    db,
                    "SELECT v.entity_sync_id FROM pm_plans p LEFT JOIN pm_plan_versions v ON v.id = p.current_version_id
                     WHERE p.id = ?",
                    vec![plan_here.into()],
                )
                .await,
                current_before,
                "{name}"
            );
        }
        // The move is recorded on the PC that created the moved version, and the server holds it.
        let moved_on = if high == b_version { &b } else { &a };
        assert!(
            scalar_i64(
                moved_on,
                "SELECT COUNT(*) FROM audit_events WHERE action_code = ? \
                 AND json_extract(details_json, '$.summary') = 'renuméroté de v1 en v2 (conflit de synchronisation)'",
                vec![RENUMBERED_ACTION.into()],
            )
            .await
                >= 1
        );
        let on_server: Value = serde_json::from_str(
            &server
                .state
                .get(&("pm_plan_versions".to_string(), high.to_string()))
                .expect("on the server")
                .payload_json,
        )
        .expect("payload");
        assert_eq!(on_server["version_no"], 2);
    }

    #[tokio::test]
    async fn versions_numbered_alike_on_two_pcs_converge_when_the_receiver_holds_the_greater_id() {
        two_pcs_number_alike(LOW, HIGH).await;
    }

    #[tokio::test]
    async fn versions_numbered_alike_on_two_pcs_converge_when_the_sender_holds_the_greater_id() {
        two_pcs_number_alike(HIGH, LOW).await;
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
        let number = number_of(&a, &version).await.expect("number");
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
        let mut numbers = vec![
            number_of(&b, &version).await.expect("received version here"),
            number_of(&b, "local-version-of-b").await.expect("own version kept"),
        ];
        numbers.sort_unstable();
        assert_eq!(numbers, vec![number, number + 1]);
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
}
