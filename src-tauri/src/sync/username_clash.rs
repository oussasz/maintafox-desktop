//! Usernames created on two PCs (CMMS Advisor decision 2026-10-21, safety net).
//!
//! Before a new PC had to join its plant first (`sync::plant_join`), two PCs of one plant could
//! each create a first administrator, often with the same username. The account received from the
//! other PC then failed the unique username of `user_accounts` and showed as a technical error
//! (`MATERIALIZE_ERROR`) on both PCs. Accounts made while two PCs did not share them yet can still
//! meet (a PC offline for a while, an account created on each at once), so a received account
//! whose username is already used here by another account becomes a `USERNAME_CLASH` conflict that
//! names the username. Nothing is renamed or merged automatically; there is no merge tool. A
//! disabled or deleted account keeps its username (the unique index holds it), so the remedy is a
//! rename: one of the two accounts is renamed (Users) on the PC that created it, then both PCs
//! sync. The renamed version reaches the other PC, passes this check and is applied; the open
//! clash of that account there is closed by it ([`close_outdated_username_clashes`]). On the PC
//! that renamed its own account, the other account received earlier no longer clashes and is taken
//! from its conflict ("take the received version", which goes through this same check).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::materialize::ApplyOutcome;

/// Conflict type of a received account whose username is already used here by another account.
pub const USERNAME_CLASH: &str = "USERNAME_CLASH";

/// After a received version of an account was applied: its earlier `USERNAME_CLASH` conflicts
/// still open are no longer true (it was renamed on its PC), so they are closed and their inbox
/// rows superseded (`code_clash::close_conflicts_superseded_by`). A conflict already dismissed
/// stays as the person left it.
pub(crate) async fn close_outdated_username_clashes(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    applied_row_version: i64,
) -> AppResult<()> {
    if entity_type != "user_accounts" {
        return Ok(());
    }
    crate::sync::code_clash::close_conflicts_superseded_by(
        conn,
        USERNAME_CLASH,
        entity_type,
        entity_sync_id,
        applied_row_version,
        "The account was renamed on its PC; its newer version was applied.",
    )
    .await
}

/// Before a received account is written: `Some(USERNAME_CLASH)` when its username, trimmed and
/// compared without case (as sign-in and account creation compare it), is used here by an account
/// with another sync identity, disabled or deleted ones included (the unique index holds them too).
pub(crate) async fn inbound_username_clash(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<ApplyOutcome>> {
    let Some(username) = payload
        .get("username")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|u| !u.is_empty())
    else {
        return Ok(None);
    };
    let holder = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_sync_id FROM user_accounts
              WHERE LOWER(TRIM(username)) = LOWER(?) AND (entity_sync_id IS NULL OR entity_sync_id <> ?)
              LIMIT 1",
            [username.into(), entity_sync_id.into()],
        ))
        .await?;
    let Some(holder) = holder else {
        return Ok(None);
    };
    let local_id: i64 = holder
        .try_get("", "id")
        .map_err(|e| AppError::SyncError(format!("user_accounts.id: {e}")))?;
    let local_sync_id: Option<String> = holder
        .try_get("", "entity_sync_id")
        .map_err(|e| AppError::SyncError(format!("user_accounts.entity_sync_id: {e}")))?;
    Ok(Some(ApplyOutcome::Conflict {
        code: USERNAME_CLASH.to_string(),
        message: format!(
            "The username '{username}' is already used on this PC by another account ({}): the same username was \
             created on two PCs. Rename one of the two accounts on the PC that created it, then synchronise.",
            local_sync_id.unwrap_or_else(|| format!("local id {local_id}"))
        ),
    }))
}

#[cfg(test)]
mod tests {
    use sea_orm::DatabaseConnection;

    use super::*;
    use crate::sync::domain::{ApplySyncBatchInput, SyncInboundItemInput, SYNC_PROTOCOL_VERSION_V1};
    use crate::sync::test_support::{current_payload, exec, scalar_i64, scalar_opt_text, started_db};

