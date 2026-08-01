//! VPS contract boundary guards for Phase 4 control-plane integration.

pub mod audit_support_hardening;
pub mod customer_entitlement_machine;
pub mod deployment_observability;
pub mod domain;
pub mod guards;
pub mod mirror;
pub mod object_storage;
pub mod sync_rollout_platform_ops;
pub mod vendor_admin_console;

mod audit_support_hardening_tests;
#[cfg(test)]
mod customer_entitlement_machine_tests;
#[cfg(test)]
mod deployment_observability_tests;
#[cfg(test)]
mod mirror_tests;
#[cfg(test)]
mod object_storage_tests;
mod sync_rollout_platform_ops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vendor_admin_console_tests;
