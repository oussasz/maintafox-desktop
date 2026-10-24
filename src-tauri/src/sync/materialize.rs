//! Inbox materialization — apply pending sync_inbox rows to domain tables.
//!
//! Ingest (`apply_sync_batch`) stores envelopes as `pending`. This module applies them in
//! dependency order (`sync::registry::apply_priority`) and marks each row:
//! - `applied`: written to the domain table (or already current here);
//! - `deferred`: waiting for a parent row from a later batch, or for a materializer (entity types
//!   that are staged but not yet applied inbound); retried on every run;
//! - `conflict`: needs a person; a `sync_conflicts` record is written in the same transaction.
//!
//! A row is never marked `applied` unless it was applied or is already current.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::sync::queries::{record_sync_conflict, record_synced_state, synced_row_version, NewSyncConflict};
use crate::sync::registry::{self, ConflictPolicy};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterializeInboxResult {
    pub processed: i64,
    pub applied: i64,
    pub conflicts: i64,
    /// Already current here (replayed row or this device's own change sent back).
    pub skipped: i64,
    /// Waiting for a parent row or a materializer; retried on the next run.
    pub deferred: i64,
    pub errors: Vec<String>,
}

/// Deferral code for a row whose parent has not been applied yet.
pub(crate) const DEFER_MISSING_PARENT: &str = "MISSING_PARENT";
/// Deferral code for an entity type that is staged by other devices but not applied here yet.
pub(crate) const DEFER_NO_MATERIALIZER: &str = "NO_MATERIALIZER";
/// Deferral code for a row without a field this version requires (sent by an older app version).
pub(crate) const DEFER_MISSING_FIELD: &str = "MISSING_FIELD";
/// Deferral code for an entity type this version of the app does not know.
pub(crate) const DEFER_UNKNOWN_ENTITY_TYPE: &str = "UNKNOWN_ENTITY_TYPE";

#[derive(Debug, Clone)]
pub(crate) struct InboxRow {
    pub(crate) id: i64,
    server_batch_id: String,
    checkpoint_token: String,
    pub(crate) entity_type: String,
    pub(crate) entity_sync_id: String,
    operation: String,
    row_version: i64,
    pub(crate) payload_json: String,
    payload_hash: String,
}

const INBOX_COLUMNS: &str = "id, server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                   row_version, payload_json, payload_hash";

fn inbox_row_from(row: &sea_orm::QueryResult) -> AppResult<InboxRow> {
    Ok(InboxRow {
        id: row.try_get("", "id").map_err(|e| decode("id", e))?,
        server_batch_id: row
            .try_get("", "server_batch_id")
            .map_err(|e| decode("server_batch_id", e))?,
        checkpoint_token: row
            .try_get("", "checkpoint_token")
            .map_err(|e| decode("checkpoint_token", e))?,
        entity_type: row.try_get("", "entity_type").map_err(|e| decode("entity_type", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode("entity_sync_id", e))?,
        operation: row.try_get("", "operation").map_err(|e| decode("operation", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode("row_version", e))?,
        payload_json: row.try_get("", "payload_json").map_err(|e| decode("payload_json", e))?,
        payload_hash: row.try_get("", "payload_hash").map_err(|e| decode("payload_hash", e))?,
    })
}

/// One inbox row by its id.
pub(crate) async fn load_inbox_row(conn: &impl ConnectionTrait, id: i64) -> AppResult<Option<InboxRow>> {
    conn.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        format!("SELECT {INBOX_COLUMNS} FROM sync_inbox WHERE id = ?"),
        [id.into()],
    ))
    .await?
    .as_ref()
    .map(inbox_row_from)
    .transpose()
}

/// Whether an inbox row would pass the checks that come before its apply: not this device's own
/// change sent back, no unsent local change of the row, not older than (or diverging from) the
/// version applied here, no code clash. The safety re-checks (`permit::sync_check`) count a
/// waiting row only then.
pub(crate) async fn inbox_row_admissible(conn: &impl ConnectionTrait, item: &InboxRow) -> AppResult<bool> {
    let Some(def) = registry::entity(&item.entity_type) else {
        return Ok(false);
    };
    if !def.has_materializer || concurrency_check(conn, def, item).await?.is_some() {
        return Ok(false);
    }
    Ok(crate::sync::code_clash::check_inbound(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.operation,
        &item.payload_json,
    )
    .await?
    .is_none())
}

