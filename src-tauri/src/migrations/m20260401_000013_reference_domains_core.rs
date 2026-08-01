//! Migration 013 — Reference domain governance layer.
//!
//! Phase 2 - Sub-phase 03 - File 01 - Sprint S1.
//!
//! Creates the PRD 6.13 governed reference data model:
//!   - `reference_domains`: domain catalog with structure type and governance level
//!   - `reference_sets`: versioned set snapshots (draft → validated → published → superseded)
//!   - `reference_values`: coded values scoped to a set version, with optional hierarchy
//!
//! These tables coexist with the earlier `lookup_domains` / `lookup_values` tables
//! (migration 003) which remain the flat, read-optimized consumer path for dropdowns.
//! Sub-phase 03 files 02–04 bridge the governance layer to the consumer layer.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000013_reference_domains_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── reference_domains ─────────────────────────────────────────────────
        // Master catalog of governed reference domains. Each domain has a stable
        // code, a structure type, and a governance level that control what editing
        // workflows are available and how values are consumed by downstream modules.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_domains"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // Stable programmatic code (e.g. "FAILURE_CLASS", "EQUIPMENT_FAMILY").
                    // Uppercase snake or dot-safe token, unique across catalog.
                    .col(
                        ColumnDef::new(Alias::new("code"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    // Human-readable display name.
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    // Structure type governs how values are organized and edited.
                    // Allowed: flat, hierarchical, versioned_code_set, unit_set, external_code_set
                    .col(
                        ColumnDef::new(Alias::new("structure_type"))
                            .text()
                            .not_null(),
                    )
                    // Governance level controls edit constraints and downstream impact.
                    // Allowed: protected_analytical, tenant_managed, system_seeded, erp_synced
                    .col(
                        ColumnDef::new(Alias::new("governance_level"))
                            .text()
                            .not_null(),
                    )
                    // Whether tenant users can add values to this domain.
                    .col(
                        ColumnDef::new(Alias::new("is_extendable"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // Optional JSON with domain-specific validation rules
                    // (e.g. code format regex, required metadata fields).
                    .col(ColumnDef::new(Alias::new("validation_rules_json")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── reference_sets ────────────────────────────────────────────────────
        // A versioned snapshot of values within a domain. Values belong to a set
        // version, not directly to the domain, so historical semantics remain
        // stable after publish. Only one published set per domain at any time.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_sets"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("domain_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("version_no")).integer().not_null())
                    // Lifecycle status: draft | validated | published | superseded
                    .col(ColumnDef::new(Alias::new("status")).text().not_null())
                    // When this set version becomes the effective reference (optional,
                    // allows future-dated activation).
                    .col(ColumnDef::new(Alias::new("effective_from")).text())
                    .col(ColumnDef::new(Alias::new("created_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("published_at")).text())
                    .to_owned(),
            )
            .await?;

        // One version number per domain — prevents duplicate version creation.
        manager
            .create_index(
                Index::create()
                    .name("idx_reference_sets_domain_version")
                    .table(Alias::new("reference_sets"))
                    .col(Alias::new("domain_id"))
                    .col(Alias::new("version_no"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ── reference_values ──────────────────────────────────────────────────
        // Individual coded values within a set version. For hierarchical domains,
        // parent_id points to another value in the same set. Code is unique within
        // a set to prevent duplicate semantics.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_values"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("set_id")).integer().not_null())
                    // Self-referencing FK for hierarchical domains (e.g. failure class → mode).
                    .col(ColumnDef::new(Alias::new("parent_id")).integer())
                    // Stable programmatic code, unique within the set.
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    // Human-readable display label.
                    .col(ColumnDef::new(Alias::new("label")).text().not_null())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("sort_order")).integer())
                    // Optional hex color for badge/status rendering.
                    .col(ColumnDef::new(Alias::new("color_hex")).text())
                    // Optional icon identifier for UI rendering.
                    .col(ColumnDef::new(Alias::new("icon_name")).text())
                    // Semantic classification tag (e.g. "cause", "effect" within failure hierarchy).
                    .col(ColumnDef::new(Alias::new("semantic_tag")).text())
                    // ERP or external system mapping code.
                    .col(ColumnDef::new(Alias::new("external_code")).text())
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // Open JSON extension bag for domain-specific attributes.
                    .col(ColumnDef::new(Alias::new("metadata_json")).text())
                    .to_owned(),
            )
            .await?;

        // Code uniqueness scoped to set — the core semantic invariant.
        manager
            .create_index(
                Index::create()
                    .name("idx_reference_values_set_code")
                    .table(Alias::new("reference_values"))
                    .col(Alias::new("set_id"))
                    .col(Alias::new("code"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Parent lookup index for hierarchy traversal queries.
        manager
            .create_index(
                Index::create()
                    .name("idx_reference_values_parent_id")
                    .table(Alias::new("reference_values"))
                    .col(Alias::new("parent_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("reference_values")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("reference_sets")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("reference_domains")).to_owned())
            .await?;
        Ok(())
    }
}
