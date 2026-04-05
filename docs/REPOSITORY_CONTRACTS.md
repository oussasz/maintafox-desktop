# Repository Contracts

This document is the binding reference for all Phase 2 sprint authors. Every function
listed here is a stable API contract. Do NOT change function signatures without
updating this document and all callers.

## Contract Rules

1. All repository functions are `async fn` returning `AppResult<T>`.
2. All filter structs implement `Default`. Callers may pass `Default::default()` for
   "no filter, return all".
3. All list functions accept a `PageRequest` and return `Page<T>`.
4. Soft-deleted records (`deleted_at IS NOT NULL`) are NEVER included unless the caller
   sets `SearchFilter::include_deleted = true`.
5. Repository functions never call IPC or frontend code.
6. All mutations (insert, update, delete) go through the service layer, never called
   directly from IPC command handlers.

---

## `lookup_repository` — `src-tauri/src/repository/lookup_repository.rs`

| Function | Parameters | Returns | Notes |
|----------|-----------|---------|-------|
| `list_lookup_domains` | `db`, `LookupDomainFilter`, `PageRequest` | `Page<LookupDomainSummary>` | Includes value_count subquery |
| `get_domain_values` | `db`, `domain_key: &str`, `active_only: bool` | `Vec<LookupValueOption>` | Hot path for dropdowns |
| `get_value_by_id` | `db`, `value_id: i32` | `LookupValueRecord` | Returns NotFound if absent |
| `get_value_by_code` | `db`, `domain_key: &str`, `code: &str` | `LookupValueRecord` | Used for import mapping |
| `insert_lookup_value` | `db`, domain_id, code, label, fr_label, en_label, sort_order, color, actor_sync_id | `i32` (new id) | Validate via service layer first |

---

## `org_repository` — `src-tauri/src/repository/org_repository.rs`

| Function | Parameters | Returns | Notes |
|----------|-----------|---------|-------|
| `get_org_tree` | `db` | `Vec<OrgNodeTreeRow>` | Full tree, ordered by depth |
| `get_descendants` | `db`, `ancestor_path_prefix: &str` | `Vec<OrgNodeTreeRow>` | O(1) with index on ancestor_path |
| `get_node_by_id` | `db`, `node_id: i32` | `OrgNodeTreeRow` | Returns NotFound if absent |
| `get_asset_host_nodes` | `db` | `Vec<OrgNodeOption>` | Filtered by can_host_assets capability |
| `get_work_owner_nodes` | `db` | `Vec<OrgNodeOption>` | Filtered by can_own_work capability |

---

## `equipment_repository` — `src-tauri/src/repository/equipment_repository.rs`

| Function | Parameters | Returns | Notes |
|----------|-----------|---------|-------|
| `list_equipment` | `db`, `EquipmentFilter`, `PageRequest` | `Page<EquipmentListRow>` | Joins class, node, criticality |
| `get_equipment_by_id` | `db`, `equipment_id: i32` | `EquipmentDetail` | Full detail including all FKs resolved |

---

## `team_repository` — `src-tauri/src/repository/team_repository.rs`

| Function | Parameters | Returns | Notes |
|----------|-----------|---------|-------|
| `list_active_teams` | `db` | `Vec<TeamSummary>` | Active teams only |
| `list_all_skills` | `db` | `Vec<SkillDefinitionRow>` | Active skills with category |
| `get_team_by_id` | `db`, `team_id: i32` | `TeamSummary` | Returns NotFound if absent |

---

## Frontend Service Contracts — `src/services/lookup-service.ts`

| Function | Arguments | Returns | Notes |
|----------|-----------|---------|-------|
| `getLookupValues(domainKey)` | `domainKey: string` | `Promise<LookupValueOption[]>` | Primary dropdown source |
| `getLookupValueById(valueId)` | `valueId: number` | `Promise<LookupValueRecord>` | FK resolution |
| `listLookupDomains(filter?, page?)` | optional | `Promise<Page<LookupDomainSummary>>` | Admin use |

---

## Adding a New Repository in Phase 2

1. Create `src-tauri/src/repository/<module>_repository.rs`
2. Declare `pub mod <module>_repository;` in `repository/mod.rs`
3. Follow the existing DTO + filter + function pattern
4. Use `Statement::from_sql_and_values` for all queries with user-supplied values
5. Add `AppResult<T>` return type to all functions
6. Add unit tests for any pure logic (pagination calculations, code validation)
7. Document the new functions in this file before the sprint is marked complete
