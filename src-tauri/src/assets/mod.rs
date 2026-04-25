//! Asset identity and lifecycle module.
//!
//! This module provides the Rust service layer for PRD §6.3 — the governed
//! equipment and asset registry backbone.
//!
//! The database tables were created in Phase 1 migration 005 (`equipment`,
//! `equipment_classes`, `equipment_hierarchy`, `equipment_meters`,
//! `equipment_lifecycle_events`) and extended by migration 010 with governed
//! identity columns (`maintainable_boundary`, `decommissioned_at`,
//! `asset_external_ids`, effective-dated hierarchy).
//!
//! Sub-module layout:
//!   `identity`  — asset identity CRUD, classification validation, org binding
//!   `hierarchy` — parent-child relations, cycle checks, org-node moves (Sprint S2)
//!   `lifecycle` — append-only lifecycle event history (File 02, Sprint S1)
//!   `meters`    — meter definitions and governed readings (File 02, Sprint S2)
//!   `documents` — governed document link references (File 02, Sprint S3)

pub mod bindings;
pub mod documents;
pub mod governance;
pub mod health;
pub mod hierarchy;
pub mod identity;
pub mod import;
pub mod lifecycle;
pub mod meters;
pub mod photos;
pub mod search;
pub mod taxonomy_reference;

#[cfg(test)]
mod documents_tests;
#[cfg(test)]
mod hierarchy_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod meters_tests;

// Re-export most-used types at module root for clean import in command handlers.
pub use bindings::AssetBindingSummary;
pub use documents::{AssetDocumentLink, UpsertDocumentLinkPayload};
pub use governance::{ConflictCategory, NormalizedImportRow, ValidationMessage, ValidationOutcome};
pub use hierarchy::{AssetHierarchyRow, LinkAssetPayload};
pub use identity::{Asset, CreateAssetPayload, UpdateAssetIdentityPayload};
pub use import::{ApplyPolicy, ApplyResult, ImportBatchSummary, ImportEvent, ImportPreview, ImportPreviewRow};
pub use lifecycle::{AssetLifecycleEvent, RecordLifecycleEventPayload};
pub use meters::{AssetMeter, CreateAssetMeterPayload, MeterReading, RecordMeterReadingPayload};
pub use taxonomy_reference::{EquipmentTaxonomyCatalog, EquipmentTaxonomyOption};
