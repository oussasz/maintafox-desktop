//! Change capture for replicated tables (Phase 2.4).
//!
//! Every replicated table with a `sync::row_spec` entry gets three SQLite triggers
//! (`sync_cap_<table>_ai|_au|_ad`). Each business insert, update or delete writes one row to
//! `sync_capture_journal` **inside the same transaction** as the write, so a committed change is
//! always captured and a rolled-back one never is, whichever command made it. The insert trigger
//! also gives a row created without `entity_sync_id` a random identity in the same statement.
//!
//! The journal is turned into outbox envelopes by [`drain_capture_journal`], which builds the
//! payload from the current row (`sync::row_codec`): before every push, and inside the transaction
//! that applies an inbound row (so a local change is always seen by the conflict check).
//!
//! Inbound rows are applied with capture suppressed (`sync_capture_suppression`), so a row received
//! from the server is not sent back. Triggers are dropped while migrations and startup seeders run
//! (rows every device seeds itself are never pushed) and installed again afterwards
//! ([`suspend_capture`], [`install_capture`]).

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::StageOutboxItemInput;
use crate::sync::queries::{record_sync_conflict, stage_outbox_item_with_owner, NewSyncConflict};
use crate::sync::registry::{self, SyncScope};
use crate::sync::row_codec::{self, RowLocator};
use crate::sync::row_layout::{self, EntityLayout, Role, Target};
use crate::sync::row_spec::ROW_SPECS;

/// Random RFC 9562 version-4 UUID in SQL (text form), for rows inserted without identity.
const UUID_V4_SQL: &str = "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || \
     substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || \
     substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))";

const TRIGGER_PREFIX: &str = "sync_cap_";

/// Journal rows read per round while draining.
const DRAIN_PAGE_SIZE: i64 = 500;

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaptureDrainReport {
    /// Entities whose current state was staged (create/update or delete).
    pub staged: i64,
    /// Entities with nothing to send (created and deleted before any sync).
    pub skipped: i64,
    /// Entities that could not be staged; kept in the journal and listed as `STAGE_ERROR` conflicts.
    pub failed: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaptureInstallReport {
    pub entity_types: i64,
    /// Rows that existed before their entity type was synchronised and were queued once.
    pub baseline_rows: i64,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Drop every capture trigger. Run before migrations and seeders.
pub async fn suspend_capture(conn: &impl ConnectionTrait) -> AppResult<()> {
    let names = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT name FROM sqlite_master WHERE type = 'trigger' AND substr(name, 1, {}) = '{TRIGGER_PREFIX}'",
                TRIGGER_PREFIX.len()
            ),
        ))
        .await?;
    for row in names {
        let name: String = row
            .try_get("", "name")
            .map_err(|e| AppError::SyncError(format!("trigger name: {e}")))?;
        conn.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {}", quote(&name)))
            .await?;
    }
    Ok(())
}

/// Install the capture triggers of every synchronised entity type, give identity to rows that
/// have none, and queue once the existing rows of entity types synchronised for the first time.
pub async fn install_capture(db: &DatabaseConnection) -> AppResult<CaptureInstallReport> {
    let tx = db.begin().await?;
    let report = install_capture_in(&tx).await?;
    tx.commit().await?;
    Ok(report)
}

/// [`install_capture`] on an open transaction or connection.
pub async fn install_capture_in(conn: &impl ConnectionTrait) -> AppResult<CaptureInstallReport> {
    suspend_capture(conn).await?;
    // Rows the migrations and startup seeders created without identity get their seeded one.
    crate::sync::seeded_identity::assign_seeded_identities(
        conn,
        crate::sync::seeded_identity::SeededScope::MissingIdentity,
    )
    .await?;
    let mut specs: Vec<_> = ROW_SPECS.iter().collect();
    specs.sort_by_key(|s| registry::apply_priority(s.entity_type));
    let mut report = CaptureInstallReport::default();
    for spec in specs {
        let layout = row_layout::layout(conn, spec.entity_type).await?;
        if layout.natural_pk.is_none() {
            conn.execute_unprepared(&format!(
                "UPDATE {} SET entity_sync_id = {UUID_V4_SQL} WHERE entity_sync_id IS NULL OR trim(entity_sync_id) = ''",
                quote(layout.table())
            ))
            .await?;
        }
        for sql in trigger_sql(&layout)? {
            conn.execute_unprepared(&sql).await?;
        }
        report.baseline_rows += baseline(conn, &layout).await?;
        report.entity_types += 1;
    }
    Ok(report)
}

