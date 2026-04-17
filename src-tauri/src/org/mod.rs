//! Organization & Site Operating Model module.
//!
//! This module provides the Rust service layer for PRD §6.2.
//! The database tables (`org_structure_models`, `org_node_types`,
//! `org_type_relationship_rules`, `org_nodes`, `org_node_responsibilities`,
//! `org_entity_bindings`) were created in Phase 1 migration 004.
//!
//! Sub-module layout:
//!   `structure_model`    — lifecycle of the versioned structure schema
//!   `node_types`         — tenant-defined node type vocabulary (Sprint S2)
//!   `relationship_rules` — allowed parent-child type pairings (Sprint S2)
//!
//! Sub-phase 01 File 01 covers the configuration layer.
//! Sub-phase 01 File 02 covers node management and responsibility bindings.

pub mod node_types;
pub mod relationship_rules;
pub mod structure_model;

#[cfg(test)]
mod node_types_tests;
#[cfg(test)]
mod structure_model_tests;

// Re-export most-used types at module root for clean import in command handlers.
pub use node_types::{CreateNodeTypePayload, OrgNodeType, UpdateNodeTypePayload};
pub use relationship_rules::{CreateRelationshipRulePayload, OrgRelationshipRule};
pub use structure_model::{CreateStructureModelPayload, OrgStructureModel};
