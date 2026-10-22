//! The received rows that wait, and what each waits for (0.9.0 test finding 4, rules 2 and 3).
//!
//! A received row that cannot be applied yet is kept `deferred` in the inbox and retried on every
//! synchronisation (`sync::materialize`). The sync centre never shows how many wait without why:
//! [`list_waiting_inbox_rows`] names, for each waiting row, the row it waits for and where that row
//! stands here, following a chain of waiting rows to its first link:
//! - [`WAIT_CONFLICT`]: the row it waits for (or the first row of the chain) is a sync conflict
//!   still open; it waits for that conflict to be resolved;
//! - [`WAIT_REFUSED`]: that row was refused here (its conflict was closed without applying it);
//! - [`WAIT_NOT_RECEIVED`]: that row has not arrived yet;
//! - [`WAIT_READY`]: that row is here now; the waiting row is applied at the next synchronisation;
//! - [`WAIT_OWN_REASON`]: it waits for something other than one row (an app update, a right, a
//!   person, a rule): its own reason code says what (`sync::materialize::DEFER_*`);
//! - [`WAIT_CHAIN_UNRESOLVED`]: the chain of rows waiting for each other loops, or is longer than
//!   this list follows: named at the row where it stopped.
//!
//! Groups count every waiting row by what it waits for; the rows themselves are listed up to a
//! limit, oldest first.

use std::collections::{BTreeMap, HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

pub const WAIT_CONFLICT: &str = "conflict";
pub const WAIT_REFUSED: &str = "refused";
pub const WAIT_NOT_RECEIVED: &str = "not_received";
pub const WAIT_READY: &str = "ready";
pub const WAIT_OWN_REASON: &str = "own_reason";
pub const WAIT_CHAIN_UNRESOLVED: &str = "chain_unresolved";

/// A chain of waiting rows longer than this is reported at the link reached.
const MAX_CHAIN: usize = 32;

/// What a waiting row waits for, as far as this PC knows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WaitTarget {
    /// One of the `WAIT_*` states.
    pub state: String,
    /// The open conflict it waits for ([`WAIT_CONFLICT`]).
    pub conflict_id: Option<i64>,
    /// The row at the end of the chain (the conflicted, refused, missing or ready row), or the
    /// waiting row of the chain that waits for something else ([`WAIT_OWN_REASON`]).
    pub entity_type: Option<String>,
    pub entity_sync_id: Option<String>,
    /// [`WAIT_OWN_REASON`]: the reason code of that row (`MISSING_PARENT`, `MISSING_FIELD`, …).
    pub reason_code: Option<String>,
}

/// One received row that waits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitingInboxRow {
    pub inbox_id: i64,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub payload_json: String,
    /// Why it was deferred (`sync::materialize::DEFER_*`).
    pub reason_code: Option<String>,
    /// The text recorded when it was deferred (technical details).
    pub detail_message: Option<String>,
    /// The row it waits for directly, when it waits for one.
    pub waits_for_entity_type: Option<String>,
    pub waits_for_sync_id: Option<String>,
    pub target: WaitTarget,
    pub attempt_count: i64,
    pub last_attempt_at: Option<String>,
}

/// How many rows wait for the same thing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitingGroup {
    pub target: WaitTarget,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitingInboxReport {
    /// Every waiting row.
    pub total: i64,
    /// Every waiting row, counted by what it waits for (largest group first).
    pub groups: Vec<WaitingGroup>,
    /// The oldest waiting rows, up to the limit asked.
    pub rows: Vec<WaitingInboxRow>,
}

fn decode(field: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("waiting rows {field}: {e}"))
}

struct Deferred {
    id: i64,
    entity_type: String,
    entity_sync_id: String,
    reason_code: Option<String>,
    waits_for: Option<(String, String)>,
}

