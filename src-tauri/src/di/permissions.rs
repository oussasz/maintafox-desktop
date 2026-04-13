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
        ("di.view", "View intervention request list and details", false, false),
        ("di.create", "Submit new intervention requests (all assets)", false, false),
        ("di.create.own", "Submit intervention requests (own entity only)", false, false),
        ("di.review", "Screen, return, and reject intervention requests", false, false),
        ("di.approve", "Approve, defer, or reactivate intervention requests", true, true),
        ("di.convert", "Convert approved DI to work order", true, true),
        ("di.admin", "Override, archive, reopen, manage SLA rules", true, false),
    ]
}
