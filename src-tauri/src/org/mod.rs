//! Organization & Site Operating Model module.
//!
//! This module provides the Rust service layer for PRD §6.2.
//! The database tables (`org_structure_models`, `org_node_types`,
//! `org_type_relationship_rules`, `org_nodes`, `org_node_responsibilities`,
//! `org_entity_bindings`) were created in Phase 1 migration 004.
//!
//! Sub-module layout:
//!   `structure_model`    — lifecycle of the versioned structure schema
//!   `node_types`         — tenant-defined node type vocabulary
//!   `relationship_rules` — allowed parent-child type pairings
//!   `nodes`              — operational node lifecycle and tree integrity
//!   `responsibilities`   — effective-dated ownership bindings
//!   `entity_bindings`    — external system identifier mappings
//!
//! Sub-phase 01 File 01 covers the configuration layer.
//! Sub-phase 01 File 02 covers node management and responsibility bindings.
//! Sub-phase 01 File 04 covers publish validation, node-type remap, and audit.

pub mod audit;
pub mod entity_bindings;
pub mod impact_preview;
pub mod node_types;
pub mod nodes;
pub mod relationship_rules;
pub mod responsibilities;
pub mod structure_model;
pub mod tree_queries;
pub mod validation;

#[cfg(test)]
mod node_types_tests;
#[cfg(test)]
mod nodes_tests;
#[cfg(test)]
mod responsibilities_bindings_tests;
#[cfg(test)]
mod structure_model_tests;
#[cfg(test)]
mod tree_queries_preview_tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod validation_tests;

// Re-export most-used types at module root for clean import in command handlers.
pub use entity_bindings::{OrgEntityBinding, UpsertOrgEntityBindingPayload};
pub use node_types::{CreateNodeTypePayload, OrgNodeType, UpdateNodeTypePayload};
pub use nodes::{
    CreateOrgNodePayload, MoveOrgNodePayload, OrgNode, OrgTreeRow, UpdateOrgNodeMetadataPayload,
};
pub use relationship_rules::{CreateRelationshipRulePayload, OrgRelationshipRule};
pub use responsibilities::{AssignResponsibilityPayload, OrgNodeResponsibility};
pub use structure_model::{CreateStructureModelPayload, OrgStructureModel};
pub use tree_queries::{OrgDesignerNodeRow, OrgDesignerSnapshot};
pub use impact_preview::{OrgImpactPreview, OrgPreviewAction, PreviewOrgChangePayload};
pub use validation::{NodeTypeRemap, OrgPublishValidationResult, OrgValidationIssue};
pub use audit::{OrgAuditEventInput, OrgChangeEvent};