/// Applies an inbox row inside the caller's transaction, through the same checks as the inbox pass
/// (concurrency, code clash, apply), and records it as applied (inbox status, closed clashes, synced
/// version) when it is. Used for the rows applied together with the row they belong to; the caller
/// rolls everything back when one of them is not applied.
pub(crate) async fn apply_inbox_row_in(tx: &impl ConnectionTrait, item: &InboxRow) -> AppResult<ApplyOutcome> {
    let outcome = Box::pin(evaluate_and_apply(tx, item)).await?;
    if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged) {
        record_applied(tx, item).await?;
    }
    Ok(outcome)
}

async fn record_applied(tx: &impl ConnectionTrait, item: &InboxRow) -> AppResult<()> {
    mark_inbox(tx, item.id, "applied", None, None).await?;
    crate::sync::code_clash::close_outdated_clashes(tx, &item.entity_type, &item.entity_sync_id, item.row_version)
        .await?;
    crate::sync::username_clash::close_outdated_username_clashes(
        tx,
        &item.entity_type,
        &item.entity_sync_id,
        item.row_version,
    )
    .await?;
    record_synced_state(
        tx,
        &item.entity_type,
        &item.entity_sync_id,
        item.row_version,
        &item.payload_hash,
        "inbound_apply",
    )
    .await
}

fn decode(field: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inbox {field}: {e}"))
}

/// Which inbox rows one materialization pass takes.
#[derive(Debug, Clone, Copy)]
enum Selection<'a> {
    /// These rows, if still in the given status (`pending`: never tried; `deferred`: waiting for
    /// a parent). Callers order the ids parents first and pass them in chunks, so the order is
    /// computed once per call and rows that cannot be applied never hide the ones that can.
    Ids { status: &'static str, ids: &'a [i64] },
    /// Both, pending first (the older one-call behaviour, used by the replay tools and tests).
    Any,
}

/// Apply pending inbox rows, plus deferred rows whose entity type can now be applied (one pass of
/// at most `limit` rows, pending rows first).
pub async fn materialize_inbox_batch(db: &DatabaseConnection, limit: Option<i64>) -> AppResult<MaterializeInboxResult> {
    materialize_selection(db, Selection::Any, limit.unwrap_or(200)).await
}

/// Apply every pending inbox row (passes of `batch` rows, parents first). With `retry_deferred`,
/// then sweep the deferred rows until a sweep applies nothing more. A snapshot applies each page
/// without the sweep (rows whose parent comes in a later page are deferred once) and sweeps after
/// its last page, so the deferred rows are not re-read after every page.
pub async fn materialize_inbox(
    db: &DatabaseConnection,
    batch: i64,
    retry_deferred: bool,
) -> AppResult<MaterializeInboxResult> {
    let mut total = MaterializeInboxResult {
        processed: 0,
        applied: 0,
        conflicts: 0,
        skipped: 0,
        deferred: 0,
        errors: Vec::new(),
    };
    let add = |total: &mut MaterializeInboxResult, r: MaterializeInboxResult| {
        total.processed += r.processed;
        total.applied += r.applied;
        total.conflicts += r.conflicts;
        total.skipped += r.skipped;
        total.errors.extend(r.errors);
    };
    // PM plan versions refused before two versions could share a number are applied again first,
    // so the rows waiting for them follow in this run (`pm::version_clash`).
    total.applied += crate::pm::version_clash::retry_refused_number_clashes(db).await?;
    // Rows arriving while this runs are taken by the next round; every round moves its rows out
    // of `pending`, so this ends.
    let started = std::time::Instant::now();
    loop {
        let ids = ordered_ids(db, "pending").await?;
        if ids.is_empty() {
            break;
        }
        for (n, chunk) in ids.chunks(chunk_len(batch)).enumerate() {
            let r = materialize_selection(
                db,
                Selection::Ids {
                    status: "pending",
                    ids: chunk,
                },
                batch,
            )
            .await?;
            add(&mut total, r);
            if (n + 1) % PROGRESS_EVERY_CHUNKS == 0 {
                tracing::info!(
                    target: "maintafox::sync",
                    done = (n + 1) * chunk_len(batch),
                    of = ids.len(),
                    elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    "applying received rows"
                );
            }
        }
    }
    if retry_deferred {
        // Parents first, so one sweep usually applies a whole waiting chain; a sweep that applies
        // nothing leaves only rows that cannot be applied yet.
        loop {
            let ids = ordered_ids(db, "deferred").await?;
            let mut applied = 0;
            for chunk in ids.chunks(chunk_len(batch)) {
                let r = materialize_selection(
                    db,
                    Selection::Ids {
                        status: "deferred",
                        ids: chunk,
                    },
                    batch,
                )
                .await?;
                applied += r.applied + r.skipped;
                add(&mut total, r);
            }
            if applied == 0 {
                break;
            }
        }
    }
    total.deferred = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM sync_inbox WHERE apply_status = 'deferred'".to_string(),
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "n"))
        .transpose()
        .map_err(|e| decode("deferred count", e))?
        .unwrap_or(0);
    Ok(total)
}

