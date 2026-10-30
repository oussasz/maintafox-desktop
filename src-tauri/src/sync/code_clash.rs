//! Number clashes between devices (Phase 2 leftover 2).
//!
//! A record received from another device whose human number (`OT-0012`, `DI-0007`, `PER-0003`,
//! `PTW-…`) is already used here by a *different* record cannot be stored (the number is unique)
//! and must never be renamed silently. It becomes a `CODE_CLASH` conflict in the Sync Center,
//! naming the number and both records. A person resolves it with "renumber the record on this
//! device" (`renumber_local_record`): the local record gets a new number of this device
//! (`sync::device_code`), that change is sent like any edit, and the received record is applied
//! with its own number. Numbers created before device codes existed are the usual source; new
//! numbers carry the device code and do not clash unless two device codes are equal. PM plan codes
//! are typed by people, so two PCs can give two plans the same code: such a clash is handled the
//! same way, the plan on this device getting a `PM-<device>-<sequence>` code when renumbered.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::SyncConflictRecord;
use crate::sync::materialize::ApplyOutcome;

/// Conflict type of a number already used here by another record.
pub const CODE_CLASH: &str = "CODE_CLASH";

/// A table whose rows carry a number generated on the device that creates them.
#[derive(Debug, Clone, Copy)]
pub struct NumberedTable {
    pub entity_type: &'static str,
    pub table: &'static str,
    pub column: &'static str,
    /// Prefix of new numbers (`OT` → `OT-<device>-0001`); permits add the year.
    prefix: fn() -> String,
    /// Digits of the sequence of new numbers.
    width: usize,
}

impl NumberedTable {
    pub fn prefix(&self) -> String {
        (self.prefix)()
    }

    /// Next number of this device for this table. Pass the transaction that writes the row.
    pub async fn next_number(&self, conn: &impl ConnectionTrait) -> AppResult<String> {
        crate::sync::device_code::next_device_number(conn, self.table, self.column, &self.prefix(), self.width).await
    }
}

fn work_order_prefix() -> String {
    "OT".to_string()
}
fn request_prefix() -> String {
    "DI".to_string()
}
fn personnel_prefix() -> String {
    "PER".to_string()
}
fn pm_plan_prefix() -> String {
    "PM".to_string()
}
fn permit_prefix() -> String {
    format!("PTW-{}", crate::clock::now().format("%Y"))
}

pub const WORK_ORDERS: NumberedTable = NumberedTable {
    entity_type: "work_orders",
    table: "work_orders",
    column: "code",
    prefix: work_order_prefix,
    width: 4,
};
pub const INTERVENTION_REQUESTS: NumberedTable = NumberedTable {
    entity_type: "intervention_requests",
    table: "intervention_requests",
    column: "code",
    prefix: request_prefix,
    width: 4,
};
pub const PERSONNEL: NumberedTable = NumberedTable {
    entity_type: "personnel",
    table: "personnel",
    column: "employee_code",
    prefix: personnel_prefix,
    width: 4,
};
pub const WORK_PERMITS: NumberedTable = NumberedTable {
    entity_type: "work_permits",
    table: "work_permits",
    column: "code",
    prefix: permit_prefix,
    width: 6,
};

/// PM plan codes are typed by people (unique on each PC); renumbering gives a device code.
pub const PM_PLANS: NumberedTable = NumberedTable {
    entity_type: "pm_plans",
    table: "pm_plans",
    column: "code",
    prefix: pm_plan_prefix,
    width: 4,
};

pub const NUMBERED_TABLES: &[NumberedTable] = &[WORK_ORDERS, INTERVENTION_REQUESTS, PERSONNEL, WORK_PERMITS, PM_PLANS];

pub fn numbered_table(entity_type: &str) -> Option<&'static NumberedTable> {
    NUMBERED_TABLES.iter().find(|t| t.entity_type == entity_type)
}

/// The number carried by an inbound payload (`row.v1`: columns by name).
fn inbound_number(table: &NumberedTable, payload_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()?
        .get(table.column)?
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

/// The local record (local id, sync id) that holds `number`, other than `entity_sync_id`.
async fn local_holder(
    conn: &impl ConnectionTrait,
    table: &NumberedTable,
    number: &str,
    entity_sync_id: &str,
) -> AppResult<Option<(i64, Option<String>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT id, entity_sync_id FROM {} WHERE {} = ? AND (entity_sync_id IS NULL OR entity_sync_id <> ?) LIMIT 1",
                table.table, table.column
            ),
            [number.into(), entity_sync_id.into()],
        ))
        .await?;
    row.map(|r| {
        Ok((
            r.try_get::<i64>("", "id")
                .map_err(|e| AppError::SyncError(format!("{}.id: {e}", table.table)))?,
            r.try_get::<Option<String>>("", "entity_sync_id")
                .map_err(|e| AppError::SyncError(format!("{}.entity_sync_id: {e}", table.table)))?,
        ))
    })
    .transpose()
}