/// Owner (`owner_user_sync_id`) of a row of a user-scoped entity, as SQL over `alias`.
fn owner_sql(layout: &EntityLayout, alias: &str) -> String {
    match (layout.def.scope, layout.def.owner_user_column) {
        (SyncScope::User, Some(column)) => format!(
            "(SELECT u.entity_sync_id FROM user_accounts u WHERE u.id = {alias}.{})",
            quote(column)
        ),
        _ => "NULL".to_string(),
    }
}

/// Natural key of a natural-key row in sync terms (`json_object`), as SQL over `alias`.
fn natural_key_sql(layout: &EntityLayout, alias: &str) -> AppResult<String> {
    let pk = layout
        .natural_pk
        .as_ref()
        .ok_or_else(|| AppError::SyncError(format!("{} is keyed by id", layout.table())))?;
    let mut parts = Vec::with_capacity(pk.len());
    for &i in pk {
        let column = &layout.columns[i];
        let col = format!("{alias}.{}", quote(&column.name));
        let (key, expr) = match &column.role {
            Role::Value { key } => (key, col),
            Role::Ref {
                key,
                target: Target::Entity { table, .. },
                ..
            } => (
                key,
                format!("(SELECT entity_sync_id FROM {} WHERE id = {col})", quote(table)),
            ),
            Role::Ref {
                key,
                target: Target::Catalog { table, key_column },
                ..
            } => (
                key,
                format!("(SELECT {} FROM {} WHERE id = {col})", quote(key_column), quote(table)),
            ),
            _ => {
                return Err(AppError::SyncError(format!(
                    "{}.{}: unsupported natural key column",
                    layout.table(),
                    column.name
                )))
            }
        };
        parts.push(format!("'{key}', {expr}"));
    }
    Ok(format!("json_object({})", parts.join(", ")))
}

const NOT_SUPPRESSED: &str = "NOT EXISTS (SELECT 1 FROM sync_capture_suppression)";

/// An update is a change to send only when a synchronised column changed: updates of device-local
/// or derived columns (a login time, an SLA "notified" stamp, an org path) and of the local
/// version counter alone stay on the device.
fn replicated_change_sql(layout: &EntityLayout) -> String {
    let changed: Vec<String> = layout
        .columns
        .iter()
        .filter(|c| matches!(c.role, Role::Identity | Role::Value { .. } | Role::Ref { .. }))
        .map(|c| format!("NEW.{0} IS NOT OLD.{0}", quote(&c.name)))
        .collect();
    if changed.is_empty() {
        "0".to_string()
    } else {
        format!("({})", changed.join(" OR "))
    }
}

