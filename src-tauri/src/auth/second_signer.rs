//! A second person's password signature, typed on this PC for someone else's action: the FMECA
//! parts override of a close-out (`wo::closeout`) and a planning break-in (`planning::scheduling`).
//!
//! The password counts toward the signer's account lockout as at sign-in (`auth::lockout`):
//! - before the action's transaction, `lockout_state` tells whether the signer is locked out (a
//!   locked-out signer is not checked at all: `SignerPassword::Locked`);
//! - inside it, `verify_user_password` checks the password;
//! - after it (committed or rolled back), `record_outcome` counts the outcome, so a refused action
//!   still counts a wrong password.

use crate::audit;
use crate::auth::password;
use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

/// The signature password of a second signer, as the action can check it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SignerPassword<'a> {
    /// Typed by the signer for this action, on this PC: verified (the outcome is counted toward
    /// the signer's lockout after the action's transaction).
    Given(Option<&'a str>),
    /// Typed for this action, but the signer's account is locked out here (`auth::lockout`, read
    /// before the transaction): not checked, refused with the LOCKED code.
    Locked,
    /// An action received by synchronisation: the password was checked on the PC that acted and
    /// cannot be checked again here (the other rules of the signature still are).
    NotVerifiableHere,
}

/// Whether the signer's account is locked out here. Read before the action's transaction: the
/// lockout check may write (it ends an expired lock).
pub(crate) async fn lockout_state(db: &DatabaseConnection, signer: i32) -> AppResult<bool> {
    match crate::auth::lockout::check_lockout(db, signer).await {
        Ok(()) => Ok(false),
        Err(AppError::AccountLocked { .. }) => Ok(true),
        Err(e) => Err(e),
    }
}

/// The password of an active, not deleted account, checked against its stored hash. An unknown,
/// inactive or password-less account never matches.
pub(crate) async fn verify_user_password(
    db: &impl ConnectionTrait,
    user_id: i64,
    password_raw: &str,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT password_hash
             FROM user_accounts
             WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let hash: Option<String> = row
        .try_get("", "password_hash")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("second signer password_hash decode: {e}")))?;
    let Some(hash) = hash else {
        return Ok(false);
    };
    let ok = password::verify_password(password_raw, &hash)?;
    if ok {
        hold_signer_key(db, user_id, password_raw, &hash).await;
    }
    Ok(ok)
}

/// Passwords of second signers whose key must still be enrolled, by user sync id: taken by
/// [`record_outcome`] once the action committed (enrolment needs the network), wiped otherwise.
fn with_pending_enrolments<R>(f: impl FnOnce(&mut std::collections::HashMap<String, String>) -> R) -> R {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    // A poisoned lock still holds plain data: recover it, so a wipe is never skipped.
    let mut pending = PENDING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut pending)
}

fn take_pending_enrolment(sync_id: &str) -> Option<String> {
    with_pending_enrolments(|pending| pending.remove(sync_id))
}

/// Wipes every second signer password still waiting for an enrolment (session end).
pub(crate) fn release_pending_enrolments() {
    with_pending_enrolments(|pending| {
        for (_, mut password) in pending.drain() {
            crate::sync::signing::wipe_string(&mut password);
        }
    });
}

/// Per-user signing (design v9 §9): the second signer's key, unlocked (or made) with the password
/// they just typed. It co-signs nothing until the action commits and [`record_outcome`] binds it
/// to that action's record (review F2). Never fails the action.
async fn hold_signer_key(db: &impl ConnectionTrait, user_id: i64, password_raw: &str, hash: &str) {
    let sync_id = match crate::sync::identity::optional_entity_sync_id(db, "user_accounts", user_id).await {
        Ok(Some(id)) => id,
        _ => return,
    };
    match crate::auth::signing_keys::unlock_or_create(db, user_id, &sync_id, password_raw, hash).await {
        Ok(key) => {
            crate::auth::signing_keys::hold_cosigner_key(key);
            let previous = with_pending_enrolments(|pending| pending.insert(sync_id, password_raw.to_string()));
            if let Some(mut previous) = previous {
                crate::sync::signing::wipe_string(&mut previous);
            }
        }
        Err(e) => tracing::warn!(error = %e, "second signer: signing key not unlocked"),
    }
}