/// One pass over the selected rows, with its own reference-lookup cache.
/// A progress line every this many chunks of a long materialization (a bootstrap).
const PROGRESS_EVERY_CHUNKS: usize = 40;

fn chunk_len(batch: i64) -> usize {
    usize::try_from(batch.clamp(1, 2000)).unwrap_or(500)
}

/// Inbox rows in `status` that this version can apply, parents first (`registry::apply_priority`),
/// then by arrival.
async fn ordered_ids(db: &DatabaseConnection, status: &str) -> AppResult<Vec<i64>> {
    let materializable = serde_json::to_string(&registry::materializable_entity_types())?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type FROM sync_inbox
             WHERE apply_status = ? AND entity_type IN (SELECT value FROM json_each(?))",
            [status.into(), materializable.into()],
        ))
        .await?;
    let mut ordered = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: i64 = r.try_get("", "id").map_err(|e| decode("id", e))?;
        let entity_type: String = r.try_get("", "entity_type").map_err(|e| decode("entity_type", e))?;
        ordered.push((registry::apply_priority(&entity_type), id));
    }
    ordered.sort_unstable();
    Ok(ordered.into_iter().map(|(_, id)| id).collect())
}

async fn materialize_selection(
    db: &DatabaseConnection,
    selection: Selection<'_>,
    limit: i64,
) -> AppResult<MaterializeInboxResult> {
    let result = crate::sync::row_codec::REF_CACHE
        .scope(Default::default(), materialize_pass(db, selection, limit))
        .await;
    // Safety net: marks the pass could not take (`inventory::reservations`); nothing is written when
    // there is none. A failure keeps the marks for the next pass, cycle or start.
    if let Err(e) = crate::inventory::reservations::recompute_dirty_reserved(db).await {
        tracing::warn!(target: "maintafox::sync", "reserved stock recompute after apply failed: {e}");
    }
    result
}