fn trigger_sql(layout: &EntityLayout) -> AppResult<Vec<String>> {
    let table = quote(layout.table());
    let et = layout.def.entity_type;
    let name = |suffix: &str| quote(&format!("{TRIGGER_PREFIX}{}_{suffix}", layout.table()));
    let journal =
        "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, owner_user_sync_id)";
    Ok(if layout.natural_pk.is_none() {
        vec![
            format!(
                "CREATE TRIGGER {ai} AFTER INSERT ON {table} BEGIN
                    UPDATE {table} SET entity_sync_id = {UUID_V4_SQL}
                     WHERE id = NEW.id AND (NEW.entity_sync_id IS NULL OR trim(NEW.entity_sync_id) = '');
                    {journal}
                    SELECT '{et}', r.entity_sync_id, NULL, 'upsert', {owner}
                      FROM {table} r
                     WHERE r.id = NEW.id AND {filter} AND {NOT_SUPPRESSED};
                 END",
                ai = name("ai"),
                owner = owner_sql(layout, "r"),
                filter = layout.filter_sql("r"),
            ),
            format!(
                "CREATE TRIGGER {au} AFTER UPDATE ON {table}
                 WHEN OLD.entity_sync_id IS NOT NULL AND trim(OLD.entity_sync_id) <> '' AND {changed} AND {NOT_SUPPRESSED}
                 BEGIN
                    {journal}
                    SELECT '{et}', NEW.entity_sync_id, NULL, 'upsert', {owner_new}
                     WHERE NEW.entity_sync_id IS NOT NULL AND ({filter_new} OR {filter_old});
                    {journal}
                    SELECT '{et}', OLD.entity_sync_id, NULL, 'delete', {owner_old}
                     WHERE NEW.entity_sync_id IS NOT OLD.entity_sync_id AND {filter_old};
                 END",
                au = name("au"),
                changed = replicated_change_sql(layout),
                owner_new = owner_sql(layout, "NEW"),
                owner_old = owner_sql(layout, "OLD"),
                filter_new = layout.filter_sql("NEW"),
                filter_old = layout.filter_sql("OLD"),
            ),
            format!(
                "CREATE TRIGGER {ad} AFTER DELETE ON {table}
                 WHEN OLD.entity_sync_id IS NOT NULL AND trim(OLD.entity_sync_id) <> '' AND {filter_old} AND {NOT_SUPPRESSED}
                 BEGIN
                    {journal}
                    VALUES ('{et}', OLD.entity_sync_id, NULL, 'delete', {owner_old});
                 END",
                ad = name("ad"),
                owner_old = owner_sql(layout, "OLD"),
                filter_old = layout.filter_sql("OLD"),
            ),
        ]
    } else {
        let key_new = natural_key_sql(layout, "NEW")?;
        let key_old = natural_key_sql(layout, "OLD")?;
        vec![
            format!(
                "CREATE TRIGGER {ai} AFTER INSERT ON {table}
                 WHEN {filter_new} AND {NOT_SUPPRESSED}
                 BEGIN
                    {journal} VALUES ('{et}', NULL, {key_new}, 'upsert', {owner_new});
                 END",
                ai = name("ai"),
                owner_new = owner_sql(layout, "NEW"),
                filter_new = layout.filter_sql("NEW"),
            ),
            format!(
                "CREATE TRIGGER {au} AFTER UPDATE ON {table}
                 WHEN {changed} AND {NOT_SUPPRESSED}
                 BEGIN
                    {journal}
                    SELECT '{et}', NULL, {key_new}, 'upsert', {owner_new}
                     WHERE {filter_new} OR {filter_old};
                    {journal}
                    SELECT '{et}', NULL, {key_old}, 'delete', {owner_old}
                     WHERE {key_new} IS NOT {key_old} AND {filter_old};
                 END",
                au = name("au"),
                changed = replicated_change_sql(layout),
                owner_new = owner_sql(layout, "NEW"),
                owner_old = owner_sql(layout, "OLD"),
                filter_new = layout.filter_sql("NEW"),
                filter_old = layout.filter_sql("OLD"),
            ),
            format!(
                "CREATE TRIGGER {ad} AFTER DELETE ON {table}
                 WHEN {filter_old} AND {NOT_SUPPRESSED}
                 BEGIN
                    {journal} VALUES ('{et}', NULL, {key_old}, 'delete', {owner_old});
                 END",
                ad = name("ad"),
                owner_old = owner_sql(layout, "OLD"),
                filter_old = layout.filter_sql("OLD"),
            ),
        ]
    })
}