/// Before an inbound create or update is applied: `Some(CODE_CLASH)` when its number is used
/// here by another record.
pub(crate) async fn check_inbound(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    operation: &str,
    payload_json: &str,
) -> AppResult<Option<ApplyOutcome>> {
    if operation == "delete" || operation == "archive" {
        return Ok(None);
    }
    let Some(table) = numbered_table(entity_type) else {
        return Ok(None);
    };
    let Some(number) = inbound_number(table, payload_json) else {
        return Ok(None);
    };
    let Some((local_id, local_sync_id)) = local_holder(conn, table, &number, entity_sync_id).await? else {
        return Ok(None);
    };
    Ok(Some(ApplyOutcome::Conflict {
        code: CODE_CLASH.to_string(),
        message: format!(
            "Number {number} is already used on this device by another {entity_type} record ({}). \
             Renumber the record on this device to receive this one.",
            local_sync_id.unwrap_or_else(|| format!("local id {local_id}"))
        ),
    }))
}

/// After a received version of a numbered record was applied: earlier `CODE_CLASH` conflicts of
/// that record are no longer true (the record was renumbered on its device and its newer version
/// applied here), so they are closed and their inbox rows superseded. No data is changed.
pub(crate) async fn close_outdated_clashes(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    applied_row_version: i64,
) -> AppResult<()> {
    if numbered_table(entity_type).is_none() {
        return Ok(());
    }
    close_conflicts_superseded_by(
        conn,
        CODE_CLASH,
        entity_type,
        entity_sync_id,
        applied_row_version,
        "The record was renumbered on its device; its newer version was applied.",
    )
    .await
}

/// Closes the open `conflict_type` conflicts of a record older than the version just applied here
/// (`resolved_remote`, `newer_version_applied`, with `note`) and supersedes their inbox rows. Used
/// for the clashes a newer version removes: a number (`CODE_CLASH`), a username
/// (`sync::username_clash`). No data is changed.
pub(crate) async fn close_conflicts_superseded_by(
    conn: &impl ConnectionTrait,
    conflict_type: &str,
    entity_type: &str,
    entity_sync_id: &str,
    applied_row_version: i64,
    note: &str,
) -> AppResult<()> {
    // Usually there is none: look them up first (a small table) so a received row costs one
    // indexed read, never a scan of the inbox.
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, linked_inbox_id FROM sync_conflicts
             WHERE conflict_type = ? AND entity_type = ? AND entity_sync_id = ?
               AND inbound_row_version < ?
               AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed')",
            [
                conflict_type.into(),
                entity_type.into(),
                entity_sync_id.into(),
                applied_row_version.into(),
            ],
        ))
        .await?;
    for row in rows {
        let conflict_id: i64 = row
            .try_get("", "id")
            .map_err(|e| AppError::SyncError(format!("sync_conflicts.id: {e}")))?;
        let inbox_id: Option<i64> = row
            .try_get("", "linked_inbox_id")
            .map_err(|e| AppError::SyncError(format!("sync_conflicts.linked_inbox_id: {e}")))?;
        if let Some(inbox_id) = inbox_id {
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_inbox SET apply_status = 'superseded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [inbox_id.into()],
            ))
            .await?;
        }
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_conflicts
             SET status = 'resolved_remote', resolution_action = 'newer_version_applied',
                 resolution_note = ?,
                 resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [note.into(), conflict_id.into()],
        ))
        .await?;
    }
    Ok(())
}

async fn has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// What a renumbering changed, for the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumbered {
    pub entity_type: &'static str,
    pub old_number: String,
    pub new_number: String,
    pub local_entity_sync_id: Option<String>,
}

