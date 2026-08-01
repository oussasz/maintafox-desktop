//! DI permission domain definitions.
//!
//! Phase 2 - Sub-phase 04 - File 04 - Sprint S1.
//!
//! Provides the canonical list of `di.*` permissions as defined in PRD §6.7.
//! The system seeder can call `di_permission_domain()` to idempotently
//! insert all DI permissions on app startup.

/// Returns the canonical DI permission domain.
///
/// Each tuple: `(name, description, is_dangerous, requires_step_up)`.
///
/// Permissions:
///   - `di.view`       — read-only access to DI list and details
///   - `di.create`     — submit DIs for any asset
///   - `di.create.own` — submit DIs scoped to own entity only
///   - `di.review`     — screen, return, reject DIs
///   - `di.approve`    — approve, defer, reactivate (dangerous, step-up)
///   - `di.convert`    — convert approved DI to work order (dangerous, step-up)
///   - `di.admin`      — override, archive, reopen, manage SLA rules (dangerous)
pub fn di_permission_domain() -> Vec<(&'static str, &'static str, bool, bool)> {
    vec![
        (crate::rbac::permissions::DI_VIEW, "View intervention request list and details", false, false),
        (crate::rbac::permissions::DI_CREATE, "Submit new intervention requests (all assets)", false, false),
        (crate::rbac::permissions::DI_CREATE_OWN, "Submit intervention requests (own entity only)", false, false),
        (crate::rbac::permissions::DI_REVIEW, "Screen, return, and reject intervention requests", false, false),
        (crate::rbac::permissions::DI_APPROVE, "Approve, defer, or reactivate intervention requests", true, true),
        (crate::rbac::permissions::DI_CONVERT, "Convert approved DI to work order", true, true),
        (crate::rbac::permissions::DI_ADMIN, "Override, archive, reopen, manage SLA rules", true, false),
    ]
}