/// Queue the existing rows of an entity type the first time it is synchronised on this database.
///
/// Rows the server already has (known version) or that are already in the outbox are skipped, and
/// so are rows every device seeds itself: they carry a seeded identity (`seeded_entity_sync_id`,
/// UUID version 8) or belong to a natural-key catalog marked `skip_baseline`.
async fn baseline(conn: &impl ConnectionTrait, layout: &EntityLayout) -> AppResult<i64> {
    let et = layout.def.entity_type;
    let done = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS v FROM sync_capture_baseline WHERE entity_type = ?",
            [et.into()],
        ))
        .await?
        .is_some();
    if done {
        return Ok(0);
    }
    let table = quote(layout.table());
    let queued = if layout.natural_pk.is_none() {
        conn.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, owner_user_sync_id)
                 SELECT '{et}', r.entity_sync_id, NULL, 'upsert', {owner}
                   FROM {table} r
                  WHERE r.entity_sync_id IS NOT NULL
                    -- Seeded identities (UUID v8, `seeded_entity_sync_id`) exist on every device.
                    -- Older modules prefix their ids (`failure_event:<uuid>`): only a bare UUID
                    -- can be a seeded one.
                    AND NOT (length(r.entity_sync_id) = 36
                             AND substr(r.entity_sync_id, 9, 1) = '-'
                             AND substr(r.entity_sync_id, 15, 1) = '8')
                    AND {filter}
                    AND NOT EXISTS (SELECT 1 FROM sync_entity_state s
                                     WHERE s.entity_type = '{et}' AND s.entity_sync_id = r.entity_sync_id)
                    AND NOT EXISTS (SELECT 1 FROM sync_outbox o
                                     WHERE o.entity_type = '{et}' AND o.entity_sync_id = r.entity_sync_id
                                       AND o.status <> 'superseded')
                  ORDER BY r.id",
                owner = owner_sql(layout, "r"),
                filter = layout.filter_sql("r"),
            ),
        ))
        .await?
        .rows_affected()
    } else if layout.spec.skip_baseline {
        0
    } else {
        conn.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, owner_user_sync_id)
                 SELECT '{et}', NULL, {key}, 'upsert', {owner} FROM {table} r WHERE {filter}",
                key = natural_key_sql(layout, "r")?,
                owner = owner_sql(layout, "r"),
                filter = layout.filter_sql("r"),
            ),
        ))
        .await?
        .rows_affected()
    };
    let queued = i64::try_from(queued).unwrap_or(i64::MAX);
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_capture_baseline (entity_type, captured_at, row_count)
         VALUES (?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
        [et.into(), queued.into()],
    ))
    .await?;
    Ok(queued)
}

/// Turn capture off for the current transaction while inbound rows are applied. Always pair with
/// [`end_suppression`] before the transaction commits.
pub(crate) async fn begin_suppression(conn: &impl ConnectionTrait) -> AppResult<()> {
    conn.execute_unprepared("INSERT OR IGNORE INTO sync_capture_suppression (id) VALUES (1)")
        .await?;
    Ok(())
}

pub(crate) async fn end_suppression(conn: &impl ConnectionTrait) -> AppResult<()> {
    conn.execute_unprepared("DELETE FROM sync_capture_suppression").await?;
    Ok(())
}

/// Queue the current state of a row explicitly (by local id). Used for rows that every device
/// derives itself and that are therefore written with capture suppressed, at the moment a
/// person's decision starts to depend on them (for example the scheduling candidate a schedule
/// commitment is made for).
pub(crate) async fn capture_row_change(conn: &impl ConnectionTrait, entity_type: &str, local_id: i64) -> AppResult<()> {
    let layout = row_layout::layout(conn, entity_type).await?;
    if layout.natural_pk.is_some() {
        return Err(AppError::SyncError(format!(
            "{entity_type} is keyed by a natural key; capture it by writing the row."
        )));
    }
    let table = quote(layout.table());
    let inserted = conn
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, owner_user_sync_id)
                 SELECT ?, r.entity_sync_id, NULL, 'upsert', {owner} FROM {table} r
                  WHERE r.id = ? AND r.entity_sync_id IS NOT NULL",
                owner = owner_sql(&layout, "r"),
            ),
            [layout.def.entity_type.into(), local_id.into()],
        ))
        .await?;
    if inserted.rows_affected() == 0 {
        let exists = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("SELECT 1 AS v FROM {table} WHERE id = ?"),
                [local_id.into()],
            ))
            .await?
            .is_some();
        if !exists {
            return Err(AppError::SyncError(format!(
                "{entity_type} row {local_id} does not exist."
            )));
        }
        // A row without identity only exists where capture is not installed: nothing to send.
        tracing::debug!(
            entity_type,
            local_id,
            "sync capture not installed: explicit capture skipped"
        );
    }
    Ok(())
}

/// Journal entries not turned into outbox envelopes yet.
pub async fn pending_capture_count(conn: &impl ConnectionTrait) -> AppResult<i64> {
    conn.query_one(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) AS v FROM sync_capture_journal".to_string(),
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "v"))
    .transpose()
    .map_err(|e| AppError::SyncError(format!("journal count: {e}")))
    .map(Option::unwrap_or_default)
}

