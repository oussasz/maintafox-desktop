//! VPS contract boundary guards for Phase 4 control-plane integration.

pub mod domain;
pub mod guards;
pub mod mirror;
pub mod object_storage;
pub mod deployment_observability;
pub mod vendor_admin_console;
pub mod customer_entitlement_machine;
pub mod sync_rollout_platform_ops;
pub mod audit_support_hardening;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod mirror_tests;
#[cfg(test)]
mod object_storage_tests;
#[cfg(test)]
mod deployment_observability_tests;
#[cfg(test)]
mod vendor_admin_console_tests;
#[cfg(test)]
mod customer_entitlement_machine_tests;
mod sync_rollout_platform_ops_tests;
mod audit_support_hardening_tests;