/// "Renumber the record on this device": give the local holder of the clashing number a new
/// number of this device, then apply the received record. Runs in the transaction that closes the
/// conflict. `None` when the number was already freed meanwhile (only the apply was needed).
pub(crate) async fn renumber_local_record(
    conn: &impl ConnectionTrait,
    conflict: &SyncConflictRecord,
) -> AppResult<Option<Renumbered>> {
    if conflict.conflict_type != CODE_CLASH {
        return Err(AppError::ValidationFailed(vec![
            "GATE_SYNC_RENUMBER_NOT_CLASH: Renumbering is only available for a number clash.".to_string(),
        ]));
    }
    let table = numbered_table(&conflict.entity_type).ok_or_else(|| {
        AppError::ValidationFailed(vec![format!(
            "GATE_SYNC_ENTITY_NO_NUMBER:entity={0}: '{0}' records have no generated number.",
            conflict.entity_type
        )])
    })?;
    let number = conflict
        .inbound_payload_json
        .as_deref()
        .and_then(|p| inbound_number(table, p))
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_SYNC_RECEIVED_NO_NUMBER: The received record carries no number.".to_string(),
            ])
        })?;
    // The received record may already be here in a newer version (renumbered on its own device
    // meanwhile): applying this older copy would bring the clashing number back.
    if let (Some((applied, _)), Some(inbound)) = (
        crate::sync::queries::synced_row_version(conn, &conflict.entity_type, &conflict.entity_sync_id).await?,
        conflict.inbound_row_version,
    ) {
        if applied >= inbound {
            crate::sync::conflicts::set_linked_inbox_status(conn, conflict, "superseded").await?;
            return Ok(None);
        }
    }
    let renamed = match local_holder(conn, table, &number, &conflict.entity_sync_id).await? {
        None => None,
        Some((local_id, local_sync_id)) => {
            let next = table.next_number(conn).await?;
            let touch = if has_column(conn, table.table, "updated_at").await? {
                ", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')"
            } else {
                ""
            };
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!(
                    "UPDATE {} SET {} = ?, row_version = row_version + 1{touch} WHERE id = ?",
                    table.table, table.column
                ),
                [next.clone().into(), local_id.into()],
            ))
            .await?;
            Some(Renumbered {
                entity_type: table.entity_type,
                old_number: number,
                new_number: next,
                local_entity_sync_id: local_sync_id,
            })
        }
    };
    crate::sync::conflicts::take_remote_version(conn, conflict).await?;
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::domain::{ResolveSyncConflictInput, SyncConflictFilter};
    use crate::sync::queries::{list_sync_conflicts, resolve_sync_conflict};
    use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, TestServer};

    async fn add_person(db: &sea_orm::DatabaseConnection, esid: &str, code: &str) {
        exec(
            db,
            "INSERT INTO personnel (entity_sync_id, employee_code, full_name, employment_type, availability_status, row_version)
             VALUES (?, ?, 'Clash test', 'employee', 'available', 1)",
            vec![esid.into(), code.into()],
        )
        .await;
    }

    async fn code_of(db: &sea_orm::DatabaseConnection, esid: &str) -> Option<String> {
        scalar_opt_text(
            db,
            "SELECT employee_code FROM personnel WHERE entity_sync_id = ?",
            vec![esid.into()],
        )
        .await
    }

    async fn add_pm_plan(db: &sea_orm::DatabaseConnection, esid: &str, code: &str) {
        exec(
            db,
            "INSERT INTO pm_plans (entity_sync_id, code, title, asset_scope_type, strategy_type, row_version)
             VALUES (?, ?, 'Clash test plan', 'equipment', 'fixed', 1)",
            vec![esid.into(), code.into()],
        )
        .await;
    }

    async fn plan_code_of(db: &sea_orm::DatabaseConnection, esid: &str) -> Option<String> {
        scalar_opt_text(
            db,
            "SELECT code FROM pm_plans WHERE entity_sync_id = ?",
            vec![esid.into()],
        )
        .await
    }

    #[tokio::test]
    async fn a_pm_plan_code_typed_on_two_pcs_is_a_number_clash() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        add_pm_plan(&a, "plan-from-a", "PM-POMPE-01").await;
        add_pm_plan(&b, "plan-from-b", "PM-POMPE-01").await;
        server.exchange("A", &a).await;
        let report = server.exchange("B", &b).await;
        assert_eq!(report.materialized.conflicts, 1, "{report:?}");
        let conflicts = list_sync_conflicts(
            &b,
            SyncConflictFilter {
                statuses: None,
                conflict_type: Some(CODE_CLASH.to_string()),
                requires_operator_review: None,
                limit: Some(10),
            },
        )
        .await
        .expect("conflicts");
        assert_eq!(conflicts.len(), 1, "a coded clash, not a MATERIALIZE_ERROR");
        let clash = &conflicts[0];
        assert_eq!(clash.entity_type, "pm_plans");
        assert_eq!(clash.recommended_action, "renumber_local");

        let person = scalar_i64(&b, "SELECT id FROM user_accounts ORDER BY id LIMIT 1", vec![]).await;
        resolve_sync_conflict(
            &b,
            person,
            ResolveSyncConflictInput {
                conflict_id: clash.id,
                expected_row_version: clash.row_version,
                action: "renumber_local".to_string(),
                resolution_note: Some("Two different plans; renumber ours".to_string()),
            },
        )
        .await
        .expect("renumber");
        let dev_b = crate::sync::device_code::this_device_code(&b).await.expect("code");
        assert_eq!(plan_code_of(&b, "plan-from-b").await, Some(format!("PM-{dev_b}-0001")));
        assert_eq!(plan_code_of(&b, "plan-from-a").await.as_deref(), Some("PM-POMPE-01"));

        server.exchange("B", &b).await;
        let report = server.exchange("A", &a).await;
        assert_eq!(report.materialized.conflicts, 0, "{report:?}");
        assert_eq!(plan_code_of(&a, "plan-from-b").await, Some(format!("PM-{dev_b}-0001")));
    }

    #[tokio::test]
    async fn new_numbers_differ_between_devices_and_ignore_old_ones() {
        let a = started_device_db().await;
        let b = started_device_db().await;
        add_person(&a, "p-old", "PER-0009").await;
        let on_a = PERSONNEL.next_number(&a).await.expect("number on A");
        let on_b = PERSONNEL.next_number(&b).await.expect("number on B");
        assert_ne!(on_a, on_b, "two devices never hand out the same number");
        assert!(
            on_a.ends_with("-0001"),
            "old numbers do not feed the new sequence: {on_a}"
        );
        let dev_a = crate::sync::device_code::this_device_code(&a).await.expect("code");
        assert_eq!(on_a, format!("PER-{dev_a}-0001"));
    }

    #[tokio::test]
    async fn a_clash_is_shown_then_renumbered_by_a_person() {
        let mut server = TestServer::new();
        let a = started_device_db().await;
        let b = started_device_db().await;
        // Numbered alike on two devices before device codes existed.
        add_person(&a, "person-from-a", "PER-0007").await;
        add_person(&b, "person-from-b", "PER-0007").await;
        server.exchange("A", &a).await;
        let report = server.exchange("B", &b).await;
        assert_eq!(report.materialized.conflicts, 1, "{report:?}");

        let conflicts = list_sync_conflicts(
            &b,
            SyncConflictFilter {
                statuses: None,
                conflict_type: Some(CODE_CLASH.to_string()),
                requires_operator_review: None,
                limit: Some(10),
            },
        )
        .await
        .expect("conflicts");
        assert_eq!(conflicts.len(), 1);
        let clash = &conflicts[0];
        assert_eq!(clash.entity_sync_id, "person-from-a");
        assert_eq!(clash.recommended_action, "renumber_local");
        // Shown, never renamed silently.
        assert_eq!(code_of(&b, "person-from-b").await.as_deref(), Some("PER-0007"));
        assert_eq!(code_of(&b, "person-from-a").await, None);

        let person = scalar_i64(&b, "SELECT id FROM user_accounts ORDER BY id LIMIT 1", vec![]).await;
        let closed = resolve_sync_conflict(
            &b,
            person,
            ResolveSyncConflictInput {
                conflict_id: clash.id,
                expected_row_version: clash.row_version,
                action: "renumber_local".to_string(),
                resolution_note: Some("Both people exist; renumber ours".to_string()),
            },
        )
        .await
        .expect("renumber");
        assert_eq!(closed.status, "merged");
        let dev_b = crate::sync::device_code::this_device_code(&b).await.expect("code");
        assert_eq!(code_of(&b, "person-from-b").await, Some(format!("PER-{dev_b}-0001")));
        assert_eq!(code_of(&b, "person-from-a").await.as_deref(), Some("PER-0007"));
        let history = scalar_i64(
            &b,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'sync.code_clash.renumbered'",
            vec![],
        )
        .await;
        assert_eq!(history, 1);
        // The choice itself is audited with its reason, as every conflict choice.
        let choice = scalar_i64(
            &b,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'sync.conflict_resolved' \
             AND json_extract(details_json, '$.action') = 'renumber_local' \
             AND json_extract(details_json, '$.reason') = 'Both people exist; renumber ours'",
            vec![],
        )
        .await;
        assert_eq!(choice, 1);

        // The new number travels like any edit; A ends with both records, no conflict.
        server.exchange("B", &b).await;
        let report = server.exchange("A", &a).await;
        assert_eq!(report.materialized.conflicts, 0, "{report:?}");
        assert_eq!(code_of(&a, "person-from-b").await, Some(format!("PER-{dev_b}-0001")));
        assert_eq!(code_of(&a, "person-from-a").await.as_deref(), Some("PER-0007"));
    }
}
