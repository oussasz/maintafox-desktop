//! WO permission domain definitions.
//!
//! Phase 2 - Sub-phase 05 - File 04 - Sprint S1.
//!
//! Provides the canonical list of `ot.*` permissions as defined in PRD §6.7.
//! The system seeder can call `wo_permission_domain()` to idempotently
//! insert all WO permissions on app startup.

/// Returns the canonical WO permission domain.
///
/// Each tuple: `(name, description, is_dangerous, requires_step_up)`.
///
/// Permissions:
///   - `ot.view`    — read-only access to WO list and details
///   - `ot.create`  — create new work orders
///   - `ot.edit`    — edit, plan, assign, and execute work orders
///   - `ot.approve` — approve work orders from draft
///   - `ot.close`   — close technically verified work orders (dangerous, step-up)
///   - `ot.reopen`  — reopen recently closed work orders (dangerous, step-up)
///   - `ot.admin`   — override, archive, manage WO settings (dangerous)
///   - `ot.delete`  — delete draft work orders (dangerous)
pub fn wo_permission_domain() -> Vec<(&'static str, &'static str, bool, bool)> {
    vec![
        (crate::rbac::permissions::OT_VIEW, "View work orders and details", false, false),
        (crate::rbac::permissions::OT_CREATE, "Create new work orders", false, false),
        (crate::rbac::permissions::OT_EDIT, "Edit, plan, assign, and execute work orders", false, false),
        (crate::rbac::permissions::OT_APPROVE, "Approve work orders from draft", false, false),
        (crate::rbac::permissions::OT_CLOSE, "Close technically verified work orders", true, true),
        (crate::rbac::permissions::OT_REOPEN, "Reopen recently closed work orders", true, true),
        (crate::rbac::permissions::OT_ADMIN, "Override, archive, manage WO settings", true, false),
        (crate::rbac::permissions::OT_DELETE, "Delete draft work orders", true, false),
    ]
}
