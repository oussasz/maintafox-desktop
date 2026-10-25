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
    password::verify_password(password_raw, &hash)
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
pub(crate) async fn record_outcome(
    db: &DatabaseConnection,
    signer: Option<i32>,
    checked: Option<bool>,
    ctx: &SignatureContext<'_>,
) -> AppResult<()> {
    let (Some(signer), Some(ok)) = (signer, checked) else {
        return Ok(());
    };
    if ok {
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