#[derive(Debug)]
struct JournalRow {
    id: i64,
    entity_type: String,
    entity_sync_id: Option<String>,
    natural_key: Option<Map<String, Value>>,
    operation: String,
    owner_user_sync_id: Option<String>,
}

#[derive(Debug)]
struct Group {
    entity_type: String,
    entity_sync_id: String,
    natural_key: Option<Map<String, Value>>,
    ids: Vec<i64>,
    owner_user_sync_id: Option<String>,
}

fn read_journal_row(row: &sea_orm::QueryResult) -> AppResult<JournalRow> {
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("sync_capture_journal: {e}"));
    let natural: Option<String> = row.try_get("", "natural_key_json").map_err(read)?;
    let natural_key = match natural {
        None => None,
        Some(text) => match serde_json::from_str::<Value>(&text)? {
            Value::Object(map) => Some(map),
            _ => return Err(AppError::SyncError("journal natural key is not an object".into())),
        },
    };
    Ok(JournalRow {
        id: row.try_get("", "id").map_err(read)?,
        entity_type: row.try_get("", "entity_type").map_err(read)?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(read)?,
        natural_key,
        operation: row.try_get("", "operation").map_err(read)?,
        owner_user_sync_id: row.try_get("", "owner_user_sync_id").map_err(read)?,
    })
}

/// Turn captured changes into outbox envelopes, one per entity, from the entity's current state.
///
/// Run it on the transaction that needs the outbox up to date (push, inbound apply). An entity
/// that cannot be staged stays in the journal and is listed as a `STAGE_ERROR` conflict; the other
/// entities are staged.
pub async fn drain_capture_journal(conn: &impl ConnectionTrait) -> AppResult<CaptureDrainReport> {
    let mut report = CaptureDrainReport::default();
    let mut after_id = 0_i64;
    loop {
        let rows = conn
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id, entity_type, entity_sync_id, natural_key_json, operation, owner_user_sync_id
                 FROM sync_capture_journal WHERE id > ? ORDER BY id LIMIT ?",
                [after_id.into(), DRAIN_PAGE_SIZE.into()],
            ))
            .await?;
        if rows.is_empty() {
            break;
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut index: HashMap<(String, String), usize> = HashMap::new();
        let mut unusable: Vec<i64> = Vec::new();
        for row in &rows {
            let j = read_journal_row(row)?;
            after_id = after_id.max(j.id);
            let identity = match (&j.entity_sync_id, &j.natural_key) {
                (Some(id), _) => id.clone(),
                (None, Some(key)) => {
                    let layout = row_layout::layout(conn, &j.entity_type).await?;
                    match row_codec::natural_entity_sync_id(&layout, key) {
                        Ok(id) => id,
                        // A delete whose key rows were removed in the same statement (cascade):
                        // the receiving devices remove it together with its parent.
                        Err(_) if j.operation == "delete" => {
                            unusable.push(j.id);
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                (None, None) => {
                    unusable.push(j.id);
                    continue;
                }
            };
            let k = (j.entity_type.clone(), identity.clone());
            match index.get(&k) {
                Some(&i) => {
                    let g = &mut groups[i];
                    g.ids.push(j.id);
                    if j.owner_user_sync_id.is_some() {
                        g.owner_user_sync_id = j.owner_user_sync_id;
                    }
                    if j.natural_key.is_some() {
                        g.natural_key = j.natural_key;
                    }
                }
                None => {
                    index.insert(k, groups.len());
                    groups.push(Group {
                        entity_type: j.entity_type,
                        entity_sync_id: identity,
                        natural_key: j.natural_key,
                        ids: vec![j.id],
                        owner_user_sync_id: j.owner_user_sync_id,
                    });
                }
            }
        }
        delete_journal_rows(conn, &unusable).await?;
        for group in groups {
            match stage_group(conn, &group).await {
                Ok(true) => {
                    report.staged += 1;
                    delete_journal_rows(conn, &group.ids).await?;
                    close_stage_failure(conn, &group, StageClose::Staged).await?;
                }
                Ok(false) => {
                    report.skipped += 1;
                    delete_journal_rows(conn, &group.ids).await?;
                    close_stage_failure(conn, &group, StageClose::NotSent).await?;
                }
                Err(e) => {
                    report.failed += 1;
                    record_stage_failure(conn, &group, &e.to_string()).await?;
                }
            }
        }
    }
    Ok(report)
}

async fn delete_journal_rows(conn: &impl ConnectionTrait, ids: &[i64]) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM sync_capture_journal WHERE id IN (SELECT value FROM json_each(?))",
        [serde_json::to_string(ids)?.into()],
    ))
    .await?;
    Ok(())
}

