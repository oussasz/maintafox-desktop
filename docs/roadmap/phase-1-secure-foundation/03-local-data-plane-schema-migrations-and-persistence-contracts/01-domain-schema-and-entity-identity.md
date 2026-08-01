# Phase 1 · Sub-phase 03 · File 01
# Domain Schema and Entity Identity

## Context and Purpose

Sub-phase 02 delivered the complete application shell. Before Phase 2 modules can build
any real UI or data workflows, the database must carry the structural tables that every
module shares. This file creates the four domain-area migrations that form the Phase 2
data foundation:

- **Migration 003** — Governed reference domain tables (§6.13): the lookup and reference
  value system that all other modules consume for classifications, statuses, families, and
  controlled vocabulary. These tables must exist before foreign-key-bearing domain tables.
- **Migration 004** — Organization structure tables (§6.2): tenant-defined node hierarchy,
  node types, relationship rules, responsibilities, and structural versioning.
- **Migration 005** — Equipment registry foundation (§6.3): equipment classes, families,
  status/criticality domains, the equipment entity itself, hierarchy, meters, and lifecycle
  event log.
- **Migration 006** — Teams, skills, and workforce structure (§6.6 precursor): teams,
  team member assignments, skill definitions, and the skill categories needed by personnel
  readiness and work-order assignment.

## Architecture Rules Applied

From PRD §7.1:

1. Every synchronized business table has an `INTEGER PRIMARY KEY` for local joins plus a
   `sync_id TEXT NOT NULL UNIQUE` (UUID v4) for cross-machine identity.
2. Mutable records carry `created_at`, `updated_at`, `deleted_at` (soft-delete), and
   `row_version INTEGER NOT NULL DEFAULT 1` for optimistic concurrency.
3. Additional provenance fields on cross-machine business records: `origin_machine_id TEXT`
   and `last_synced_checkpoint TEXT`.
4. Reference/lookup tables that govern enum-like values are in `lookup_domains` /
   `lookup_values` — not hardcoded enums — so the tenant can extend them.
5. All timestamps are ISO-8601 strings in SQLite because SQLite has no native DATETIME
   type; application layer provides the parsing discipline.

## Prerequisites

- SP01-F03: `db/mod.rs` with `init_db`, sea-orm Migrator, migrations 001 and 002 complete
- SP02-F02: AppState with DbPool injected into IPC commands
- SP02-F03: Router with all placeholder pages registered

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Reference Domain Tables (Migration 003) | `lookup_domains`, `lookup_values`, `lookup_value_aliases`, Migrator update |
| S2 | Organization Structure Tables (Migration 004) | Six org tables, Migrator update, entity identity docs |
| S3 | Equipment and Workforce Foundation Tables (Migrations 005–006) | Equipment registry, lifecycle events, teams, skills |

---

## Sprint S1 — Reference Domain Tables (Migration 003)

### AI Agent Prompt

