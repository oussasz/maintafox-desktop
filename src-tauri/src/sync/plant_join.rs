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
//! - stops counting as soon as a user account exists here or is received (inbox or sync conflict;
//!   checked on every read), is consumed by the creation of the first administrator ([`forget`], in
//!   the same transaction), and is erased by a local data reset, which keeps `system_config` but
//!   wipes the accounts and the sync checkpoint.
//!
//! The dev-only built-in "admin" account (seeded identity, `auth::builtin_admin`) is never counted:
//! release builds do not create it, and in a development build every PC has it.
//!
//! A licence without the `sync` capability is a single-PC plant: there is no plant to join and no
//! pull will ever run, so the first administrator may be created at once ([`licence_syncs`]). That
//! is read from the licence's capability map (the entitlement source the worker's "not licensed"
//! comes from), never from the worker status, so a transient status cannot open the creation; a
//! licence with sync that is suspended, revoked or expired still needs the confirmed pull, and so
//! does a no-sync envelope older than the current activation (it may belong to an earlier one).

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;

use crate::auth::builtin_admin::{builtin_admin_entity_sync_id, BUILTIN_ADMIN_USERNAME};
use crate::errors::AppResult;
use crate::sync::transport::worker::{SyncWorkerState, SyncWorkerStatus, BLOCKER_NOT_ACTIVATED};

/// `system_config` key of the fact; its value is the tenant id the complete pull ran for.
pub const SERVER_CONFIRMED_NO_ACCOUNTS_KEY: &str = "server_confirmed_no_user_accounts";

/// Whether the licence includes sync, i.e. this PC belongs to a plant it must join before a first
/// administrator is created (`entitlements::queries::licence_includes_capability`, whatever the
/// entitlement state). `false`, a single-PC plant, only when the envelope in force says so and was
/// verified and applied here at or after the current activation's key entry (same second
/// included): envelopes carry no organisation and are not cleared when the activation changes, so
/// an envelope left by an earlier activation must not open the creation on a PC re-activated into
/// a plant that syncs. Every doubt (no envelope, unverified, an unreadable time, no key entry)
/// counts as syncing: the confirmed pull is then required, which a fresh envelope lifts.
pub async fn licence_syncs(db: &DatabaseConnection) -> AppResult<bool> {
    if crate::entitlements::queries::licence_includes_capability(db, "sync").await? {
        return Ok(true);
    }
    let verified_at = crate::entitlements::queries::active_envelope_verified_at(db).await?;
    let entered_at = crate::commands::product_license::activation_submitted_at(db).await?;
    let seconds = |at: Option<String>| {
        at.and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
            .map(|at| at.timestamp())
    };
    Ok(match (seconds(verified_at), seconds(entered_at)) {
        // `verified_at` is stored to the second; the key entry time is truncated to match.
        (Some(verified), Some(entered)) => verified < entered,
        _ => true,
    })
}

/// Whether this PC holds a user account, or has received one: in the inbox (any status: a row
/// waiting, refused or superseded still means the server has accounts) or only as a sync conflict
/// (a received row with an invalid payload or hash is recorded in `sync_conflicts` and never
/// reaches the inbox, `queries::ingest_inbound_items`). The dev built-in account is left out, by
/// its seeded identity and its username together (`auth::builtin_admin`).
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
                 )
                 OR EXISTS (
                     SELECT 1 FROM sync_conflicts
                      WHERE entity_type = 'user_accounts' AND entity_sync_id <> ?
                 ) AS present",
            [
                BUILTIN_ADMIN_USERNAME.into(),
                builtin_sync_id.clone().into(),
                builtin_sync_id.clone().into(),
                builtin_sync_id.into(),
            ],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "present")).transpose()?.unwrap_or(0) == 1)
}

/// `system_config` key: this PC has completed a full pull (its first join is over); its value is the
/// tenant id the pull ran for. Unlike [`SERVER_CONFIRMED_NO_ACCOUNTS_KEY`] it is never erased by a
/// later pull (accounts or not): only a local data reset deletes it, which makes the PC join again.
/// A snapshot received once it is recorded (a re-bootstrap after the server refused the checkpoint)
/// is not a first join (`inventory::count_four_eyes`, CMMS Advisor Q9).
pub const COMPLETE_PULL_DONE_KEY: &str = "sync_complete_pull_done";