async fn record_stage_failure(conn: &impl ConnectionTrait, group: &Group, error: &str) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_capture_journal SET attempt_count = attempt_count + 1, last_error = ?
         WHERE id IN (SELECT value FROM json_each(?))",
        [error.into(), serde_json::to_string(&group.ids)?.into()],
    ))
    .await?;
    tracing::error!(
        entity_type = %group.entity_type,
        entity_sync_id = %group.entity_sync_id,
        error,
        "sync capture: change cannot be staged"
    );
    record_sync_conflict(
        conn,
        NewSyncConflict {
            source_scope: "outbox",
            source_batch_id: None,
            linked_outbox_id: None,
            linked_inbox_id: None,
            entity_type: group.entity_type.clone(),
            entity_sync_id: group.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            conflict_type: "STAGE_ERROR".to_string(),
            local_payload_json: Some(serde_json::json!({ "error": error }).to_string()),
            inbound_payload_json: None,
            checkpoint_token: None,
            local_row_version: None,
            inbound_row_version: None,
            detail_message: Some(error.to_string()),
        },
    )
    .await?;
    // Closed automatically by an earlier successful attempt: the new failure needs attention again.
    // A conflict a person closed keeps their decision.
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_conflicts
         SET status = 'new',
             resolution_action = NULL,
             resolved_by_id = NULL,
             resolution_note = NULL,
             resolved_at = NULL,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE source_scope = 'outbox' AND conflict_type = 'STAGE_ERROR'
           AND entity_type = ? AND entity_sync_id = ?
           AND status = 'resolved_local' AND resolution_action IN ('auto_restaged', 'auto_not_sent')",
        [group.entity_type.clone().into(), group.entity_sync_id.clone().into()],
    ))
    .await?;
    Ok(())
}

/// How a change that failed to stage earlier was settled by a later drain.
#[derive(Clone, Copy)]
enum StageClose {
    /// Staged: it is on its way to the server.
    Staged,
    /// Nothing was sent: the row is kept on this PC (outside its entity's sync row filter), or it
    /// was removed before it was ever sent. The note must not claim it was sent.
    NotSent,
}

/// A change that failed to stage earlier and is now settled (staged, or with nothing to send): its
/// open `STAGE_ERROR` conflict is closed with a note saying which, so the conflict list (and the
/// replay guard) no longer shows a problem that is gone.
async fn close_stage_failure(conn: &impl ConnectionTrait, group: &Group, how: StageClose) -> AppResult<()> {
    let (action, note) = match how {
        StageClose::Staged => ("auto_restaged", "The change was staged on a later attempt."),
        StageClose::NotSent => (
            "auto_not_sent",
            "Not sent: kept on this PC (outside synchronisation), or removed before it was ever sent.",
        ),
    };
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_conflicts
         SET status = 'resolved_local',
             resolution_action = ?,
             resolved_by_id = NULL,
             resolution_note = ?,
             resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE source_scope = 'outbox' AND conflict_type = 'STAGE_ERROR'
           AND entity_type = ? AND entity_sync_id = ?
           AND status IN ('new', 'triaged', 'escalated')",
        [
            action.into(),
            note.into(),
            group.entity_type.clone().into(),
            group.entity_sync_id.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// True when the server or the outbox already knows the entity, so its disappearance must be sent.
async fn known_beyond_this_device(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS v WHERE EXISTS (SELECT 1 FROM sync_entity_state WHERE entity_type = ? AND entity_sync_id = ?)
                OR EXISTS (SELECT 1 FROM sync_outbox WHERE entity_type = ? AND entity_sync_id = ? AND status <> 'superseded')",
            [
                entity_type.into(),
                entity_sync_id.into(),
                entity_type.into(),
                entity_sync_id.into(),
            ],
        ))
        .await?
        .is_some())
}