```
You are a senior Rust engineer working on Maintafox Desktop (Tauri 2.x, sea-orm 1.x).
Sub-phases 01 and 02 are complete. Migrations 001 (system_config, trusted_devices,
audit_events, app_sessions) and 002 (user_accounts, roles, permissions, role_permissions,
user_scope_assignments) are already written.

YOUR TASK: Create migration 003, which defines the governed reference domain tables
described in PRD §6.13. These tables are the foundation for all controlled vocabulary —
equipment classes, work order types, urgency levels, failure codes, unit of measure, and
any other governed list that a module or the tenant controls.

Architecture rule: reference domain tables must be created BEFORE any table that
foreign-keys into them. This migration must contain no forward references.

────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/migrations/m20260331_000003_reference_domains.rs
────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260331_000003_reference_domains.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000003_reference_domains"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── lookup_domains ────────────────────────────────────────────────────
        // Each domain is a named, versioned, governed set of values.
        // domain_key is the stable programmatic identifier (e.g. "equipment.class").
        // domain_type distinguishes operational list (user can extend) from
        // system-protected list (only admin can alter).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_domains"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("domain_key")).text().not_null().unique_key())
                    // human name shown in the UI
                    .col(ColumnDef::new(Alias::new("display_name")).text().not_null())
                    // "system" | "tenant" | "module"
                    .col(ColumnDef::new(Alias::new("domain_type")).text().not_null().default("tenant"))
                    // which modules use this domain — informational, JSON array of module keys
                    .col(ColumnDef::new(Alias::new("consumer_modules_json")).text())
                    // whether values in this domain are orderable (have a sort_order)
                    .col(ColumnDef::new(Alias::new("is_ordered")).integer().not_null().default(0))
                    // whether tenant can add values (0 = strictly system-managed)
                    .col(ColumnDef::new(Alias::new("is_extensible")).integer().not_null().default(1))
                    // locked domains cannot be published after first install
                    .col(ColumnDef::new(Alias::new("is_locked")).integer().not_null().default(0))
                    // minor version incremented on each publish cycle
                    .col(ColumnDef::new(Alias::new("schema_version")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("published_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("published_at")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .to_owned(),
            )
            .await?;

        // ── lookup_values ─────────────────────────────────────────────────────
        // Individual governed values within a domain.
        // code is the stable programmatic key (e.g. "CORRECTIVE") — never changes.
        // label is the displayable name; fr_label and en_label allow i18n coupling.
        // sort_order controls display ordering for ordered domains.
        // is_system marks values that ship with the product and cannot be deleted.
        // color is an optional hex for status/badge color rendering.
        // metadata_json is an open extension bag for domain-specific attributes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_values"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("domain_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("label")).text().not_null())
                    .col(ColumnDef::new(Alias::new("fr_label")).text())
                    .col(ColumnDef::new(Alias::new("en_label")).text())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("sort_order")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("is_system")).integer().not_null().default(0))
                    // optional hex color for badge/status rendering
                    .col(ColumnDef::new(Alias::new("color")).text())
                    // optional parent value id — for hierarchical domains (e.g. failure categories)
                    .col(ColumnDef::new(Alias::new("parent_value_id")).integer())
                    // free JSON extension bag for domain-specific extra attributes
                    .col(ColumnDef::new(Alias::new("metadata_json")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .to_owned(),
            )
            .await?;

        // Compound unique: one code per domain (active values only enforced at app layer)
        manager
            .create_index(
                Index::create()
                    .name("idx_lookup_values_domain_code")
                    .table(Alias::new("lookup_values"))
                    .col(Alias::new("domain_id"))
                    .col(Alias::new("code"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ── lookup_value_aliases ───────────────────────────────────────────────
        // Aliases allow legacy codes, import abbreviations, and ERP synonyms to
        // resolve to the canonical governed value without polluting the master list.
        // Used during import mapping and ERP connector reconciliation (§6.13, §6.22).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_value_aliases"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("value_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("alias_code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("alias_type")).text().not_null().default("import"))
                    // "import" | "erp" | "legacy" | "translation"
                    .col(ColumnDef::new(Alias::new("source_system")).text())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("lookup_value_aliases")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("lookup_values")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("lookup_domains")).to_owned())
            .await?;
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — Register migration 003 in src-tauri/migrations/mod.rs (the Migrator)
────────────────────────────────────────────────────────────────────
Open src-tauri/migrations/mod.rs and add after migration 002:

```rust
mod m20260331_000003_reference_domains;

// In the migrations() vec:
Box::new(m20260331_000003_reference_domains::Migration),
```

Ensure migrations are listed in strict ascending order. The Migrator must never reorder
existing migrations.

────────────────────────────────────────────────────────────────────
STEP 3 — Add src-tauri/src/db/reference_domains.rs (raw SQL verification queries)
────────────────────────────────────────────────────────────────────
Add a small verification module that can confirm the reference domain tables are
accessible:

```rust
// src-tauri/src/db/reference_domains.rs
use sea_orm::DatabaseConnection;
use crate::errors::AppResult;

/// Confirms the reference domain tables exist by running a trivial COUNT query.
/// Called during startup validation (Sprint S3 of this sub-phase).
pub async fn verify_reference_domain_tables(db: &DatabaseConnection) -> AppResult<()> {
    use sea_orm::Statement;
    use sea_orm::ConnectionTrait;
    use sea_orm::DbBackend;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COUNT(*) FROM lookup_domains;".to_string(),
    ))
    .await
    .map(|_| ())
    .map_err(|e| crate::errors::AppError::Database(e.to_string()))
}
```

Register this module in `src-tauri/src/db/mod.rs`:
```rust
pub mod reference_domains;
```

────────────────────────────────────────────────────────────────────
STEP 4 — Add entity identity documentation to docs/DB_SCHEMA_CONVENTIONS.md
────────────────────────────────────────────────────────────────────
Create a new file:

```markdown
# DB Schema Conventions

