//! Entitlement envelope persistence and capability enforcement (PRD §10).

pub mod domain;
pub mod queries;
pub mod trusted_keys;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