/// The record a committed action wrote with the second signer named in an author field: their key
/// co-signs only the changes of this record made by the signed-in user.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CosignedRecord {
    pub entity_type: &'static str,
    /// Local row id of the record.
    pub row_id: i64,
}

/// Binds the second signer's held key to `record` and starts its enrolment if needed. Without a
/// record (the action did not commit) or when the record or maker cannot be read, the key and the
/// typed password are dropped instead.
async fn bind_or_release_cosigner(db: &DatabaseConnection, sync_id: &str, record: Option<CosignedRecord>) {
    let password = take_pending_enrolment(sync_id);
    let grant = match record {
        Some(record) => {
            let entity = crate::sync::identity::optional_entity_sync_id(db, record.entity_type, record.row_id).await;
            let maker = crate::sync::capture::current_capture_actor(db).await;
            match (entity, maker) {
                (Ok(Some(entity_sync_id)), Ok(Some(maker_user_sync_id))) => {
                    Some(crate::auth::signing_keys::CosignGrant {
                        entity_type: record.entity_type.to_string(),
                        entity_sync_id,
                        maker_user_sync_id,
                    })
                }
                _ => None,
            }
        }
        None => None,
    };
    let key = grant.and_then(|grant| crate::auth::signing_keys::bind_cosigner_key(sync_id, grant));
    match (key, password) {
        (Some(key), Some(password)) => crate::sync::signing::spawn_enrolment_if_needed(db.clone(), key, password),
        (Some(_), None) => {}
        (None, rest) => {
            crate::auth::signing_keys::release_cosigner_key(sync_id);
            if let Some(mut rest) = rest {
                crate::sync::signing::wipe_string(&mut rest);
            }
        }
    }
}

/// What an outcome is recorded for: who acted, the action (audit `context`) and its subject.
pub(crate) struct SignatureContext<'a> {
    /// The user doing the action (the audit actor of a wrong password).
    pub actor_id: i64,
    /// Short name of the action, written as `context` in the audit detail.
    pub context: &'a str,
    /// Audit summary of a wrong password.
    pub summary: &'a str,
    /// Extra detail fields (merged with `context`), e.g. the subject's id.
    pub detail: serde_json::Value,
}

/// The signer's password outcome, counted toward the signer's account lockout as at sign-in: a
/// wrong one is a failed attempt (`lockout::record_failed_attempt`, same policy), a right one
/// resets the count. Called outside the action's transaction, so a refused action still counts.
/// A wrong password is audited like the other failed password checks (actor: the user acting;
/// target: the signer's account). `checked = None`: not checked (no password, or locked out),
/// nothing recorded.
///
/// `committed`: the record the committed action wrote naming the signer (`None` when the action
/// was refused or rolled back): only then does the signer's key co-sign, and only that record.
pub(crate) async fn record_outcome(
    db: &DatabaseConnection,
    signer: Option<i32>,
    checked: Option<bool>,
    committed: Option<CosignedRecord>,
    ctx: &SignatureContext<'_>,
) -> AppResult<()> {
    let (Some(signer), Some(ok)) = (signer, checked) else {
        return Ok(());
    };
    if ok {
        if let Ok(Some(sync_id)) =
            crate::sync::identity::optional_entity_sync_id(db, "user_accounts", i64::from(signer)).await
        {
            bind_or_release_cosigner(db, &sync_id, committed).await;
        }
        return crate::auth::lockout::clear_failed_attempts(db, signer).await;
    }
    let policy = crate::auth::lockout::LockoutPolicy::load(db).await;
    tracing::warn!(
        signer_id = signer,
        context = ctx.context,
        "second signer: wrong password"
    );
    let signer_ref = signer.to_string();
    let mut detail = serde_json::json!({ "context": ctx.context });
    if let (Some(base), Some(extra)) = (detail.as_object_mut(), ctx.detail.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    audit::emit(
        db,
        audit::AuditEvent {
            event_type: audit::event_type::STEP_UP_FAILURE,
            actor_id: i32::try_from(ctx.actor_id).ok(),
            entity_type: Some("user_account"),
            entity_id: Some(&signer_ref),
            summary: ctx.summary,
            detail_json: Some(detail.to_string()),
            ..Default::default()
        },
    )
    .await;
    crate::auth::lockout::record_failed_attempt(db, signer, &policy).await
}