Source: PRD §7.1 Local SQLite Schema Principles

## Entity Identity

Every synchronized business table MUST follow this identity pattern:

```sql
id       INTEGER PRIMARY KEY AUTOINCREMENT,  -- local fast-join key
sync_id  TEXT NOT NULL UNIQUE,               -- UUID v4, cross-machine identity
```

`id` is used for local SQLite foreign keys. `sync_id` is used for sync outbox,
conflict resolution, and cross-machine references.

## Timestamp Convention

All timestamps are stored as TEXT in ISO 8601 format: `2026-03-31T14:23:00Z`.
SQLite has no native DATETIME type. The application layer enforces parsing and
sorting discipline.

| Column | Presence | Purpose |
|--------|----------|---------|
| `created_at` | Mandatory on all mutable tables | Record creation timestamp |
| `updated_at` | Mandatory on all mutable tables | Last modification timestamp |
| `deleted_at` | On soft-deletable tables | NULL means active; set = soft-deleted |
| `row_version` | On sync-eligible tables | Incremented on every write for optimistic concurrency |
| `origin_machine_id` | On sync-eligible business tables | Identifies the machine that created the record |
| `last_synced_checkpoint` | On sync-eligible business tables | Checkpoint token of last successful sync |

## Soft Deletes

Records referenced by historical work, cost, reliability, or audit data are soft-deleted
via `deleted_at`. Hard delete is only permitted for draft records that have never been
referenced. The application layer filters `WHERE deleted_at IS NULL` for live queries.

## Reference Domain FK Convention

When a table references a governed lookup value, it stores the `lookup_values.id`
(integer FK), NOT the code string. The code is looked up at render time. This allows
label renames without data migration.

## Optimistic Concurrency

Before writing an update, the application must confirm:
```sql
WHERE id = :id AND row_version = :expected_version
```
If 0 rows are affected, the write was rejected by a concurrent modification.
The row_version is incremented in the same UPDATE statement.
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- pnpm run dev opens without errors
- Running `pnpm run dev` and checking the startup logs shows migration 003 applied
  (look for sea-orm migration runner output in the Tauri console)
- The three tables (lookup_domains, lookup_values, lookup_value_aliases) exist in the
  SQLite database file (use DBeaver or sqlitebrowser to verify)
