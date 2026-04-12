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
pub mod macros;
pub mod model;
pub mod resolver;
pub mod scope_chain;