/// Whether this PC has completed a full pull of the organisation it is activated for now, since it
/// joined (or since its last local data reset). A pull recorded for another organisation (the PC
/// re-activated into another one without a local data reset) is not: that organisation's first
/// snapshot here is its first join. Not activated: none.
pub(crate) async fn complete_pull_done(conn: &impl ConnectionTrait) -> AppResult<bool> {
    let Some(tenant_id) = crate::commands::product_license::get_activation_claim_tenant_id(conn).await? else {
        return Ok(false);
    };
    let recorded = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = ?",
            [COMPLETE_PULL_DONE_KEY.into()],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "value"))
        .transpose()?;
    Ok(!tenant_id.trim().is_empty() && recorded.as_deref() == Some(tenant_id.as_str()))
}

/// Whether a row of `entity_type` new here, received with a snapshot, may be taken as the plant's
/// history of this PC's first join, already judged where it was made (CMMS Advisor Q9, review of
/// 536cb792): only while this PC has not completed a full pull of its organisation
/// ([`complete_pull_done`]) AND nothing of that row is held here — no open sync conflict and no
/// version waiting or refused from an exchange. A snapshot asked for again later (a re-bootstrap
/// after the server refused the checkpoint) is no first join: its rows are judged as any other.
/// The caller checks that the row is new here and that a snapshot row is being applied
/// (`sync::materialize::applying_snapshot_row`). Shared by `inventory::count_four_eyes` (count
/// lines) and `wo::closeout_sync` (work orders).
pub(crate) async fn first_join_history(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
    if complete_pull_done(conn).await? {
        return Ok(false);
    }
    let held = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT 1 AS h WHERE EXISTS (
                     SELECT 1 FROM sync_conflicts WHERE entity_type = ? AND entity_sync_id = ?
                        AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed'))
                 OR EXISTS (
                     SELECT 1 FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ?
                        AND apply_status IN ('deferred', 'conflict')
                        AND server_batch_id NOT LIKE '{}%')",
                crate::sync::materialize::SNAPSHOT_BATCH_PREFIX
            ),
            [
                entity_type.into(),
                entity_sync_id.into(),
                entity_type.into(),
                entity_sync_id.into(),
            ],
        ))
        .await?
        .is_some();
    Ok(!held)
}

/// Whether the received row being applied (new here: the caller checks it) is the plant's history
/// of this PC's first join, not judged again by the work-order rules that skip it: a snapshot row
/// is being applied (`sync::materialize::applying_snapshot_row`), this PC is activated, and
/// [`first_join_history`] holds (no complete pull of its organisation, nothing of that row held
/// here). A snapshot received again once the PC has joined (a re-bootstrap after the server refused
/// its checkpoint) is judged as any received change. Fail safe: a PC with no activation judges.
/// Used by `wo::closeout_sync` (completion, close, verifier rules), `permit::sync_check` (the
/// permit start gate) and `sync::apply_hooks` (the started-without-rule finding).
pub(crate) async fn first_join_snapshot_row(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
    if !crate::sync::materialize::applying_snapshot_row() {
        return Ok(false);
    }
    let activated = crate::commands::product_license::get_activation_claim_tenant_id(conn)
        .await?
        .is_some_and(|tenant| !tenant.trim().is_empty());
    if !activated {
        return Ok(false);
    }
    first_join_history(conn, entity_type, entity_sync_id).await
}

/// Called by the sync worker once a pull for `tenant_id` is complete (all snapshot pages stored,
/// exchanges caught up with the server head, received rows materialized). Records that a complete
/// pull was done ([`COMPLETE_PULL_DONE_KEY`]), then the fact when this PC holds no user account and
/// received none; erases the fact otherwise. Returns whether the fact holds now.
pub(crate) async fn record_complete_pull(conn: &impl ConnectionTrait, tenant_id: &str) -> AppResult<bool> {
    if !tenant_id.trim().is_empty() {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO system_config (key, value, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            [
                COMPLETE_PULL_DONE_KEY.into(),
                tenant_id.into(),
                Utc::now().to_rfc3339().into(),
            ],
        ))
        .await?;
    }
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
    /// A complete pull confirmed that the organisation has no account, or the licence has no sync
    /// (a single-PC plant): the first administrator may be created.
    CreateFirstAdmin,
    /// The pull is running or about to run ("Joining your plant… (synchronising)").
    Joining,
    /// The pull cannot run or failed (offline, server error, sync refused or not allowed): no
    /// first administrator; "A connection is needed to join your plant", with a retry.
    ConnectionNeeded,
}