fn deferred_from(r: &sea_orm::QueryResult) -> AppResult<Deferred> {
    let waits_type: Option<String> = r
        .try_get("", "waits_for_entity_type")
        .map_err(|e| decode("waits_for_entity_type", e))?;
    let waits_id: Option<String> = r
        .try_get("", "waits_for_sync_id")
        .map_err(|e| decode("waits_for_sync_id", e))?;
    Ok(Deferred {
        id: r.try_get("", "id").map_err(|e| decode("id", e))?,
        entity_type: r.try_get("", "entity_type").map_err(|e| decode("entity_type", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode("entity_sync_id", e))?,
        reason_code: r
            .try_get("", "rejection_code")
            .map_err(|e| decode("rejection_code", e))?,
        waits_for: waits_type.zip(waits_id),
    })
}

fn own_reason(entity_type: &str, entity_sync_id: &str, reason_code: Option<String>) -> WaitTarget {
    WaitTarget {
        state: WAIT_OWN_REASON.to_string(),
        conflict_id: None,
        entity_type: Some(entity_type.to_string()),
        entity_sync_id: Some(entity_sync_id.to_string()),
        reason_code,
    }
}

fn at(state: &str, entity_type: &str, entity_sync_id: &str) -> WaitTarget {
    WaitTarget {
        state: state.to_string(),
        conflict_id: None,
        entity_type: Some(entity_type.to_string()),
        entity_sync_id: Some(entity_sync_id.to_string()),
        reason_code: None,
    }
}

/// Whether the row is in its table here (applied, or created on this PC).
async fn present_here(conn: &impl ConnectionTrait, entity_type: &str, entity_sync_id: &str) -> AppResult<bool> {
    let Some(def) = crate::sync::registry::entity(entity_type) else {
        return Ok(false);
    };
    // Rows keyed by a natural key have no sync id column: they are never waited for by one.
    let keyed = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = 'entity_sync_id'",
            [def.table.into()],
        ))
        .await?
        .is_some();
    if !keyed {
        return Ok(false);
    }
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT 1 AS present FROM \"{}\" WHERE entity_sync_id = ? LIMIT 1",
                def.table
            ),
            [entity_sync_id.into()],
        ))
        .await?
        .is_some())
}

/// Where the row `(entity_type, entity_sync_id)` stands here, following its own wait.
async fn resolve(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    cache: &mut HashMap<(String, String), WaitTarget>,
) -> AppResult<WaitTarget> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut current = (entity_type.to_string(), entity_sync_id.to_string());
    let target = loop {
        if let Some(known) = cache.get(&current) {
            break known.clone();
        }
        if !seen.insert(current.clone()) || chain.len() >= MAX_CHAIN {
            // A loop of rows waiting for each other, or a very long chain: said so, at this link.
            break at(WAIT_CHAIN_UNRESOLVED, &current.0, &current.1);
        }
        chain.push(current.clone());
        let (et, sid) = (&current.0, &current.1);
        let latest = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id, entity_type, entity_sync_id, apply_status, rejection_code,
                        waits_for_entity_type, waits_for_sync_id
                 FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ?
                 ORDER BY id DESC LIMIT 1",
                [et.clone().into(), sid.clone().into()],
            ))
            .await?;
        let Some(row) = latest else {
            let state = if present_here(conn, et, sid).await? {
                WAIT_READY
            } else {
                WAIT_NOT_RECEIVED
            };
            break at(state, et, sid);
        };
        let status: String = row.try_get("", "apply_status").map_err(|e| decode("apply_status", e))?;
        match status.as_str() {
            "deferred" => {
                let d = deferred_from(&row)?;
                match d.waits_for {
                    Some(next) => current = next,
                    None => break own_reason(et, sid, d.reason_code),
                }
            }
            "conflict" => {
                let inbox_id: i64 = row.try_get("", "id").map_err(|e| decode("id", e))?;
                let open = conn
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT id FROM sync_conflicts
                         WHERE linked_inbox_id = ? AND status IN ('new', 'triaged', 'escalated')
                         ORDER BY id DESC LIMIT 1",
                        [inbox_id.into()],
                    ))
                    .await?
                    .map(|r| r.try_get::<i64>("", "id"))
                    .transpose()
                    .map_err(|e| decode("conflict id", e))?;
                break match open {
                    Some(conflict_id) => WaitTarget {
                        conflict_id: Some(conflict_id),
                        ..at(WAIT_CONFLICT, et, sid)
                    },
                    None => at(WAIT_REFUSED, et, sid),
                };
            }
            "rejected" => break at(WAIT_REFUSED, et, sid),
            "pending" => break at(WAIT_READY, et, sid),
            // Applied or superseded: here unless removed since.
            _ => {
                let state = if present_here(conn, et, sid).await? {
                    WAIT_READY
                } else {
                    WAIT_NOT_RECEIVED
                };
                break at(state, et, sid);
            }
        }
    };
    for link in chain {
        cache.insert(link, target.clone());
    }
    Ok(target)
}

/// What one waiting row waits for.
async fn target_of(
    conn: &impl ConnectionTrait,
    row: &Deferred,
    cache: &mut HashMap<(String, String), WaitTarget>,
) -> AppResult<WaitTarget> {
    match &row.waits_for {
        Some((et, sid)) => resolve(conn, et, sid, cache).await,
        None => Ok(own_reason(
            &row.entity_type,
            &row.entity_sync_id,
            row.reason_code.clone(),
        )),
    }
}