    async fn add_account(db: &DatabaseConnection, sync_id: &str, username: &str) {
        exec(
            db,
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
                 force_password_change, created_at, updated_at)
             VALUES (?, ?, ?, 'local', 1, 1, 0, '2026-10-21T08:00:00Z', '2026-10-21T08:00:00Z')",
            vec![sync_id.into(), username.into(), username.into()],
        )
        .await;
    }

    /// The account `sync_id` as the other PC sends it.
    async fn sent_by(other: &DatabaseConnection, sync_id: &str) -> SyncInboundItemInput {
        let payload = current_payload(other, "user_accounts", sync_id, &serde_json::Map::new())
            .await
            .expect("payload")
            .expect("account on the other PC");
        SyncInboundItemInput {
            entity_type: "user_accounts".into(),
            entity_sync_id: sync_id.into(),
            operation: "upsert".into(),
            row_version: 1,
            payload_json: serde_json::Value::Object(payload).to_string(),
            ..Default::default()
        }
    }

    async fn receive(
        db: &DatabaseConnection,
        items: Vec<SyncInboundItemInput>,
    ) -> crate::sync::materialize::MaterializeInboxResult {
        crate::sync::queries::apply_sync_batch(
            db,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
                server_batch_id: "b1".into(),
                checkpoint_token: "ckpt-b1".into(),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: items,
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
        crate::sync::materialize::materialize_inbox_batch(db, Some(10))
            .await
            .expect("materialize")
    }

    /// The finding of the 0.9.0 test: a first administrator created on each PC with the same
    /// username. The account received is a coded conflict naming the username, never a technical
    /// error; another account from that PC is applied as usual.
    #[tokio::test]
    async fn a_username_created_on_two_pcs_is_a_coded_conflict() {
        let other = started_db().await;
        add_account(&other, "acct-pc-a", " Chef ").await;
        add_account(&other, "acct-pc-a-tech", "technicien").await;
        let db = started_db().await;
        add_account(&db, "acct-pc-b", "chef").await;

        let result = receive(
            &db,
            vec![
                sent_by(&other, "acct-pc-a").await,
                sent_by(&other, "acct-pc-a-tech").await,
            ],
        )
        .await;
        assert!(result.errors.is_empty(), "{result:?}");
        assert_eq!((result.applied, result.conflicts), (1, 1), "{result:?}");
        assert_eq!(
            scalar_opt_text(
                &db,
                "SELECT conflict_type FROM sync_conflicts WHERE entity_sync_id = 'acct-pc-a'",
                vec![]
            )
            .await
            .as_deref(),
            Some(USERNAME_CLASH)
        );
        assert_eq!(
            scalar_opt_text(
                &db,
                "SELECT rejection_code FROM sync_inbox WHERE entity_sync_id = 'acct-pc-a'",
                vec![]
            )
            .await
            .as_deref(),
            Some(USERNAME_CLASH)
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM user_accounts WHERE entity_sync_id IN ('acct-pc-b', 'acct-pc-a-tech')",
                vec![]
            )
            .await,
            2,
            "this PC's account is kept, the other account is applied"
        );
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM sync_conflicts WHERE conflict_type = 'MATERIALIZE_ERROR'",
                vec![]
            )
            .await,
            0
        );
    }

    /// Latest conflict of a received account: (id, row_version, status).
    async fn conflict_of(db: &DatabaseConnection, sync_id: &str) -> (i64, i64, String) {
        let query = |column: &str| {
            format!("SELECT {column} FROM sync_conflicts WHERE entity_sync_id = ? ORDER BY id DESC LIMIT 1")
        };
        (
            scalar_i64(db, &query("id"), vec![sync_id.into()]).await,
            scalar_i64(db, &query("row_version"), vec![sync_id.into()]).await,
            scalar_opt_text(db, &query("status"), vec![sync_id.into()])
                .await
                .expect("status"),
        )
    }

    /// The remedy converges on both PCs. Each PC received the other's "chef" (two clashes). PC A
    /// renames its account: the newer version reaches B, passes the check, is applied and closes
    /// B's open clash. On A, B's "chef" no longer clashes and is taken from its conflict.
    #[tokio::test]
    async fn renaming_one_account_resolves_the_clash_on_both_pcs() {
        let pc_a = started_db().await;
        add_account(&pc_a, "acct-pc-a", "chef").await;
        let pc_b = started_db().await;
        add_account(&pc_b, "acct-pc-b", "chef").await;
        receive(&pc_b, vec![sent_by(&pc_a, "acct-pc-a").await]).await;
        receive(&pc_a, vec![sent_by(&pc_b, "acct-pc-b").await]).await;
        assert_eq!(conflict_of(&pc_b, "acct-pc-a").await.2, "new");
        assert_eq!(conflict_of(&pc_a, "acct-pc-b").await.2, "new");

        // A renames its own account (Users); the newer version reaches B.
        exec(
            &pc_a,
            "UPDATE user_accounts SET username = 'chef-a' WHERE entity_sync_id = 'acct-pc-a'",
            vec![],
        )
        .await;
        let mut renamed = sent_by(&pc_a, "acct-pc-a").await;
        renamed.row_version = 2;
        let result = receive(&pc_b, vec![renamed]).await;
        assert_eq!((result.applied, result.conflicts), (1, 0), "{result:?}");
        assert_eq!(
            scalar_opt_text(
                &pc_b,
                "SELECT username FROM user_accounts WHERE entity_sync_id = 'acct-pc-a'",
                vec![]
            )
            .await
            .as_deref(),
            Some("chef-a")
        );
        assert_eq!(
            conflict_of(&pc_b, "acct-pc-a").await.2,
            "resolved_remote",
            "closed by the newer version"
        );
        assert_eq!(
            scalar_opt_text(
                &pc_b,
                "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = 'acct-pc-a' AND row_version = 1",
                vec![]
            )
            .await
            .as_deref(),
            Some("superseded")
        );

        // On A, B's "chef" is taken from its conflict, through the same check.
        let (id, version, _) = conflict_of(&pc_a, "acct-pc-b").await;
        let admin = scalar_i64(
            &pc_a,
            "SELECT id FROM user_accounts WHERE entity_sync_id = 'acct-pc-a'",
            vec![],
        )
        .await;
        crate::sync::queries::resolve_sync_conflict(
            &pc_a,
            admin,
            crate::sync::domain::ResolveSyncConflictInput {
                conflict_id: id,
                expected_row_version: version,
                action: "accept_remote".into(),
                resolution_note: Some("Account renamed on this PC; the other one is taken".into()),
            },
        )
        .await
        .expect("take the received account");
        assert_eq!(
            scalar_opt_text(
                &pc_a,
                "SELECT username FROM user_accounts WHERE entity_sync_id = 'acct-pc-b'",
                vec![]
            )
            .await
            .as_deref(),
            Some("chef")
        );
    }

    /// Taking the received account while its username is still used here is refused with the
    /// reason, nothing written.
    #[tokio::test]
    async fn taking_a_still_clashing_account_is_refused() {
        let pc_a = started_db().await;
        add_account(&pc_a, "acct-pc-a", "chef").await;
        let pc_b = started_db().await;
        add_account(&pc_b, "acct-pc-b", "chef").await;
        receive(&pc_b, vec![sent_by(&pc_a, "acct-pc-a").await]).await;
        let (id, version, _) = conflict_of(&pc_b, "acct-pc-a").await;
        let admin = scalar_i64(
            &pc_b,
            "SELECT id FROM user_accounts WHERE entity_sync_id = 'acct-pc-b'",
            vec![],
        )
        .await;
        let err = crate::sync::queries::resolve_sync_conflict(
            &pc_b,
            admin,
            crate::sync::domain::ResolveSyncConflictInput {
                conflict_id: id,
                expected_row_version: version,
                action: "accept_remote".into(),
                resolution_note: Some("Trying before any rename was made".into()),
            },
        )
        .await
        .expect_err("still clashing");
        assert!(format!("{err:?}").contains("already used"), "{err:?}");
        assert_eq!(
            scalar_i64(
                &pc_b,
                "SELECT COUNT(*) FROM user_accounts WHERE entity_sync_id = 'acct-pc-a'",
                vec![]
            )
            .await,
            0
        );
    }
}