/// The first-start screen's state from what this PC holds and what the sync worker reports.
/// `administered`: `product_license::plant_has_administrator`; `licence_syncs`: [`licence_syncs`]
/// (without sync there is no plant to join).
pub async fn plant_join_state(
    conn: &impl ConnectionTrait,
    tenant_id: &str,
    administered: bool,
    licence_syncs: bool,
    worker: &SyncWorkerStatus,
) -> AppResult<PlantJoinState> {
    if administered || accounts_present(conn).await? {
        return Ok(PlantJoinState::SignIn);
    }
    if !licence_syncs || server_confirmed_no_accounts(conn, tenant_id).await? {
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
        crate::commands::product_license::record_activation_claim_for_tests(&db, PLANT, "fp-1", "token-1")
            .await
            .expect("claim");
        assert!(!server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        assert!(!complete_pull_done(&db).await.expect("done"), "not joined yet");
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
        // The next complete pull erases it, and records that a complete pull was done (kept).
        assert!(complete_pull_done(&db).await.expect("done"));
        assert!(!record_complete_pull(&db, PLANT).await.expect("record"));
        assert!(
            complete_pull_done(&db).await.expect("done"),
            "a pull that brought accounts is still a complete pull"
        );
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

    /// Review of 93817231 (NOTE 1): the complete pull is recorded for one organisation; a PC
    /// re-activated into another one without a local data reset has not pulled that one yet.
    #[tokio::test]
    async fn a_complete_pull_counts_only_for_the_organisation_it_was_made_for() {
        let db = crate::db::test_db::seeded().await;
        assert!(record_complete_pull(&db, PLANT).await.expect("record"));
        assert!(!complete_pull_done(&db).await.expect("done"), "not activated: none");
        crate::commands::product_license::record_activation_claim_for_tests(&db, PLANT, "fp-1", "token-1")
            .await
            .expect("claim");
        assert!(complete_pull_done(&db).await.expect("done"), "the organisation pulled");
        crate::commands::product_license::record_activation_claim_for_tests(&db, "tenant-other", "fp-1", "token-1")
            .await
            .expect("claim");
        assert!(
            !complete_pull_done(&db).await.expect("done"),
            "re-activated into another organisation: its first pull is still to come"
        );
        record_complete_pull(&db, "tenant-other").await.expect("record");
        assert!(complete_pull_done(&db).await.expect("done"), "now pulled");
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
            plant_join_state(&db, PLANT, false, true, &worker).await.expect("state"),
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

    // ── Single-PC licence (no sync): no plant to join ────────────────────────────────────────

    /// A PC activated, then given a signed licence with these capabilities and this state (as a key
    /// entry applies the envelope its claim carries); no pull yet.
    /// A signed test envelope of this plant's licence.
    fn plant_envelope(capabilities_json: &str, state: &str) -> crate::entitlements::domain::EntitlementEnvelopeInput {
        use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG};
        use crate::entitlements::test_support::{now_rfc3339, sign_envelope};
        let mut envelope = EntitlementEnvelopeInput {
            envelope_id: "env-plant-join".into(),
            previous_envelope_id: None,
            lineage_version: 1,
            issuer: "maintafox-vps".into(),
            key_id: crate::entitlements::trusted_keys::TEST_ENTITLEMENT_KEY_ID.into(),
            signature_alg: ENTITLEMENT_SIGNATURE_ALG.into(),
            tier: "standard".into(),
            state: state.into(),
            channel: "stable".into(),
            machine_slots: 1,
            feature_flags_json: "{}".into(),
            capabilities_json: capabilities_json.into(),
            policy_json: "{}".into(),
            issued_at: now_rfc3339(),
            valid_from: now_rfc3339(),
            valid_until: "2099-01-01T00:00:00Z".into(),
            offline_grace_until: "2099-01-03T00:00:00Z".into(),
            signature: String::new(),
        };
        envelope.signature = sign_envelope(&envelope);
        envelope
    }

    async fn licensed_pc(capabilities_json: &str, state: &str) -> sea_orm::DatabaseConnection {
        licensed_pc_with(plant_envelope(capabilities_json, state)).await
    }

    async fn licensed_pc_with(
        envelope: crate::entitlements::domain::EntitlementEnvelopeInput,
    ) -> sea_orm::DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        crate::commands::product_license::record_activation_claim_for_tests(&db, PLANT, "fp-1", "token-1")
            .await
            .expect("claim");
        crate::license::security::ensure_default_licensing_trust_keys(&db)
            .await
            .expect("trust keys");
        crate::entitlements::queries::apply_entitlement_envelope(&db, envelope)
            .await
            .expect("envelope");
        db
    }

    async fn join_state_of(db: &sea_orm::DatabaseConnection, worker: &SyncWorkerStatus) -> Option<PlantJoinState> {
        crate::commands::product_license::get_activation_bootstrap_state_in(db, worker)
            .await
            .expect("bootstrap state")
            .plant_join
    }

    async fn create_first_admin(db: &sea_orm::DatabaseConnection) -> AppResult<()> {
        crate::commands::product_license::bootstrap_initial_tenant_admin_in(
            db,
            crate::commands::product_license::BootstrapInitialAdminInput {
                username: "chef".into(),
                email: "chef@plant.example".into(),
                password: "Chef-Usine-2026!Sur".into(),
                display_name: None,
            },
        )
        .await
    }

    fn code_of(err: &crate::errors::AppError) -> Option<&str> {
        match err {
            crate::errors::AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.as_str()),
            _ => None,
        }
    }

    /// A licence without sync is a single-PC plant: the first administrator is offered and created
    /// at once, without any pull, whatever the worker reports.
    #[tokio::test]
    async fn a_single_pc_licence_creates_the_first_administrator_without_a_pull() {
        let db = licensed_pc(r#"{"sync":false,"equipment":true}"#, "active").await;
        assert!(!licence_syncs(&db).await.expect("licence"));
        for worker in [
            worker_in(SyncWorkerState::Blocked, Some("not_licensed")),
            worker_in(SyncWorkerState::Starting, None),
        ] {
            assert_eq!(
                join_state_of(&db, &worker).await,
                Some(PlantJoinState::CreateFirstAdmin)
            );
        }
        create_first_admin(&db)
            .await
            .expect("single-PC plant: no plant to join");
        assert_eq!(
            join_state_of(&db, &worker_in(SyncWorkerState::Idle, None)).await,
            Some(PlantJoinState::SignIn)
        );
    }

    /// A licence with sync keeps the Advisor rule (complete pull confirmed empty), also while the
    /// entitlement is suspended: the worker then reports "not licensed", which never opens the gate.
    #[tokio::test]
    async fn a_licence_with_sync_needs_the_confirmed_pull_even_when_suspended() {
        for (capabilities, state) in [
            (r#"{"sync":true}"#, "active"),
            // Not listed: included, as `check_entitlement_capability` reads it.
            (r#"{"equipment":true}"#, "active"),
            (r#"{"sync":true}"#, "suspended"),
        ] {
            let db = licensed_pc(capabilities, state).await;
            assert!(licence_syncs(&db).await.expect("licence"), "{capabilities} {state}");
            let usable = crate::entitlements::queries::check_entitlement_capability(&db, "sync".into())
                .await
                .expect("capability")
                .allowed;
            assert_eq!(usable, state == "active", "{state}: the worker's not_licensed source");
            let worker = if usable {
                worker_in(SyncWorkerState::Offline, None)
            } else {
                worker_in(SyncWorkerState::Blocked, Some("not_licensed"))
            };
            assert_eq!(
                join_state_of(&db, &worker).await,
                Some(PlantJoinState::ConnectionNeeded),
                "{capabilities} {state}"
            );
            let err = create_first_admin(&db).await.expect_err("no confirmed pull");
            assert_eq!(code_of(&err), Some("FIRST_ADMIN_PLANT_JOIN_REQUIRED"), "{err:?}");

            confirm_empty_plant_for_tests(&db, PLANT).await;
            assert_eq!(
                join_state_of(&db, &worker).await,
                Some(PlantJoinState::CreateFirstAdmin)
            );
            create_first_admin(&db).await.expect("confirmed empty");
        }
    }

    /// A no-sync envelope left from an earlier activation (verified and applied before the current
    /// key entry) never opens the single-PC shortcut: the PC may have been re-activated into a
    /// plant that syncs. A fresh envelope verified after the key entry does.
    #[tokio::test]
    async fn a_no_sync_envelope_older_than_the_activation_counts_as_syncing() {
        let db = licensed_pc(r#"{"sync":false}"#, "active").await;
        assert!(!licence_syncs(&db).await.expect("licence"), "fresh: single-PC");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE entitlement_envelopes SET verified_at = '2020-01-01T00:00:00Z'",
        ))
        .await
        .expect("age the envelope");
        assert!(
            licence_syncs(&db).await.expect("licence"),
            "earlier activation's envelope"
        );
        let worker = worker_in(SyncWorkerState::Blocked, Some("not_licensed"));
        assert_eq!(
            join_state_of(&db, &worker).await,
            Some(PlantJoinState::ConnectionNeeded)
        );
        let err = create_first_admin(&db).await.expect_err("stale envelope");
        assert_eq!(code_of(&err), Some("FIRST_ADMIN_PLANT_JOIN_REQUIRED"), "{err:?}");
        // Not verified at all: syncing too.
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE entitlement_envelopes SET verified_at = NULL",
        ))
        .await
        .expect("unverify");
        assert!(licence_syncs(&db).await.expect("licence"), "unverified envelope");
    }

    /// The same no-sync envelope verified again (the key entered again after a local data reset)
    /// is fresh: its verification time moves forward, so a single-PC plant is not sent to
    /// "connection needed" for ever.
    #[tokio::test]
    async fn the_same_no_sync_envelope_verified_again_is_fresh() {
        let envelope = plant_envelope(r#"{"sync":false}"#, "active");
        let db = licensed_pc_with(envelope.clone()).await;
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE entitlement_envelopes SET verified_at = '2020-01-01T00:00:00Z'",
        ))
        .await
        .expect("age the envelope");
        assert!(licence_syncs(&db).await.expect("licence"), "aged: syncing");
        crate::entitlements::queries::apply_entitlement_envelope(&db, envelope)
            .await
            .expect("same envelope again");
        assert!(!licence_syncs(&db).await.expect("licence"), "verified again: single-PC");
    }

    /// A received account recorded only as a sync conflict (invalid payload or hash: it never
    /// reaches the inbox) still means the server has accounts.
    #[tokio::test]
    async fn an_account_recorded_only_as_a_sync_conflict_ends_the_fact() {
        let db = crate::db::test_db::seeded().await;
        assert!(record_complete_pull(&db, PLANT).await.expect("record"));
        crate::sync::queries::apply_sync_batch(
            &db,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
                server_batch_id: "b-bad".into(),
                checkpoint_token: "ckpt-b-bad".into(),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: vec![SyncInboundItemInput {
                    entity_type: "user_accounts".into(),
                    entity_sync_id: "acct-bad-hash".into(),
                    operation: "upsert".into(),
                    row_version: 1,
                    payload_json: serde_json::json!({ "username": "chef" }).to_string(),
                    payload_hash: Some("0".repeat(64)),
                    ..Default::default()
                }],
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
        let in_inbox = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM sync_inbox WHERE entity_type = 'user_accounts'",
            ))
            .await
            .expect("query")
            .map(|r| r.try_get::<i64>("", "c").expect("count"));
        assert_eq!(in_inbox, Some(0), "an invalid row never reaches the inbox");
        assert!(!server_confirmed_no_accounts(&db, PLANT).await.expect("fact"));
        assert!(!record_complete_pull(&db, PLANT).await.expect("record"));
    }
}
