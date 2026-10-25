//! Migration 262 — who created or last changed a delegated administration policy.
//!
//! A delegation policy lets the holders of a role manage access in a scope: a policy created or
//! changed on another PC gives rights here. The receiving PC checks that the person who made it
//! could (`sync::grant_check`, the right of the screen: `adm.roles`), so the row names them:
//! `changed_by_id`, a reference to `user_accounts` (sent as `changed_by_sync_id`; soft in
//! `sync::row_spec`: an author not applied yet never holds the policy back as a parent, the check
//! makes it wait for that author instead). The time is the row's own `updated_at`.
//!
//! Existing rows keep a NULL author ("not recorded"): no author is guessed from the admin ledger.
//! A later change names its author. A PC that downloads such a row during its first complete pull
//! (a new or rebuilt PC) applies it and lists it for the administrators (`sync::grant_check`);
//! saving the policy again on a PC that holds it records its author. An already-present column is
//! left alone (a re-run changes nothing).
//!
//! Also: a PC already in service (it holds a sync checkpoint, so its first download finished under
//! an earlier version) records that a complete pull is done (`sync::plant_join::
//! COMPLETE_PULL_DONE_KEY`, value `before-0.9.2`), so a new account received with the first
//! administrator flag stays refused there from the first cycle after the upgrade
//! (`sync::grant_check`). A new PC (no checkpoint) records it at its own first complete pull.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261025_000262_delegation_policy_author"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, the part guarded.
        let txn = manager.get_connection().begin().await?;
        let present = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present FROM pragma_table_info('delegated_admin_policies') WHERE name = 'changed_by_id'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if !present {
            txn.execute_unprepared(
                "ALTER TABLE delegated_admin_policies ADD COLUMN changed_by_id INTEGER NULL REFERENCES user_accounts(id)",
            )
            .await?;
        }
        let synced_before = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = 'sync_checkpoint'".to_string(),
            ))
            .await?
            .is_some();
        if synced_before {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR IGNORE INTO system_config (key, value, updated_at) \
                 SELECT ?, 'before-0.9.2', strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                  WHERE EXISTS (SELECT 1 FROM sync_checkpoint \
                                 WHERE id = 1 AND COALESCE(trim(checkpoint_token), '') <> '')",
                [crate::sync::plant_join::COMPLETE_PULL_DONE_KEY.into()],
            ))
            .await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    use crate::sync::row_layout::{Role, Target};
    use crate::sync::test_support::{exec, scalar_i64, started_db, started_db_before_migration};

    #[tokio::test]
    async fn an_upgraded_database_keeps_its_policies_and_sends_the_author_as_an_account_link() {
        let db = started_db_before_migration("m20261025_000262_delegation_policy_author").await;
        exec(
            &db,
            "INSERT INTO delegated_admin_policies (entity_sync_id, admin_role_id, managed_scope_type, \
               allowed_domains_json, requires_step_up_for_publish) \
             SELECT 'legacy-policy', id, 'tenant', '[\"inv\"]', 1 FROM roles ORDER BY id LIMIT 1",
            vec![],
        )
        .await;
        let count = "SELECT COUNT(*) FROM delegated_admin_policies";
        let before = scalar_i64(&db, count, vec![]).await;
        crate::migrations::Migrator::up(&db, None).await.expect("migration 262");
        assert_eq!(scalar_i64(&db, count, vec![]).await, before, "policies kept");
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM delegated_admin_policies WHERE entity_sync_id = 'legacy-policy' \
                   AND changed_by_id IS NULL AND allowed_domains_json = '[\"inv\"]'",
                vec![],
            )
            .await,
            1,
            "existing policy unchanged, author not recorded"
        );
        // Run again: nothing changes.
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migration 262 again");

        let db = started_db().await;
        let layout = crate::sync::row_layout::layout(&db, "delegated_admin_policies")
            .await
            .expect("layout");
        let found = layout
            .columns
            .iter()
            .find(|c| c.name == "changed_by_id")
            .expect("changed_by_id in the layout");
        match &found.role {
            Role::Ref {
                key,
                target: Target::Entity { entity_type, .. },
                soft,
                ..
            } => {
                assert_eq!(key, "changed_by_sync_id");
                assert_eq!(*entity_type, "user_accounts");
                assert!(*soft, "an author not here yet never holds the policy back");
            }
            other => panic!("changed_by_id must travel as an account link, got {other:?}"),
        }
    }

    /// A PC in service (sync checkpoint held) records that a complete pull is done; a new PC
    /// (no checkpoint token) does not.
    #[tokio::test]
    async fn a_pc_in_service_records_its_complete_pull() {
        let done = "SELECT COUNT(*) FROM system_config WHERE key = ?";
        for (token, expected) in [(Some("cp-123"), 1), (None, 0)] {
            let db = started_db_before_migration("m20261025_000262_delegation_policy_author").await;
            exec(
                &db,
                "DELETE FROM system_config WHERE key = ?",
                vec![crate::sync::plant_join::COMPLETE_PULL_DONE_KEY.into()],
            )
            .await;
            exec(
                &db,
                "INSERT INTO sync_checkpoint (id, checkpoint_token) VALUES (1, ?) \
                 ON CONFLICT(id) DO UPDATE SET checkpoint_token = excluded.checkpoint_token",
                vec![token.map(str::to_string).into()],
            )
            .await;
            crate::migrations::Migrator::up(&db, None).await.expect("migration 262");
            assert_eq!(
                scalar_i64(&db, done, vec![crate::sync::plant_join::COMPLETE_PULL_DONE_KEY.into()]).await,
                expected,
                "checkpoint {token:?}"
            );
        }
    }
}
