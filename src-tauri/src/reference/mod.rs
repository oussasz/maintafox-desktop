//! Reference domain governance module.
//!
//! This module implements PRD §6.13 — the Lookup and Reference Data Manager.
//! It is the semantic backbone of Maintafox: the control plane for coded meaning
//! across workflow, analytics, inventory, reliability, and ERP mappings.
//!
//! The governance layer adds versioned set semantics on top of the existing
//! flat lookup consumer path (migration 003, `lookup_domains` / `lookup_values`).
//! Sub-phase 03 files 02–04 bridge the two layers.
//!
//! Sub-module layout:
//!   `governance` — A/B/C category decision engine (single source of truth for permissions)
//!   `audience`   — SYSTEM / COMPANY domain audience (hidden + read-only vs company-configured)
//!   `domains`    — domain catalog CRUD with structure type and governance level validation
//!   `sets`       — set version lifecycle (draft → validated → published → superseded)
//!   `values`     — value tree CRUD with hierarchy cycle detection
//!   `protected`  — protected analytical domain policy checks and usage probes
//!   `validation` — set validation engine with structured diagnostics and persisted reports
//!   `migrations` — merge/migrate operations with migration map traceability
//!   `aliases`    — typed, locale-aware alias CRUD with preferred-alias governance
//!   `imports`    — staged import/export pipeline with row-level diagnostics
//!   `search`     — alias-aware ranked search across canonical fields and aliases
//!   `publish`    — publish readiness engine and impact preview
//!   `urgency_scale` — shared DI / OT urgency scale (`WORK.URGENCY`) and its company attributes
//!   `retirement` — moving links off a merged list and deleting it (data migrations)
//!   `units`      — units of measure `MEASURE.UNITS` and their physical dimensions
//!   `currencies` — ISO 4217 currencies `INVENTORY.CURRENCY` and their operational copy

pub mod aliases;
pub mod audience;
pub mod currencies;
pub mod domains;
pub mod governance;
pub mod impact;
pub mod imports;
pub mod migrations;
pub mod protected;
pub mod publish;
pub mod retirement;
pub mod schedule_patterns;
pub mod search;
pub mod sets;
pub mod system_catalog_integrity;
pub mod units;
pub mod urgency_scale;
pub mod validation;
pub mod values;

#[cfg(test)]
mod aliases_tests;
#[cfg(test)]
mod audience_tests;
#[cfg(test)]
mod domains_tests;
#[cfg(test)]
mod equipment_class_extension_tests;
#[cfg(test)]
mod governance_tests;
#[cfg(test)]
mod impact_tests;
#[cfg(test)]
mod imports_tests;
#[cfg(test)]
mod migrations_tests;
#[cfg(test)]
mod protected_tests;
#[cfg(test)]
mod publish_tests;
#[cfg(test)]
mod schedule_patterns_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod sets_tests;
#[cfg(test)]
mod shared_lists_tests;
#[cfg(test)]
mod system_catalog_integrity_tests;
#[cfg(test)]
mod units_tests;
#[cfg(test)]
mod validation_tests;
#[cfg(test)]
mod values_tests;
