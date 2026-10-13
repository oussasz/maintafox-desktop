//! Migration 166 — Number clashes already recorded as technical errors become `CODE_CLASH`.
//!
//! Before device codes (`sync::device_code`), two devices could number two different work
//! orders, requests, people or permits alike. Applying the second one failed on the unique number
//! and was recorded as an unreadable `MATERIALIZE_ERROR` conflict. Those open conflicts are
//! re-typed as `CODE_CLASH` so the Sync Center explains them and offers "renumber the record on
//! this device" (`sync::code_clash`). Nothing is renamed here.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000166_sync_code_clash_conflicts"
    }
}

/// `(entity_type, SQLite unique-violation text of its number column)`.
const NUMBER_COLUMNS: &[(&str, &str)] = &[
    ("work_orders", "UNIQUE constraint failed: work_orders.code"),
    (
        "intervention_requests",
        "UNIQUE constraint failed: intervention_requests.code",
    ),
    ("personnel", "UNIQUE constraint failed: personnel.employee_code"),
    ("work_permits", "UNIQUE constraint failed: work_permits.code"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (entity_type, failure) in NUMBER_COLUMNS {
            let clashing_inbox = format!(
                "SELECT id FROM sync_inbox WHERE entity_type = '{entity_type}' AND apply_status = 'conflict'
                   AND rejection_code = 'MATERIALIZE_ERROR' AND rejection_message LIKE '%{failure}%'"
            );
            db.execute_unprepared(&format!(
                "UPDATE sync_conflicts
                 SET conflict_type = 'CODE_CLASH', recommended_action = 'renumber_local', authority_side = 'mixed',
                     conflict_key = REPLACE(conflict_key, ':MATERIALIZE_ERROR:', ':CODE_CLASH:'),
                     row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE conflict_type = 'MATERIALIZE_ERROR' AND entity_type = '{entity_type}'
                   AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed')
                   AND linked_inbox_id IN ({clashing_inbox})"
            ))
            .await?;
            db.execute_unprepared(&format!(
                "UPDATE sync_inbox SET rejection_code = 'CODE_CLASH' WHERE id IN ({clashing_inbox})"
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
