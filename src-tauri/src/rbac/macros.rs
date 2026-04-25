//! RBAC macro re-exports — Phase 2 SP06-F01.
//!
//! The permission-checking macros (`require_permission!`, `require_step_up!`,
//! `require_session!`) are defined in [`crate::auth`] and exported at crate
//! root via `#[macro_export]`.  They are available everywhere as
//! `require_permission!(state, &user, "perm", scope)`.
//!
//! This module does **not** redefine those macros.  It exists as the logical
//! home for RBAC macro documentation and any future helper functions that the
//! macros expand to.
//!
//! ## Current macro signatures
//!
//! ```ignore
//! // Authenticate — returns `AuthenticatedUser`
//! let user = require_session!(state);
//!
//! // Authorise — checks permission + step-up
//! require_permission!(state, &user, "di.view", PermissionScope::Global);
//!
//! // Step-up only (no permission check)
//! require_step_up!(state);
//! ```
//!
//! ## Scoped permission check (new resolver API)
//!
//! For callers that need the full `HashSet<String>` or want to check against
//! a scope_reference string directly (without constructing `PermissionScope`),
//! use [`crate::rbac::resolver::effective_permissions`] or
//! [`crate::rbac::resolver::user_has_permission`] directly.
