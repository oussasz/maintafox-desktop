//! Migration 253 — PM plan code clashes already recorded as technical errors become `CODE_CLASH`
//! (0.9.0 test finding 4, rule 4).
//!
//! PM plan codes are typed by people: two PCs can give two different plans the same code. Applying
//! the second one failed on the unique code and was recorded as an unreadable `MATERIALIZE_ERROR`
//! conflict. Those open conflicts are re-typed as `CODE_CLASH`, as migration 166 did for work
//! orders, requests, people and permits, so the sync centre explains them and offers "renumber the
//! record on this device" (`sync::code_clash::PM_PLANS`). Nothing is renamed here; closed
//! conflicts and every other row are left as they are.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000253_pm_plan_code_clash"
    }
}

const CLASHING_INBOX: &str = "SELECT id FROM sync_inbox WHERE entity_type = 'pm_plans' AND apply_status = 'conflict'
   AND rejection_code = 'MATERIALIZE_ERROR' AND rejection_message LIKE '%UNIQUE constraint failed: pm_plans.code%'";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(&format!(
            "UPDATE sync_conflicts
             SET conflict_type = 'CODE_CLASH', recommended_action = 'renumber_local', authority_side = 'mixed',
                 conflict_key = REPLACE(conflict_key, ':MATERIALIZE_ERROR:', ':CODE_CLASH:'),
                 row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE conflict_type = 'MATERIALIZE_ERROR' AND entity_type = 'pm_plans'
               AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed')
               AND linked_inbox_id IN ({CLASHING_INBOX})"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "UPDATE sync_inbox SET rejection_code = 'CODE_CLASH' WHERE id IN ({CLASHING_INBOX})"
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
