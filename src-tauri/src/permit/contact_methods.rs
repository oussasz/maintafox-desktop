//! How a worker is reached or told of a forced lock removal: the plant's contact methods
//! (`PERMIT.CONTACT_METHODS`, migration 249).
//!
//! A COMPANY list of Données de référence, the same pattern as the hand-over roles
//! (`permit::handover_roles`): it starts with a few methods flagged "à revoir"
//! ([`STARTER_METHODS`], seeded once by migration 249) that the administrator reviews, renames,
//! deactivates or completes. Each contact attempt of a forced removal, and the method by which a
//! worker without an account was told of it, is an active code of the list (checked by the save;
//! a received row is checked for its shape only, `permit::sync_check`, since the list may reach
//! this PC after the row).

use sea_orm::ConnectionTrait;

use crate::errors::AppResult;
use crate::permit::handover_roles::HandoverRoleOption;
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const DOMAIN: &str = "PERMIT.CONTACT_METHODS";

/// Stable code: a contact method that is not an active value of the list.
pub const CODE_METHOD_NOT_IN_LIST: &str = "PERMIT_CONTACT_METHOD_NOT_IN_LIST";

/// One contact method, as the forms list it (same shape as a hand-over role).
pub type ContactMethodOption = HandoverRoleOption;

/// The starter methods (code, also the French label; English label). Seeded flagged "à revoir".
pub const STARTER_METHODS: &[(&str, &str)] = &[
    ("Téléphone", "Phone"),
    ("SMS", "Text message"),
    ("Radio", "Radio"),
    ("Courriel", "Email"),
    ("En personne", "In person"),
];

/// The list with its published set. Idempotent.
pub async fn ensure_contact_methods(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        DOMAIN,
        "Moyens de contact (retrait forcé de cadenas)",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, DOMAIN).await
}

/// Seeds [`STARTER_METHODS`] (migration 249 only — never the integrity pass, so a method removed
/// later is not brought back), with the rules of the hand-over starters.
pub async fn seed_starter_methods(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_contact_methods(db).await?;
    crate::permit::handover_roles::seed_review_starters(db, DOMAIN, STARTER_METHODS).await
}

/// Active methods of the latest published set, in the list order.
pub async fn list_contact_methods(db: &impl ConnectionTrait) -> AppResult<Vec<ContactMethodOption>> {
    crate::permit::handover_roles::active_options(db, DOMAIN).await
}

/// A contact method: an active value of the list (exact code first, then ignoring case). Returns
/// the list code to store.
pub async fn checked_method(db: &impl ConnectionTrait, field: &str, code: &str) -> AppResult<String> {
    let wanted = code.trim();
    let options = list_contact_methods(db).await?;
    let exact = options.iter().position(|o| o.code == wanted);
    let index = exact.or_else(|| {
        options
            .iter()
            .position(|o| o.code.to_lowercase() == wanted.to_lowercase())
    });
    index.map(|i| options[i].code.clone()).ok_or_else(|| {
        crate::errors::org_validation_failed(
            CODE_METHOD_NOT_IN_LIST,
            format!("{field} '{wanted}' is not an active contact method of Reference data."),
            std::collections::HashMap::from([
                ("field".to_string(), field.to_string()),
                ("code".to_string(), wanted.to_string()),
            ]),
        )
    })
}