async fn locate(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    natural_key: Option<&Map<String, Value>>,
) -> AppResult<Option<RowLocator>> {
    if layout.natural_pk.is_none() {
        return Ok(Some(RowLocator::Identity(entity_sync_id.to_string())));
    }
    match natural_key {
        Some(key) => row_codec::natural_locator(conn, layout, key).await,
        None => Ok(None),
    }
}

/// Stage the current state of one entity. `Ok(false)` when there is nothing to send.
async fn stage_group(conn: &impl ConnectionTrait, group: &Group) -> AppResult<bool> {
    let layout = row_layout::layout(conn, &group.entity_type).await?;
    stage_current_state(
        conn,
        &layout,
        &group.entity_sync_id,
        group.natural_key.as_ref(),
        group.owner_user_sync_id.clone(),
    )
    .await
}

async fn stage_current_state(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    natural_key: Option<&Map<String, Value>>,
    delete_owner: Option<String>,
) -> AppResult<bool> {
    // A row its capture filter keeps on this device (`row_spec` row filters) is never staged: not
    // by the capture drain, not by "keep mine" on a conflict about it. That includes a row that has
    // just left its filter (the update trigger journals it as it leaves): nothing is sent for it,
    // neither its new state nor a delete, so other PCs keep the last version they received.
    if layout.natural_pk.is_none() {
        let local_only = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT 1 AS x FROM {} r WHERE r.entity_sync_id = ? AND NOT {}",
                    quote(layout.table()),
                    layout.filter_sql("r")
                ),
                [entity_sync_id.into()],
            ))
            .await?
            .is_some();
        if local_only {
            return Ok(false);
        }
    }
    let current = match locate(conn, layout, entity_sync_id, natural_key).await? {
        Some(locator) => row_codec::load_row_payload(conn, layout, &locator).await?,
        None => None,
    };
    let (operation, payload, owner) = match current {
        Some(row) => {
            if row.entity_sync_id != entity_sync_id {
                return Err(AppError::SyncError(format!(
                    "{} {entity_sync_id}: the row now has identity {}",
                    layout.def.entity_type, row.entity_sync_id
                )));
            }
            ("upsert", row.payload_json()?, row.owner_user_sync_id)
        }
        None => {
            if !known_beyond_this_device(conn, layout.def.entity_type, entity_sync_id).await? {
                return Ok(false);
            }
            (
                "delete",
                serde_json::to_string(&row_codec::delete_payload(layout, natural_key))?,
                delete_owner,
            )
        }
    };
    if layout.def.scope == SyncScope::User && owner.is_none() {
        return Err(AppError::SyncError(format!(
            "{} {entity_sync_id} is user-scoped but its owner is unknown.",
            layout.def.entity_type
        )));
    }
    stage_outbox_item_with_owner(
        conn,
        StageOutboxItemInput {
            entity_type: layout.def.entity_type.to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: operation.to_string(),
            payload_json: payload,
        },
        owner,
    )
    .await?;
    Ok(true)
}

/// Stage the current local state of an entity again ("keep mine" after a conflict): the row, or a
/// delete when the row is gone. Natural-key rows are found through the conflict's payloads.
pub(crate) async fn restage_current_state(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    payload_hints: &[Option<&str>],
) -> AppResult<()> {
    drain_capture_journal(conn).await?;
    let layout = row_layout::layout(conn, entity_type).await?;
    let mut natural_key = None;
    if layout.natural_pk.is_some() {
        for hint in payload_hints.iter().flatten() {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(hint) {
                if row_codec::natural_entity_sync_id(&layout, &map).ok().as_deref() == Some(entity_sync_id) {
                    natural_key = Some(map);
                    break;
                }
            }
        }
        if natural_key.is_none() {
            return Err(AppError::SyncError(format!(
                "{entity_type} {entity_sync_id}: the conflict carries no natural key to find the local row."
            )));
        }
    }
    let owner = match layout.def.scope {
        SyncScope::Tenant => None,
        SyncScope::User => conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT owner_user_sync_id AS v FROM sync_outbox
                 WHERE entity_type = ? AND entity_sync_id = ? AND owner_user_sync_id IS NOT NULL
                 ORDER BY id DESC LIMIT 1",
                [entity_type.into(), entity_sync_id.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<Option<String>>("", "v").ok().flatten()),
    };
    stage_current_state(conn, &layout, entity_sync_id, natural_key.as_ref(), owner).await?;
    Ok(())
}
