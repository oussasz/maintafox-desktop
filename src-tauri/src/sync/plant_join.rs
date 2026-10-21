//! Joining the plant before the first administrator (CMMS Advisor decision 2026-10-21).
//!
//! A new PC of an existing plant used to offer "create the first administrator" right after its
//! activation: that screen read only the local database, and the first pull had not run yet. Two
//! PCs then held two first administrators, often with the same username. Now the first
//! administrator may be created only once a complete pull has confirmed that the organisation has
//! no user account on the server ([`server_confirmed_no_accounts`]); the first-start screen
//! (`commands::product_license::get_activation_bootstrap_state`) and the creation itself
//! (`bootstrap_initial_tenant_admin`) both read it.
//!
//! The fact "the server confirmed no user accounts for organisation X":
//! - is recorded by the sync worker after a cycle whose exchanges reached the server head
//!   (`has_more` false, the outbox drained, after the scope `all` snapshot or from the delta
//!   checkpoint), when this PC then holds no user account and none waits in its inbox
//!   ([`record_complete_pull`]); a complete pull that brought accounts erases it;
//! - names the organisation the pull ran for (tenant id of the activation): a fact recorded for
//!   another organisation counts for nothing;
//! - lives in `system_config`, which is local to this PC and never synchronised;
//! - stops counting as soon as a user account exists here or arrives in the inbox (checked on
//!   every read), is consumed by the creation of the first administrator ([`forget`], in the same
//!   transaction), and is erased by a local data reset, which keeps `system_config` but wipes the
//!   accounts and the sync checkpoint.
//!
//! The dev-only built-in "admin" account (seeded identity, `auth::builtin_admin`) is never counted:
//! release builds do not create it, and in a development build every PC has it.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::Serialize;

use crate::auth::builtin_admin::{builtin_admin_entity_sync_id, BUILTIN_ADMIN_USERNAME};
use crate::errors::AppResult;
use crate::sync::transport::worker::{SyncWorkerState, SyncWorkerStatus, BLOCKER_NOT_ACTIVATED};

/// `system_config` key of the fact; its value is the tenant id the complete pull ran for.
pub const SERVER_CONFIRMED_NO_ACCOUNTS_KEY: &str = "server_confirmed_no_user_accounts";

/// Whether this PC holds a user account, or has received one (inbox, any status: a row waiting,
/// refused or superseded still means the server has accounts). The dev built-in account is left
/// out, by its seeded identity and its username together (`auth::builtin_admin`).
async fn accounts_present(conn: &impl ConnectionTrait) -> AppResult<bool> {
    let builtin_sync_id = builtin_admin_entity_sync_id();
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT EXISTS (
                     SELECT 1 FROM user_accounts
                      WHERE NOT (username = ? AND COALESCE(entity_sync_id, '') = ?)
                 )
                 OR EXISTS (
                     SELECT 1 FROM sync_inbox
                      WHERE entity_type = 'user_accounts' AND entity_sync_id <> ?
                 ) AS present",
            [
                BUILTIN_ADMIN_USERNAME.into(),
                builtin_sync_id.clone().into(),
                builtin_sync_id.into(),
            ],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "present")).transpose()?.unwrap_or(0) == 1)
}

/// Called by the sync worker once a pull for `tenant_id` is complete (all snapshot pages stored,
/// exchanges caught up with the server head, received rows materialized). Records the fact when
/// this PC holds no user account and received none; erases it otherwise. Returns whether the fact
/// holds now.
pub(crate) async fn record_complete_pull(conn: &impl ConnectionTrait, tenant_id: &str) -> AppResult<bool> {
    if tenant_id.trim().is_empty() || accounts_present(conn).await? {
        forget(conn).await?;
        return Ok(false);
    }
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO system_config (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [
            SERVER_CONFIRMED_NO_ACCOUNTS_KEY.into(),
            tenant_id.into(),
            Utc::now().to_rfc3339().into(),
        ],
    ))
    .await?;
    Ok(true)
}

/// True when a complete pull for `tenant_id` confirmed that the organisation has no user account on
/// the server, and no account has been created here or received since.
pub async fn server_confirmed_no_accounts(conn: &impl ConnectionTrait, tenant_id: &str) -> AppResult<bool> {
    let recorded = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = ?",
            [SERVER_CONFIRMED_NO_ACCOUNTS_KEY.into()],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "value"))
        .transpose()?;
    if recorded.as_deref() != Some(tenant_id) || tenant_id.trim().is_empty() {
        return Ok(false);
    }
    Ok(!accounts_present(conn).await?)
}

/// Erases the fact: the first administrator has been created (caller's transaction), or a complete
/// pull brought accounts. A local data reset deletes the same row in its own wipe transaction
/// (`product_license::reset_local_tenant_runtime_data_impl`).
pub(crate) async fn forget(conn: &impl ConnectionTrait) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM system_config WHERE key = ?",
        [SERVER_CONFIRMED_NO_ACCOUNTS_KEY.into()],
    ))
    .await?;
    Ok(())
}

/// What the first-start screen of an activated PC shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantJoinState {
    /// The plant has an administrator here, or accounts arrived from the server: sign in.
    SignIn,
    /// A complete pull confirmed that the organisation has no account: the first administrator
    /// may be created.
    CreateFirstAdmin,
    /// The pull is running or about to run ("Joining your plant… (synchronising)").
    Joining,
    /// The pull cannot run or failed (offline, server error, sync refused or not allowed): no
    /// first administrator; "A connection is needed to join your plant", with a retry.
    ConnectionNeeded,
}

