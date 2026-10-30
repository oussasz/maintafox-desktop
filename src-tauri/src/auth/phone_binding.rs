//! Who may sign in on a phone: only the person it was enrolled for.
//!
//! A phone is enrolled by an administrator for ONE person (`commands::product_license::
//! phone_enrolment`); the binding is stored with its activation. On Android, a password sign-in
//! (and so the PIN unlock of that session) succeeds only for the local account whose sync id is the
//! bound `person_sync_id`; anyone else is refused with `PHONE_ASSIGNED_TO_OTHER_PERSON` and the
//! person's name (the screen says "Ce téléphone est attribué à …"). While the phone is erasing
//! itself (`sync::transport::removal`) nobody signs in.
//!
//! A phone activated with the licence key before enrolment existed is bound to nobody. What happens
//! to it is one policy, [`unbound_phone_policy`]: by default (A) its pending work is uploaded, then
//! it is erased like a removed phone and must be enrolled.
//!
//! The decision ([`phone_sign_in`]) is platform-neutral so it is tested on any host; the gate
//! ([`enforce_phone_sign_in`]) applies it on Android only. Desktops are unchanged.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::commands::product_license::MobileEnrolmentBinding;
use crate::errors::{AppError, AppResult};
use crate::sync::transport::removal::{self, RemovalKind};

/// Someone else than the enrolled person signs in.
pub const PHONE_ASSIGNED_TO_OTHER_PERSON: &str = "PHONE_ASSIGNED_TO_OTHER_PERSON";
/// An activated phone bound to nobody: it uploads its work and resets for an enrolment.
pub const PHONE_NOT_ENROLLED: &str = "PHONE_NOT_ENROLLED";
/// The phone is uploading its work and erasing itself.
pub const PHONE_REMOVAL_IN_PROGRESS: &str = "PHONE_REMOVAL_IN_PROGRESS";
/// The licence server refused the phone without saying it may be erased: locked, data kept.
pub const PHONE_LOCKED: &str = "PHONE_LOCKED";

/// What an activated phone bound to nobody does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnboundPhonePolicy {
    /// (A) Its pending work is uploaded first, then it is erased exactly as a phone removed by its
    /// administrator, and must be enrolled with a QR code.
    UploadThenReset,
}

/// The policy for phones activated with the licence key before enrolment existed (CMMS Advisor
/// decision 2026-10-29: option A; erase only once the server has acknowledged the upload).
pub const fn unbound_phone_policy() -> UnboundPhonePolicy {
    UnboundPhonePolicy::UploadThenReset
}

/// The removal an unbound phone starts under [`unbound_phone_policy`].
pub const fn unbound_phone_removal() -> Option<RemovalKind> {
    match unbound_phone_policy() {
        UnboundPhonePolicy::UploadThenReset => Some(RemovalKind::Unbound),
    }
}

/// The sign-in decision on a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneSignIn {
    Allowed,
    /// The phone is the enrolled person's (`person_label`).
    AssignedToOther {
        person_label: String,
    },
    /// Activated, bound to nobody (see [`unbound_phone_policy`]).
    NotEnrolled,
    RemovalInProgress,
    /// Refused by the server without the erase marker (`removal`, `RevokedUnconfirmed`).
    Locked,
}

/// Whether the account with sync id `user_sync_id` may sign in on a phone. `activated`: the phone
/// holds a valid activation (without one, sign-in is refused further on as on any device);
/// `binding`: its enrolment; `removal`: the removal or lock recorded. An account without a sync id
/// is never the enrolled person.
pub fn phone_sign_in(
    activated: bool,
    binding: Option<&MobileEnrolmentBinding>,
    removal: Option<RemovalKind>,
    user_sync_id: Option<&str>,
) -> PhoneSignIn {
    match removal {
        Some(RemovalKind::RevokedUnconfirmed) => return PhoneSignIn::Locked,
        Some(_) => return PhoneSignIn::RemovalInProgress,
        None => {}
    }
    if !activated {
        return PhoneSignIn::Allowed;
    }
    let Some(binding) = binding else {
        return PhoneSignIn::NotEnrolled;
    };
    match user_sync_id {
        Some(sync_id) if sync_id == binding.person_sync_id => PhoneSignIn::Allowed,
        _ => PhoneSignIn::AssignedToOther {
            person_label: binding.person_label.clone(),
        },
    }
}