async fn materialize_pass(
    db: &DatabaseConnection,
    selection: Selection<'_>,
    limit: i64,
) -> AppResult<MaterializeInboxResult> {
    let lim = limit.clamp(1, 2000);
    let materializable = serde_json::to_string(&registry::materializable_entity_types())?;
    let columns = INBOX_COLUMNS;
    let statement = match selection {
        Selection::Ids { status, ids } => Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                // Driven by the id list (primary key lookups), not by a scan of every row in
                // that status: the inbox of a large bootstrap holds hundreds of thousands.
                "SELECT {} FROM json_each(?) AS j CROSS JOIN sync_inbox ON sync_inbox.id = j.value
                 WHERE sync_inbox.apply_status = ?",
                columns
                    .split(',')
                    .map(|c| format!("sync_inbox.{}", c.trim()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            [serde_json::to_string(ids)?.into(), status.into()],
        ),
        Selection::Any => Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT {columns} FROM sync_inbox
                 WHERE apply_status = 'pending'
                    OR (apply_status = 'deferred' AND entity_type IN (SELECT value FROM json_each(?)))
                 ORDER BY CASE apply_status WHEN 'pending' THEN 0 ELSE 1 END, id ASC
                 LIMIT ?"
            ),
            [materializable.into(), lim.into()],
        ),
    };
    let rows = db.query_all(statement).await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(inbox_row_from(row)?);
    }
    // Creates and updates go parents first; deletes go after them, children first, so a parent
    // is never removed while a row received in the same run still points to it.
    items.sort_by(|a, b| {
        let a_delete = is_delete_operation(&a.operation);
        let b_delete = is_delete_operation(&b.operation);
        a_delete
            .cmp(&b_delete)
            .then_with(|| {
                let (pa, pb) = (
                    registry::apply_priority(&a.entity_type),
                    registry::apply_priority(&b.entity_type),
                );
                if a_delete {
                    pb.cmp(&pa)
                } else {
                    pa.cmp(&pb)
                }
            })
            .then(a.id.cmp(&b.id))
    });

    let mut result = MaterializeInboxResult {
        processed: i64::try_from(items.len()).unwrap_or(i64::MAX),
        applied: 0,
        conflicts: 0,
        skipped: 0,
        deferred: 0,
        errors: Vec::new(),
    };

    // One transaction per pass (one disk commit for up to `limit` rows instead of one per row);
    // each row still runs in its own savepoint, so a row that fails leaves nothing behind.
    let pass = db.begin().await?;
    // Rows that can change a settled OT's cost (finance late costs, swept after the commit).
    let mut late_cost_rows: Vec<(String, String)> = Vec::new();
    // Permit and isolation rows whose local consequences are written after the commit
    // (`permit::ended_hold`: the hold of running work, the end of downtime).
    let mut permit_rows: Vec<(String, String)> = Vec::new();
    // Rows waiting for a parent are retried in the same run as long as other rows were applied,
    // so a child that arrived before its parent in the same run does not wait for the next one.
    let mut queue = items;
    loop {
        let mut waiting_for_parent = Vec::new();
        let mut progressed = false;
        for item in queue {
            match apply_inbox_row(&pass, &item).await? {
                ApplyOutcome::Applied => {
                    result.applied += 1;
                    progressed = true;
                    if crate::finance::settlement::LATE_COST_SYNC_TYPES.contains(&item.entity_type.as_str()) {
                        late_cost_rows.push((item.entity_type.clone(), item.entity_sync_id.clone()));
                    }
                    if crate::permit::ended_hold::AFTER_SYNC_TYPES.contains(&item.entity_type.as_str()) {
                        permit_rows.push((item.entity_type.clone(), item.entity_sync_id.clone()));
                    }
                }
                ApplyOutcome::Unchanged => {
                    result.skipped += 1;
                    progressed = true;
                }
                ApplyOutcome::Deferred { code, .. } if code == DEFER_MISSING_PARENT => {
                    waiting_for_parent.push(item);
                }
                ApplyOutcome::Deferred { .. } => result.deferred += 1,
                ApplyOutcome::Conflict { code, message } => {
                    result.conflicts += 1;
                    if code == "MATERIALIZE_ERROR" {
                        result
                            .errors
                            .push(format!("{}/{}: {message}", item.entity_type, item.entity_sync_id));
                    }
                }
            }
        }
        if waiting_for_parent.is_empty() {
            break;
        }
        if !progressed {
            result.deferred += i64::try_from(waiting_for_parent.len()).unwrap_or(i64::MAX);
            break;
        }
        queue = waiting_for_parent;
    }
    // Reserved stock of the pairs this pass wrote, once per pair, committed with the pass
    // (`inventory::reservations`). A failure keeps the marks for the safety net below.
    if let Err(e) = crate::inventory::reservations::recompute_dirty_reserved_in(&pass).await {
        tracing::warn!(target: "maintafox::sync", "reserved stock recompute in the pass failed: {e}");
    }
    pass.commit().await?;

    // After the commit, with capture active: late-cost deltas are local writes, pushed like any other.
    if !late_cost_rows.is_empty() {
        crate::finance::settlement::post_late_costs_after_sync(db, &late_cost_rows).await;
    }
    // After the commit, in their own transactions: a failure never refuses the received rows.
    if !permit_rows.is_empty() {
        crate::permit::ended_hold::after_sync_pass(db, &permit_rows).await;
    }

    Ok(result)
}