- docs/DB_SCHEMA_CONVENTIONS.md is created
```

---

### Supervisor Verification — Sprint S1

**V1 — Migration applies without errors.**
Run `pnpm run dev`. In the terminal output, look for lines from the Rust launcher
mentioning "migration" or "applying" or similar markers. The application must open
without a crash or startup error screen. If an error screen appears with text like
"Database error" or "Migration failed", copy the error message and flag it.

**V2 — New tables are present in the database.**
Open the database file with DBeaver or DB Browser for SQLite. The file is located at
(on Windows): `%APPDATA%\maintafox\maintafox.db` (or the path shown in the .env file).
In the left panel you should see three new tables: `lookup_domains`, `lookup_values`,
`lookup_value_aliases`. If any are absent, flag which one.

**V3 — Schema conventions document is present.**
Open `docs/DB_SCHEMA_CONVENTIONS.md`. It should describe the `id` + `sync_id` pattern
and timestamp conventions. If the file is empty or absent, flag it.

---

## Sprint S2 — Organization Structure Tables (Migration 004)

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop. Migration 003
(reference domain tables) is complete and applied. Your task is to create migration 004:
the Organization Structure tables described in PRD §6.2.

The org model is tenant-defined: the administrator configures the node types, allowed
parent-child relationships, and structural versions. There is no hardcoded hierarchy.
This migration creates the structural scaffolding; the tenant populates it via the
admin UI in Phase 2 § Sub-phase 03.

────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/migrations/m20260331_000004_org_schema.rs
────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260331_000004_org_schema.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000004_org_schema"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── org_structure_models ──────────────────────────────────────────────
        // A versioned model of the tenant's organizational structure schema.
        // When the tenant publishes a structural change, a new version is created.
        // This allows historical records to reference the model version active
        // at the time the work was performed.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_structure_models"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("version_number")).integer().not_null().default(1))
                    // "draft" | "active" | "superseded" | "archived"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("draft"))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("activated_at")).text())
                    .col(ColumnDef::new(Alias::new("activated_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("superseded_at")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_node_types ────────────────────────────────────────────────────
        // Tenant-defined node type vocabulary (e.g. "Site", "Plant", "Zone",
        // "Building", "Process Unit", "Functional Position").
        // Capability flags define what governance rules apply to this node type.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_node_types"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("structure_model_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("label")).text().not_null())
                    .col(ColumnDef::new(Alias::new("icon_key")).text())
                    .col(ColumnDef::new(Alias::new("depth_hint")).integer())
                    // Capability flags (0/1 booleans as INTEGER):
                    // can the node host equipment assets?
                    .col(ColumnDef::new(Alias::new("can_host_assets")).integer().not_null().default(0))
                    // can work orders and DIs be scoped to this node?
                    .col(ColumnDef::new(Alias::new("can_own_work")).integer().not_null().default(0))
                    // can this node be a cost center?
                    .col(ColumnDef::new(Alias::new("can_carry_cost_center")).integer().not_null().default(0))
                    // can this node aggregate KPIs upward?
                    .col(ColumnDef::new(Alias::new("can_aggregate_kpis")).integer().not_null().default(0))
                    // can work permits be issued at this node?
                    .col(ColumnDef::new(Alias::new("can_receive_permits")).integer().not_null().default(0))
                    // is this node the root (only one root allowed per model)?
                    .col(ColumnDef::new(Alias::new("is_root_type")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_type_relationship_rules ───────────────────────────────────────
        // Defines which parent node types may contain which child node types.
        // This prevents invalid structural configurations at publish time.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_type_relationship_rules"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("structure_model_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("parent_type_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("child_type_id")).integer().not_null())
                    // min/max child count (NULL = unrestricted)
                    .col(ColumnDef::new(Alias::new("min_children")).integer())
                    .col(ColumnDef::new(Alias::new("max_children")).integer())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_nodes ─────────────────────────────────────────────────────────
        // The actual tenant-defined organizational nodes (e.g. "Usine Sud",
        // "Zone de production A", "Atelier mécanique").
        // parent_id is self-referencing. NULL parent_id = root node.
        // effective_from / effective_to support versioned structural changes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_nodes"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("node_type_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("parent_id")).integer())
                    // hierarchy path string for fast descendant queries (closure table alternative)
                    // format: "/1/4/17/" — represents the id path from root to this node
                    .col(ColumnDef::new(Alias::new("ancestor_path")).text().not_null().default("/"))
                    // depth in the hierarchy tree (0 = root)
                    .col(ColumnDef::new(Alias::new("depth")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("cost_center_code")).text())
                    .col(ColumnDef::new(Alias::new("external_reference")).text())
                    // "active" | "inactive" | "decommissioned" | "under_construction"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("active"))
                    .col(ColumnDef::new(Alias::new("effective_from")).text())
                    .col(ColumnDef::new(Alias::new("effective_to")).text())
                    .col(ColumnDef::new(Alias::new("erp_reference")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .col(ColumnDef::new(Alias::new("last_synced_checkpoint")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_org_nodes_parent_id")
                    .table(Alias::new("org_nodes"))
                    .col(Alias::new("parent_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_org_nodes_ancestor_path")
                    .table(Alias::new("org_nodes"))
                    .col(Alias::new("ancestor_path"))
                    .to_owned(),
            )
            .await?;

        // ── org_node_responsibilities ──────────────────────────────────────────
        // Named responsibility bindings on an org node.
        // responsibility_type is a governed value from lookup_domain "org.responsibility.type"
        // (e.g. maintenance_owner, production_owner, hse_owner, planner, approver).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_node_responsibilities"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("node_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("responsibility_type")).text().not_null())
                    // references personnel.id (table created in migration 006+)
                    .col(ColumnDef::new(Alias::new("person_id")).integer())
                    // references teams.id (table created in migration 006)
                    .col(ColumnDef::new(Alias::new("team_id")).integer())
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_entity_bindings ────────────────────────────────────────────────
        // Associates external identifiers (ERP plant codes, SAP functional locations,
        // legacy system codes) to org nodes for import and sync mapping.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_entity_bindings"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("node_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("binding_type")).text().not_null())
                    // "erp_plant" | "erp_cost_center" | "sap_fl" | "legacy_code" | "external_api"
                    .col(ColumnDef::new(Alias::new("external_system")).text().not_null())
                    .col(ColumnDef::new(Alias::new("external_id")).text().not_null())
                    .col(ColumnDef::new(Alias::new("is_primary")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in &[
            "org_entity_bindings",
            "org_node_responsibilities",
            "org_nodes",
            "org_type_relationship_rules",
            "org_node_types",
            "org_structure_models",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — Register migration 004 in migrations/mod.rs
────────────────────────────────────────────────────────────────────
Add after migration 003:
```rust
mod m20260331_000004_org_schema;
// In vec: Box::new(m20260331_000004_org_schema::Migration),
```

────────────────────────────────────────────────────────────────────
STEP 3 — Add a verification query for org tables to db/mod.rs
────────────────────────────────────────────────────────────────────
In `src-tauri/src/db/mod.rs`, alongside the existing `init_db` function, add:

```rust
/// Verifies that org schema tables exist (called during startup integrity check).
pub async fn verify_org_tables(db: &sea_orm::DatabaseConnection) -> crate::errors::AppResult<()> {
    use sea_orm::{ConnectionTrait, Statement, DbBackend};
    for tbl in &["org_structure_models", "org_node_types", "org_nodes"] {
        let sql = format!("SELECT COUNT(*) FROM {};", tbl);
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .map_err(|e| crate::errors::AppError::Database(e.to_string()))?;
    }
    Ok(())
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- pnpm run dev applies migration 004 without errors
- Six org tables are present in the database: org_structure_models, org_node_types,
  org_type_relationship_rules, org_nodes, org_node_responsibilities, org_entity_bindings
- Index idx_org_nodes_parent_id and idx_org_nodes_ancestor_path are present
```

---

### Supervisor Verification — Sprint S2

**V1 — Migration 004 applied cleanly.**
Restart the app with `pnpm run dev`. No error screen should appear. If the startup screen
shows an error, copy the message and flag it.

**V2 — Org tables are present in the database.**
Open the SQLite database in DBeaver. Look for these table names: `org_structure_models`,
`org_node_types`, `org_type_relationship_rules`, `org_nodes`, `org_node_responsibilities`,
`org_entity_bindings`. All six must be present. If any are missing, flag them by name.

**V3 — Entity identity columns are present.**
Click on the `org_nodes` table in DBeaver and look at the Columns tab. You should see
column names including `id`, `sync_id`, `created_at`, `updated_at`, `deleted_at`,
`row_version`, `origin_machine_id`. If any of these are absent, flag them.

---

## Sprint S3 — Equipment and Workforce Foundation Tables (Migrations 005–006)

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop. Migrations 001–004
are complete. Your task is to create two more migrations:

- Migration 005: Equipment Asset Registry foundation tables (PRD §6.3)
- Migration 006: Teams and Skills tables (PRD §6.6 precursor — personnel table itself
  comes in Sub-phase 04, but teams and skills are needed by org, work orders, and the
  RBAC system before full personnel is built)

────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/migrations/m20260331_000005_equipment_schema.rs
────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260331_000005_equipment_schema.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000005_equipment_schema"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── equipment_classes ─────────────────────────────────────────────────
        // Governed classification hierarchy: class > family > subfamily.
        // This is a two-level self-referencing table (parent_id NULL = top-level class).
        // equipment_class_domain_id references the lookup_domains.id for the
        // "equipment.class" domain which validates canonical class codes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_classes"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("parent_id")).integer())
                    // "class" | "family" | "subfamily"
                    .col(ColumnDef::new(Alias::new("level")).text().not_null().default("class"))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("erp_reference")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .to_owned(),
            )
            .await?;

        // ── equipment ─────────────────────────────────────────────────────────
        // Core equipment identity record. Every field that might be needed by
        // work orders, planning, PM cycles, reliability, ERP, or IoT is present.
        // The asset_id_code is the internal human tag (e.g. "POMPE-001").
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("asset_id_code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("class_id")).integer())
                    // "active_in_service" | "in_stock" | "under_maintenance" | "decommissioned" |
                    // "scrapped" | "transferred" | "spare"
                    .col(ColumnDef::new(Alias::new("lifecycle_status")).text().not_null().default("active_in_service"))
                    // criticality references lookup_values.id from domain "equipment.criticality"
                    .col(ColumnDef::new(Alias::new("criticality_value_id")).integer())
                    // primary org node this asset is installed at
                    .col(ColumnDef::new(Alias::new("installed_at_node_id")).integer())
                    // the functional position this asset occupies (nullable — for installed components)
                    .col(ColumnDef::new(Alias::new("functional_position_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("manufacturer")).text())
                    .col(ColumnDef::new(Alias::new("model")).text())
                    .col(ColumnDef::new(Alias::new("serial_number")).text())
                    .col(ColumnDef::new(Alias::new("purchase_date")).text())
                    .col(ColumnDef::new(Alias::new("commissioning_date")).text())
                    .col(ColumnDef::new(Alias::new("warranty_expiry_date")).text())
                    .col(ColumnDef::new(Alias::new("replacement_value")).real())
                    .col(ColumnDef::new(Alias::new("cost_center_code")).text())
                    // ERP asset number for reconciliation
                    .col(ColumnDef::new(Alias::new("erp_asset_id")).text())
                    // SAP functional location reference
                    .col(ColumnDef::new(Alias::new("erp_functional_location")).text())
                    // IoT asset identifier for signal binding
                    .col(ColumnDef::new(Alias::new("iot_asset_id")).text())
                    .col(ColumnDef::new(Alias::new("qr_code")).text())
                    .col(ColumnDef::new(Alias::new("barcode")).text())
                    .col(ColumnDef::new(Alias::new("photo_path")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("technical_specs_json")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .col(ColumnDef::new(Alias::new("last_synced_checkpoint")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_class_id")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("class_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_node_id")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("installed_at_node_id"))
                    .to_owned(),
            )
            .await?;

        // ── equipment_hierarchy ────────────────────────────────────────────────
        // Parent-child relationships between equipment items.
        // Models assemblies, sub-components, and installed parts.
        // relationship_type: "parent_child" | "installed_in" | "drives" | "feeds"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_hierarchy"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("parent_equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("child_equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("relationship_type")).text().not_null().default("parent_child"))
                    .col(ColumnDef::new(Alias::new("position_label")).text())
                    .col(ColumnDef::new(Alias::new("installed_at")).text())
                    .col(ColumnDef::new(Alias::new("removed_at")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── equipment_meters ──────────────────────────────────────────────────
        // Meters on equipment used by PM cycle triggers and reliability analytics.
        // meter_type: "hours" | "cycles" | "distance" | "volume" | "custom"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_meters"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("meter_type")).text().not_null().default("hours"))
                    .col(ColumnDef::new(Alias::new("unit")).text().not_null())
                    .col(ColumnDef::new(Alias::new("current_reading")).real().not_null().default(0.0))
                    .col(ColumnDef::new(Alias::new("last_read_at")).text())
                    .col(ColumnDef::new(Alias::new("expected_rate_per_day")).real())
                    .col(ColumnDef::new(Alias::new("is_primary")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── equipment_lifecycle_events ─────────────────────────────────────────
        // Append-only log of significant lifecycle events on the asset.
        // event_type: "moved" | "installed" | "replaced" | "reclassified" |
        //             "decommissioned" | "reactivated" | "warranty_update"
        // This table is the provenance record for PRD §6.3 governance rules.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_lifecycle_events"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("event_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("occurred_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("performed_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("from_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("to_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("from_status")).text())
                    .col(ColumnDef::new(Alias::new("to_status")).text())
                    .col(ColumnDef::new(Alias::new("related_work_order_id")).integer())
                    .col(ColumnDef::new(Alias::new("details_json")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    // lifecycle events are append-only: no updated_at, no deleted_at
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_eq_lifecycle_equipment_id")
                    .table(Alias::new("equipment_lifecycle_events"))
                    .col(Alias::new("equipment_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in &[
            "equipment_lifecycle_events",
            "equipment_meters",
            "equipment_hierarchy",
            "equipment",
            "equipment_classes",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/migrations/m20260331_000006_teams_and_skills.rs
────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260331_000006_teams_and_skills.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000006_teams_and_skills"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── skill_categories ──────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("skill_categories"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── skill_definitions ──────────────────────────────────────────────────
        // Governed skill vocabulary: programming, electrical, hydraulic, welding, etc.
        // is_authorization_required: true means having this skill requires a formal
        // qualification record in the training module (§6.20).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("skill_definitions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("category_id")).integer())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // does possessing this skill require a training qualification record?
                    .col(ColumnDef::new(Alias::new("is_authorization_required")).integer().not_null().default(0))
                    // typical revalidation period in months (0 = no expiry)
                    .col(ColumnDef::new(Alias::new("revalidation_months")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .to_owned(),
            )
            .await?;

        // ── teams ─────────────────────────────────────────────────────────────
        // Maintenance teams scoped to org nodes. Used by work-order assignment,
        // planning, and workforce capacity views.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("teams"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    // "maintenance" | "inspection" | "planning" | "contractor" | "hse"
                    .col(ColumnDef::new(Alias::new("team_type")).text().not_null().default("maintenance"))
                    .col(ColumnDef::new(Alias::new("primary_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // "active" | "inactive" | "disbanded"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("active"))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .to_owned(),
            )
            .await?;

        // ── team_skill_requirements ───────────────────────────────────────────
        // Defines which skills a team expects to have coverage of.
        // Used by the workforce readiness dashboard.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("team_skill_requirements"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("team_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("skill_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("min_headcount")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("required_proficiency")).integer().not_null().default(3))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in &[
            "team_skill_requirements",
            "teams",
            "skill_definitions",
            "skill_categories",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — Register migrations 005 and 006 in migrations/mod.rs
────────────────────────────────────────────────────────────────────
Final migrations list in order:
1. m20260331_000001_system_tables
2. m20260331_000002_user_tables
3. m20260331_000003_reference_domains
4. m20260331_000004_org_schema
5. m20260331_000005_equipment_schema
6. m20260331_000006_teams_and_skills

────────────────────────────────────────────────────────────────────
STEP 4 — Update docs/DB_SCHEMA_CONVENTIONS.md: add migration count baseline
────────────────────────────────────────────────────────────────────
Append the following section to docs/DB_SCHEMA_CONVENTIONS.md:

```markdown
## Migration Baseline

As of Sub-phase 03, 6 migrations define the Phase 1 schema:

| Migration | Tables Created |
|-----------|---------------|
| 001_system_tables | system_config, trusted_devices, audit_events, app_sessions |
| 002_user_tables | user_accounts, roles, permissions, role_permissions, user_scope_assignments |
| 003_reference_domains | lookup_domains, lookup_values, lookup_value_aliases |
| 004_org_schema | org_structure_models, org_node_types, org_type_relationship_rules, org_nodes, org_node_responsibilities, org_entity_bindings |
| 005_equipment_schema | equipment_classes, equipment, equipment_hierarchy, equipment_meters, equipment_lifecycle_events |
| 006_teams_and_skills | skill_categories, skill_definitions, teams, team_skill_requirements |

Phase 2+ migrations add module tables as each sprint builds functional pages.
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- pnpm run dev startup log shows all 6 migrations applied (or already applied)
- DBeaver shows all tables from migrations 001–006 present in the database
- equipment table has sync_id, row_version, origin_machine_id, last_synced_checkpoint
- equipment_lifecycle_events has no deleted_at column (append-only: this is intentional)
- docs/DB_SCHEMA_CONVENTIONS.md migration baseline table is present
```

---

### Supervisor Verification — Sprint S3

**V1 — All 6 migrations applied.**
Close the app, delete the database file (only do this once — it will be rebuilt by the
migrations), run `pnpm run dev`. Watch the terminal. You should see output indicating
migration 001 through 006 each applied. The app should open normally after. If any
migration shows an error, flag it with the migration number.

**V2 — Equipment tables are present.**
Open the SQLite database in DBeaver. In the table list, find and verify: `equipment_classes`,
`equipment`, `equipment_hierarchy`, `equipment_meters`, `equipment_lifecycle_events`.
All five must be present. Count the columns in the `equipment` table — there should be
at least 25 columns (including sync fields). If the count is below 20, flag it.

**V3 — equipment_lifecycle_events has no deleted_at.**
Click on `equipment_lifecycle_events` in DBeaver and look at Columns. There should be
NO column named `deleted_at`. This is intentional — lifecycle events are append-only
records. If you see a `deleted_at` column on this table, flag it because the schema
violates the append-only rule for this table.

**V4 — Teams and skills tables are present.**
Find: `skill_categories`, `skill_definitions`, `teams`, `team_skill_requirements`.
All four should be in the table list. If any are missing, flag them.

---

*End of Phase 1 · Sub-phase 03 · File 01*
*Next: File 02 — Migration Framework and Versioning Strategy*