async fn user_sync_id(db: &DatabaseConnection, user_id: i32) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| r.try_get::<Option<String>>("", "entity_sync_id"))
        .transpose()?
        .flatten()
        .filter(|s| !s.trim().is_empty()))
}

/// Whether this activated phone is bound to nobody and must be removed under
/// [`unbound_phone_policy`] (the sync worker starts that removal by itself).
pub(crate) async fn unbound_phone_needs_removal(db: &DatabaseConnection) -> AppResult<bool> {
    if unbound_phone_removal().is_none() {
        return Ok(false);
    }
    let activated = crate::commands::product_license::get_activation_claim_tenant_id(db)
        .await?
        .is_some();
    if !activated
        || crate::commands::product_license::mobile_enrolment_binding(db)
            .await?
            .is_some()
    {
        return Ok(false);
    }
    // A record read only through the legacy fallback has lost any binding it held: whether this
    // phone is bound is unknown, so it is never erased for it.
    if crate::commands::product_license::licence_record_is_legacy(db).await? {
        tracing::error!(target: "maintafox::sync", "licence record readable only in its legacy form: no unbound-phone removal");
        return Ok(false);
    }
    Ok(true)
}

/// [`enforce_phone_sign_in`] with the platform given (`is_phone`), for tests on any host.
pub(crate) async fn enforce_phone_sign_in_on(db: &DatabaseConnection, user_id: i32, is_phone: bool) -> AppResult<()> {
    if !is_phone {
        return Ok(());
    }
    let activated = crate::commands::product_license::get_activation_claim_tenant_id(db)
        .await?
        .is_some();
    let binding = crate::commands::product_license::mobile_enrolment_binding(db).await?;
    let pending = removal::pending(db).await?.map(|removal| removal.kind);
    let sync_id = user_sync_id(db, user_id).await?;
    match phone_sign_in(activated, binding.as_ref(), pending, sync_id.as_deref()) {
        PhoneSignIn::Allowed => Ok(()),
        PhoneSignIn::AssignedToOther { person_label } => {
            tracing::warn!(user_id, "login::phone_assigned_to_other_person");
            Err(crate::errors::org_validation_failed(
                PHONE_ASSIGNED_TO_OTHER_PERSON,
                format!("Ce téléphone est attribué à {person_label}."),
                HashMap::from([("person_label".to_string(), person_label)]),
            ))
        }
        PhoneSignIn::NotEnrolled => {
            if let Some(kind) = unbound_phone_removal() {
                // Only when the binding is known to be absent (`unbound_phone_needs_removal`).
                if unbound_phone_needs_removal(db).await? {
                    removal::record(db, kind).await?;
                }
            }
            tracing::warn!(user_id, "login::phone_not_enrolled");
            Err(phone_refusal(
                PHONE_NOT_ENROLLED,
                "Ce téléphone doit être inscrit par un administrateur : son travail en attente est envoyé, \
                 puis il se réinitialise pour scanner le QR code d'inscription.",
            ))
        }
        PhoneSignIn::RemovalInProgress => Err(phone_refusal(
            PHONE_REMOVAL_IN_PROGRESS,
            "Ce téléphone envoie son travail en attente puis s'efface : la connexion est impossible.",
        )),
        PhoneSignIn::Locked => Err(phone_refusal(
            PHONE_LOCKED,
            "Ce téléphone est bloqué : le serveur l'a refusé sans confirmation. Les données sont conservées.              Contactez votre administrateur.",
        )),
    }
}

fn phone_refusal(code: &str, message: &str) -> AppError {
    crate::errors::org_validation_failed(code, message, HashMap::new())
}

/// The phone sign-in rule for the account `user_id` (password checked already): on Android only;
/// a desktop is never refused here.
pub(crate) async fn enforce_phone_sign_in(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    enforce_phone_sign_in_on(db, user_id, cfg!(target_os = "android")).await
}

#[cfg(test)]
#[path = "phone_binding_tests.rs"]
mod tests;