/// The received row a deferred row waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaitsFor {
    pub(crate) entity_type: String,
    pub(crate) sync_id: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ApplyOutcome {
    /// Written to the domain table.
    Applied,
    /// Already current locally; nothing to write.
    Unchanged,
    /// Cannot be applied yet; kept for a later run. `waits_for`: the one row it waits for, when
    /// known (a parent not applied here yet), shown in the sync centre with the row.
    Deferred {
        code: String,
        message: String,
        waits_for: Option<WaitsFor>,
    },
    /// Needs a person.
    Conflict { code: String, message: String },
}

fn is_delete_operation(operation: &str) -> bool {
    operation == "delete" || operation == "archive"
}

/// Apply one inbox row in its own transaction and record the outcome.
async fn apply_inbox_row<C: ConnectionTrait + TransactionTrait>(db: &C, item: &InboxRow) -> AppResult<ApplyOutcome> {
    // Applied meanwhile with the row it belongs to (`permit::sync_check::apply_group`): nothing more.
    let status = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT apply_status FROM sync_inbox WHERE id = ?",
            [item.id.into()],
        ))
        .await?
        .map(|r| {
            r.try_get::<String>("", "apply_status")
                .map_err(|e| decode("apply_status", e))
        })
        .transpose()?;
    if status.as_deref() == Some("applied") {
        return Ok(ApplyOutcome::Unchanged);
    }
    let tx = db.begin().await?;
    // Local changes captured since the last drain must be in the outbox before the conflict check.
    crate::sync::capture::drain_capture_journal(&tx).await?;
    let evaluated = evaluate_and_apply(&tx, item).await;
    let outcome = match evaluated {
        Ok(outcome @ (ApplyOutcome::Applied | ApplyOutcome::Unchanged)) => {
            record_applied(&tx, item).await?;
            tx.commit().await?;
            return Ok(outcome);
        }
        Ok(other) => {
            tx.rollback().await?;
            crate::sync::row_codec::clear_ref_cache();
            other
        }
        Err(e) => {
            tx.rollback().await?;
            crate::sync::row_codec::clear_ref_cache();
            ApplyOutcome::Conflict {
                code: "MATERIALIZE_ERROR".into(),
                message: e.to_string(),
            }
        }
    };

    // Nothing from the attempt is kept; record why the row was not applied.
    let tx = db.begin().await?;
    match &outcome {
        ApplyOutcome::Deferred {
            code,
            message,
            waits_for,
        } => {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_inbox
                 SET apply_status = 'deferred', rejection_code = ?, rejection_message = ?,
                     waits_for_entity_type = ?, waits_for_sync_id = ?,
                     attempt_count = attempt_count + 1,
                     last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [
                    code.as_str().into(),
                    message.as_str().into(),
                    waits_for.as_ref().map(|w| w.entity_type.clone()).into(),
                    waits_for.as_ref().map(|w| w.sync_id.clone()).into(),
                    item.id.into(),
                ],
            ))
            .await?;
        }
        ApplyOutcome::Conflict { code, message } => {
            mark_inbox(&tx, item.id, "conflict", Some(code), Some(message)).await?;
            let local = latest_local_change(&tx, &item.entity_type, &item.entity_sync_id).await?;
            record_sync_conflict(
                &tx,
                NewSyncConflict {
                    source_scope: "inbound",
                    source_batch_id: Some(item.server_batch_id.clone()),
                    linked_outbox_id: None,
                    linked_inbox_id: Some(item.id),
                    entity_type: item.entity_type.clone(),
                    entity_sync_id: item.entity_sync_id.clone(),
                    operation: item.operation.clone(),
                    conflict_type: code.clone(),
                    local_payload_json: local.as_ref().map(|l| l.payload_json.clone()),
                    inbound_payload_json: Some(item.payload_json.clone()),
                    checkpoint_token: Some(item.checkpoint_token.clone()),
                    local_row_version: local.as_ref().map(|l| l.row_version),
                    inbound_row_version: Some(item.row_version),
                    detail_message: Some(message.clone()),
                },
            )
            .await?;
            // A reservation released on two PCs is settled without a person (`inventory::reservations`).
            crate::inventory::reservations::settle_double_release(&tx, &item.entity_type, &item.entity_sync_id).await?;
        }
        ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
    }
    tx.commit().await?;
    Ok(outcome)
}