/// Every waiting received row counted by what it waits for, and the `limit` oldest of them listed.
pub async fn list_waiting_inbox_rows(conn: &impl ConnectionTrait, limit: Option<i64>) -> AppResult<WaitingInboxReport> {
    let limit = usize::try_from(limit.unwrap_or(200).clamp(1, 1000)).unwrap_or(200);
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type, entity_sync_id, rejection_code, waits_for_entity_type, waits_for_sync_id
             FROM sync_inbox WHERE apply_status = 'deferred' ORDER BY id ASC"
                .to_string(),
        ))
        .await?;
    let mut cache = HashMap::new();
    let mut groups: BTreeMap<WaitTarget, i64> = BTreeMap::new();
    let mut listed: Vec<(i64, WaitTarget)> = Vec::new();
    for r in &rows {
        let d = deferred_from(r)?;
        let target = target_of(conn, &d, &mut cache).await?;
        // Rows that wait for different missing rows of one type are one group ("not received yet").
        let mut group_key = target.clone();
        if group_key.state == WAIT_NOT_RECEIVED || group_key.state == WAIT_READY {
            group_key.entity_sync_id = None;
        }
        if group_key.state == WAIT_OWN_REASON {
            group_key.entity_type = None;
            group_key.entity_sync_id = None;
        }
        *groups.entry(group_key).or_insert(0) += 1;
        if listed.len() < limit {
            listed.push((d.id, target));
        }
    }
    let mut groups: Vec<WaitingGroup> = groups
        .into_iter()
        .map(|(target, count)| WaitingGroup { target, count })
        .collect();
    groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.target.cmp(&b.target)));

    let mut out = Vec::with_capacity(listed.len());
    for (id, target) in listed {
        let Some(r) = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id, entity_type, entity_sync_id, payload_json, rejection_code, rejection_message,
                        waits_for_entity_type, waits_for_sync_id, attempt_count, last_attempt_at
                 FROM sync_inbox WHERE id = ?",
                [id.into()],
            ))
            .await?
        else {
            continue;
        };
        out.push(WaitingInboxRow {
            inbox_id: id,
            entity_type: r.try_get("", "entity_type").map_err(|e| decode("entity_type", e))?,
            entity_sync_id: r
                .try_get("", "entity_sync_id")
                .map_err(|e| decode("entity_sync_id", e))?,
            payload_json: r.try_get("", "payload_json").map_err(|e| decode("payload_json", e))?,
            reason_code: r
                .try_get("", "rejection_code")
                .map_err(|e| decode("rejection_code", e))?,
            detail_message: r
                .try_get("", "rejection_message")
                .map_err(|e| decode("rejection_message", e))?,
            waits_for_entity_type: r
                .try_get("", "waits_for_entity_type")
                .map_err(|e| decode("waits_for_entity_type", e))?,
            waits_for_sync_id: r
                .try_get("", "waits_for_sync_id")
                .map_err(|e| decode("waits_for_sync_id", e))?,
            target,
            attempt_count: r.try_get("", "attempt_count").map_err(|e| decode("attempt_count", e))?,
            last_attempt_at: r
                .try_get("", "last_attempt_at")
                .map_err(|e| decode("last_attempt_at", e))?,
        });
    }
    Ok(WaitingInboxReport {
        total: i64::try_from(rows.len()).unwrap_or(i64::MAX),
        groups,
        rows: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::apply_reason::REQUIRED_MISSING;
    use crate::sync::domain::SyncConflictFilter;
    use crate::sync::queries::{hash_payload, list_sync_conflicts};
    use crate::sync::test_support::{scalar_opt_text, started_device_db, RowFactory, TestServer};

    /// On A: a PM execution, with its occurrence, plan version and plan. Returns the sync ids of
    /// the version and the occurrence.
    async fn pm_chain(a: &sea_orm::DatabaseConnection) -> (String, String, String) {
        let mut factory = RowFactory::for_device(1);
        let execution = factory.create(a, "pm_executions").await.expect("pm execution");
        let occurrence = scalar_opt_text(
            a,
            "SELECT o.entity_sync_id FROM pm_executions e JOIN pm_occurrences o ON o.id = e.pm_occurrence_id
             WHERE e.entity_sync_id = ?",
            vec![execution.entity_sync_id.clone().into()],
        )
        .await
        .expect("occurrence");
        let version = scalar_opt_text(
            a,
            "SELECT v.entity_sync_id FROM pm_occurrences o JOIN pm_plan_versions v ON v.id = o.plan_version_id
             WHERE o.entity_sync_id = ?",
            vec![occurrence.clone().into()],
        )
        .await
        .expect("version");
        (version, occurrence, execution.entity_sync_id)
    }

    fn rewrite_on_server(
        server: &mut TestServer,
        entity_type: &str,
        sync_id: &str,
        edit: impl FnOnce(&mut serde_json::Value),
    ) {
        let row = server
            .state
            .get_mut(&(entity_type.to_string(), sync_id.to_string()))
            .expect("row on the server");
        let mut payload: serde_json::Value = serde_json::from_str(&row.payload_json).expect("payload");
        edit(&mut payload);
        row.payload_json = payload.to_string();
        row.payload_hash = hash_payload(&row.payload_json);
    }

    #[tokio::test]
    async fn a_refused_version_shows_its_reason_and_the_rows_waiting_for_it() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let (version, occurrence, execution) = pm_chain(&a).await;
        server.exchange("A", &a).await;
        // Old data of the sending PC: a version without its start date.
        rewrite_on_server(&mut server, "pm_plan_versions", &version, |p| {
            p["effective_from"] = serde_json::Value::Null;
        });
        server.exchange("B", &b).await;

        let conflicts = list_sync_conflicts(
            &b,
            SyncConflictFilter {
                conflict_type: Some("MATERIALIZE_ERROR".into()),
                ..SyncConflictFilter::default()
            },
        )
        .await
        .expect("conflicts");
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        let refused = &conflicts[0];
        assert_eq!(refused.entity_sync_id, version);
        assert!(
            refused
                .detail_message
                .as_deref()
                .is_some_and(|m| m.contains("NOT NULL constraint failed: pm_plan_versions.effective_from")),
            "the real error is kept with the conflict: {:?}",
            refused.detail_message
        );
        let reason = refused.reason.as_ref().expect("plain reason");
        assert_eq!(reason.code, REQUIRED_MISSING);
        assert_eq!(reason.columns, vec!["effective_from"]);

        let report = list_waiting_inbox_rows(&b, None).await.expect("waiting rows");
        for waiting in [&occurrence, &execution] {
            let row = report
                .rows
                .iter()
                .find(|r| &r.entity_sync_id == waiting)
                .unwrap_or_else(|| panic!("{waiting} is listed: {report:?}"));
            assert_eq!(row.target.state, WAIT_CONFLICT, "{row:?}");
            assert_eq!(row.target.conflict_id, Some(refused.id));
            assert_eq!(row.target.entity_sync_id.as_deref(), Some(version.as_str()));
        }
        // The occurrence waits for the version itself; the execution waits for the occurrence.
        let execution_row = report
            .rows
            .iter()
            .find(|r| r.entity_sync_id == execution)
            .expect("listed");
        assert_eq!(execution_row.waits_for_sync_id.as_deref(), Some(occurrence.as_str()));
        // No count without its reason: every waiting row is in a group.
        assert_eq!(report.groups.iter().map(|g| g.count).sum::<i64>(), report.total);
        assert!(report
            .groups
            .iter()
            .any(|g| g.target.conflict_id == Some(refused.id) && g.count >= 2));
    }

    #[tokio::test]
    async fn a_row_whose_parent_has_not_arrived_says_so() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        let (version, occurrence, _) = pm_chain(&a).await;
        server.exchange("A", &a).await;
        server
            .state
            .remove(&("pm_plan_versions".to_string(), version.clone()))
            .expect("version on the server");
        server.exchange("B", &b).await;

        let report = list_waiting_inbox_rows(&b, None).await.expect("waiting rows");
        let row = report
            .rows
            .iter()
            .find(|r| r.entity_sync_id == occurrence)
            .expect("occurrence listed");
        assert_eq!(row.reason_code.as_deref(), Some("MISSING_PARENT"));
        assert_eq!(row.target.state, WAIT_NOT_RECEIVED);
        assert_eq!(row.target.entity_type.as_deref(), Some("pm_plan_versions"));
        assert_eq!(row.target.entity_sync_id.as_deref(), Some(version.as_str()));
    }

    #[tokio::test]
    async fn rows_waiting_for_each_other_are_not_called_missing() {
        let db = crate::sync::test_support::started_db().await;
        for (id, waits) in [("loop-a", "loop-b"), ("loop-b", "loop-a")] {
            crate::sync::test_support::exec(
                &db,
                "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                                         row_version, payload_json, payload_hash, apply_status, rejection_code,
                                         waits_for_entity_type, waits_for_sync_id)
                 VALUES ('b', 'c', 'pm_occurrences', ?, 'upsert', 1, '{}', ?, 'deferred', 'MISSING_PARENT',
                         'pm_occurrences', ?)",
                vec![id.into(), id.into(), waits.into()],
            )
            .await;
        }
        let report = list_waiting_inbox_rows(&db, None).await.expect("waiting rows");
        assert_eq!(report.rows.len(), 2);
        for row in &report.rows {
            assert_eq!(row.target.state, WAIT_CHAIN_UNRESOLVED, "{row:?}");
        }
    }
}