/// The first-start screen's state from what this PC holds and what the sync worker reports.
/// `administered`: `product_license::plant_has_administrator`.
pub async fn plant_join_state(
    conn: &impl ConnectionTrait,
    tenant_id: &str,
    administered: bool,
    worker: &SyncWorkerStatus,
) -> AppResult<PlantJoinState> {
    if administered || accounts_present(conn).await? {
        return Ok(PlantJoinState::SignIn);
    }
    if server_confirmed_no_accounts(conn, tenant_id).await? {
        return Ok(PlantJoinState::CreateFirstAdmin);
    }
    Ok(join_progress(worker))
}

/// Joining or not, from the worker status, while the fact is not recorded. "Not activated" is the
/// worker's state until it re-reads the activation that has just been made: the screen asks it to
/// run at once (`request_plant_join`), so that is still joining. Every other blocker, an offline,
/// retrying or failed cycle, and a pause, mean the pull cannot complete now.
fn join_progress(worker: &SyncWorkerStatus) -> PlantJoinState {
    match worker.state {
        SyncWorkerState::Starting | SyncWorkerState::Running | SyncWorkerState::Idle => PlantJoinState::Joining,
        SyncWorkerState::Blocked if worker.blocker.as_deref() == Some(BLOCKER_NOT_ACTIVATED) => PlantJoinState::Joining,
        SyncWorkerState::Blocked
        | SyncWorkerState::Offline
        | SyncWorkerState::Retrying
        | SyncWorkerState::Error
        | SyncWorkerState::Paused => PlantJoinState::ConnectionNeeded,
    }
}

/// Tests that create a first administrator without running a pull: records the fact exactly as a
/// complete pull with no account would (`record_complete_pull`), and fails the test if this PC
/// holds accounts.
#[cfg(test)]
pub(crate) async fn confirm_empty_plant_for_tests(conn: &impl ConnectionTrait, tenant_id: &str) {
    assert!(
        record_complete_pull(conn, tenant_id)
            .await
            .expect("record the complete pull"),
        "this PC holds user accounts: a complete pull would not confirm an empty plant"
    );
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    use super::*;
    use crate::sync::domain::{ApplySyncBatchInput, SyncInboundItemInput, SYNC_PROTOCOL_VERSION_V1};
    use crate::sync::transport::SyncWorkerHandle;

    const PLANT: &str = "tenant-plant-a";

    fn worker_in(state: SyncWorkerState, blocker: Option<&str>) -> SyncWorkerStatus {
        let mut status = SyncWorkerHandle::new().status();
        status.state = state;
        status.blocker = blocker.map(str::to_string);
        status
    }

    #[tokio::test]
    async fn the_fact_names_one_organisation_and_ends_when_an_account_arrives() {
        let db = crate::db::test_db::seeded().await;
        assert!(!server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        // The dev built-in "admin" of the seed does not count.
        assert!(record_complete_pull(&db, PLANT).await.expect("record"));
        assert!(server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        assert!(
            !server_confirmed_no_accounts(&db, "tenant-other").await.expect("fact"),
            "recorded for another organisation"
        );

        // An account received, even one that is not applied yet, ends it.
        crate::sync::queries::apply_sync_batch(
            &db,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
                server_batch_id: "b1".into(),
                checkpoint_token: "ckpt-b1".into(),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: vec![SyncInboundItemInput {
                    entity_type: "user_accounts".into(),
                    entity_sync_id: "acct-other-pc".into(),
                    operation: "upsert".into(),
                    row_version: 1,
                    payload_json: serde_json::json!({ "username": "chef" }).to_string(),
                    ..Default::default()
                }],
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
        assert!(!server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        // The next complete pull erases it.
        assert!(!record_complete_pull(&db, PLANT).await.expect("record"));
        let stored = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM system_config WHERE key = ?",
                [SERVER_CONFIRMED_NO_ACCOUNTS_KEY.into()],
            ))
            .await
            .expect("query")
            .map(|r| r.try_get::<i64>("", "c").expect("count"));
        assert_eq!(stored, Some(0));
    }

    #[tokio::test]
    async fn an_account_created_here_ends_the_fact() {
        let db = crate::db::test_db::seeded().await;
        assert!(record_complete_pull(&db, PLANT).await.expect("record"));
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
                 force_password_change, created_at, updated_at)
             VALUES ('acct-here', 'technicien', 'Technicien', 'local', 1, 0, 0, 'now', 'now')",
        ))
        .await
        .expect("account");
        assert!(!server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        let worker = worker_in(SyncWorkerState::Idle, None);
        assert_eq!(
            plant_join_state(&db, PLANT, false, &worker).await.expect("state"),
            PlantJoinState::SignIn
        );
    }

    #[test]
    fn joining_or_connection_needed_follows_the_worker() {
        use SyncWorkerState as S;
        for (state, blocker, expected) in [
            (S::Starting, None, PlantJoinState::Joining),
            (S::Running, None, PlantJoinState::Joining),
            (S::Idle, None, PlantJoinState::Joining),
            (S::Blocked, Some(BLOCKER_NOT_ACTIVATED), PlantJoinState::Joining),
            (S::Blocked, Some("not_licensed"), PlantJoinState::ConnectionNeeded),
            (S::Blocked, Some("activation_refused"), PlantJoinState::ConnectionNeeded),
            (S::Offline, None, PlantJoinState::ConnectionNeeded),
            (S::Retrying, None, PlantJoinState::ConnectionNeeded),
            (S::Error, None, PlantJoinState::ConnectionNeeded),
            (S::Paused, None, PlantJoinState::ConnectionNeeded),
        ] {
            assert_eq!(
                join_progress(&worker_in(state, blocker)),
                expected,
                "{state:?} {blocker:?}"
            );
        }
    }
}
