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