struct LocalChange {
    row_version: i64,
    payload_json: String,
    payload_hash: String,
    base_row_version: Option<i64>,
}

/// Local changes of an entity that the server has not accepted yet, oldest first.
async fn unsent_local_changes(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Vec<LocalChange>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT row_version, payload_json, payload_hash, base_row_version FROM sync_outbox
             WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')
             ORDER BY id ASC",
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(LocalChange {
                row_version: r.try_get("", "row_version").map_err(|e| decode("row_version", e))?,
                payload_json: r.try_get("", "payload_json").map_err(|e| decode("payload_json", e))?,
                payload_hash: r.try_get("", "payload_hash").map_err(|e| decode("payload_hash", e))?,
                base_row_version: r
                    .try_get("", "base_row_version")
                    .map_err(|e| decode("base_row_version", e))?,
            })
        })
        .collect()
}

async fn latest_local_change(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<LocalChange>> {
    Ok(unsent_local_changes(conn, entity_type, entity_sync_id).await?.pop())
}

/// Decide whether an inbound row may be applied, then apply it.
/// Prefix of the inbox batch id of a snapshot this PC asked for (`sync::transport::worker` names
/// its snapshots `snapshot-<scope>-<uuid>`).
pub(crate) const SNAPSHOT_BATCH_PREFIX: &str = "snapshot-";

/// The inbox batch id an exchange answer is stored under: the server's id, never one that reads as
/// a snapshot of this PC (`snapshot-…` becomes `delta-snapshot-…`).
pub(crate) fn exchange_batch_id(server_batch_id: &str) -> String {
    if server_batch_id.starts_with(SNAPSHOT_BATCH_PREFIX) {
        format!("delta-{server_batch_id}")
    } else {
        server_batch_id.to_string()
    }
}

tokio::task_local! {
    /// While an inbox row is applied: whether it came with a snapshot this PC asked for.
    static APPLYING_SNAPSHOT_ROW: bool;
}

/// Whether the inbox row being applied came with a snapshot this PC asked for (`false` outside an
/// inbox apply: a "take theirs", a test applying a payload directly).
pub(crate) fn applying_snapshot_row() -> bool {
    APPLYING_SNAPSHOT_ROW.try_with(|s| *s).unwrap_or(false)
}

async fn evaluate_and_apply(conn: &impl ConnectionTrait, item: &InboxRow) -> AppResult<ApplyOutcome> {
    APPLYING_SNAPSHOT_ROW
        .scope(
            item.server_batch_id.starts_with(SNAPSHOT_BATCH_PREFIX),
            Box::pin(evaluate_and_apply_row(conn, item)),
        )
        .await
}

async fn evaluate_and_apply_row(conn: &impl ConnectionTrait, item: &InboxRow) -> AppResult<ApplyOutcome> {
    let Some(def) = registry::entity(&item.entity_type) else {
        return Ok(ApplyOutcome::Deferred {
            code: DEFER_UNKNOWN_ENTITY_TYPE.into(),
            message: format!("entity type '{}' is not known by this version", item.entity_type),
            waits_for: None,
        });
    };
    if !def.has_materializer {
        return Ok(ApplyOutcome::Deferred {
            code: DEFER_NO_MATERIALIZER.into(),
            message: format!("'{}' rows are not applied by this version yet", item.entity_type),
            waits_for: None,
        });
    }
    // The same stock count posted on two PCs: when the received session or line is the same posting
    // as this PC's unsent one, that one is closed and audited and the received version is applied
    // (it reached the server first; `inventory::count_sync`). Done here, not in
    // `concurrency_check`, which also serves a side-effect-free admissibility check.
    crate::inventory::count_sync::settle_concurrent_posting(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        item.row_version,
        &item.payload_json,
    )
    .await?;
    // This PC's own hold of a work order for a permit ended elsewhere, crossed by the same hold
    // from another PC (`permit::ended_hold`): closed, the received version is applied.
    crate::permit::ended_hold::settle_crossed_hold_inbound(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.payload_json,
    )
    .await?;
    if let Some(outcome) = concurrency_check(conn, def, item).await? {
        return Ok(outcome);
    }
    if let Some(outcome) = crate::sync::code_clash::check_inbound(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.operation,
        &item.payload_json,
    )
    .await?
    {
        return Ok(outcome);
    }
    apply_inbound_payload(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.operation,
        item.row_version,
        &item.payload_json,
    )
    .await
}

/// Owner decision: concurrent changes are never merged silently.
///
/// - Our own unsent change sent back by the server → already current.
/// - A local change the server has not accepted yet → `CONCURRENT_EDIT` (both sides changed the
///   row; with protocol v2 the local change was made on `base_row_version`, which the server
///   version has moved past).
/// - An older version than the one already applied → `STALE_INBOUND`.
/// - The same version with different content → `EQUAL_VERSION_DIVERGENCE`.
async fn concurrency_check(
    conn: &impl ConnectionTrait,
    def: &registry::SyncEntityDef,
    item: &InboxRow,
) -> AppResult<Option<ApplyOutcome>> {
    let unsent = unsent_local_changes(conn, &item.entity_type, &item.entity_sync_id).await?;
    if unsent.iter().any(|c| c.payload_hash == item.payload_hash) {
        return Ok(Some(ApplyOutcome::Unchanged));
    }
    if def.conflict_policy == ConflictPolicy::AppendOnly {
        // History rows are only inserted; the materializer skips rows that already exist.
        return Ok(None);
    }
    if let Some(local) = unsent.last() {
        let base = local
            .base_row_version
            .map_or_else(|| "a new row".to_string(), |v| format!("version {v}"));
        return Ok(Some(ApplyOutcome::Conflict {
            code: "CONCURRENT_EDIT".into(),
            message: format!(
                "Local change v{} (made on {base}) is not on the server yet; the server sent v{}.",
                local.row_version, item.row_version
            ),
        }));
    }
    if let Some((synced_version, synced_hash)) =
        synced_row_version(conn, &item.entity_type, &item.entity_sync_id).await?
    {
        if item.row_version < synced_version {
            return Ok(Some(ApplyOutcome::Conflict {
                code: "STALE_INBOUND".into(),
                message: format!(
                    "Server sent v{} but v{synced_version} is already applied here.",
                    item.row_version
                ),
            }));
        }
        if item.row_version == synced_version {
            if synced_hash.eq_ignore_ascii_case(&item.payload_hash) {
                return Ok(Some(ApplyOutcome::Unchanged));
            }
            return Ok(Some(ApplyOutcome::Conflict {
                code: "EQUAL_VERSION_DIVERGENCE".into(),
                message: format!(
                    "Server sent v{} with content that differs from the v{synced_version} applied here.",
                    item.row_version
                ),
            }));
        }
    }
    Ok(None)
}

async fn mark_inbox(
    conn: &impl ConnectionTrait,
    id: i64,
    status: &str,
    rejection_code: Option<&str>,
    rejection_message: Option<&str>,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_inbox
         SET apply_status = ?,
             rejection_code = ?,
             rejection_message = ?,
             waits_for_entity_type = NULL,
             waits_for_sync_id = NULL,
             last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            status.into(),
            rejection_code.into(),
            rejection_message.into(),
            id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Write an inbound row to its domain table without the concurrency check. Used for normal apply
/// (after the check) and when a person chooses the server version of a conflict.
pub(crate) async fn apply_inbound_payload(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    operation: &str,
    row_version: i64,
    payload_json: &str,
) -> AppResult<ApplyOutcome> {
    if crate::sync::row_spec::row_spec(entity_type).is_some() {
        let layout = crate::sync::row_layout::layout(conn, entity_type).await?;
        crate::sync::capture::begin_suppression(conn).await?;
        let applied = crate::sync::row_codec::apply_row(conn, &layout, entity_sync_id, operation, payload_json).await;
        crate::sync::capture::end_suppression(conn).await?;
        return applied;
    }
    let _ = row_version;
    Err(AppError::SyncError(format!(
        "No inbound materializer for '{entity_type}' although the registry declares one."
    )))
}
