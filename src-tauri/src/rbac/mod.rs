//! RBAC domain module — Phase 2 SP06-F01.
//!
//! Provides scoped role-based access control types and a live permission resolver.
//!
//! - [`model`]: Domain types mirroring the RBAC database schema.
//! - [`resolver`]: `effective_permissions` / `user_has_permission` — the runtime
//!   permission evaluation engine.
//! - [`macros`]: Re-exports of the `require_permission!` and `require_step_up!`
//!   macros defined in [`crate::auth`].

pub mod cache;
pub mod delegation;
pub mod macros;
pub mod model;
pub mod permissions_generated;
pub mod resolver;
pub mod scope_chain;

/// Canonical permission constants and catalog metadata (generated).
pub mod permissions {
    pub use super::permissions_generated::*;
}

/// Whether a permission appears on the plant's role screens (catalogue, role editor, their
/// dependency lists). Permissions unused in this version, and the legacy alias names kept from
/// older catalogues, stay in the catalogue table and on the roles that hold them; they are only
/// left off these screens.
pub fn shown_in_plant_role_screens(name: &str, category: &str) -> bool {
    // A legacy alias row (e.g. `eq.edit`, renamed to its canonical permission) grants nothing.
    !permissions::ALIAS_MAP.iter().any(|(alias, _)| *alias == name)
        && !permissions::HIDDEN_FROM_PLANT_SCREENS.contains(&name)
        && !permissions::HIDDEN_CATEGORIES_FROM_PLANT_SCREENS.contains(&category)
}

/// [`shown_in_plant_role_screens`] for a bare name (a dependency row): the category comes from
/// the canonical catalogue; a custom permission is always shown.
pub fn name_shown_in_plant_role_screens(name: &str) -> bool {
    let category = permissions::CATALOG
        .iter()
        .find(|m| m.name == name)
        .map_or("", |m| m.category);
    shown_in_plant_role_screens(name, category)
}

#[cfg(test)]
mod tests;
