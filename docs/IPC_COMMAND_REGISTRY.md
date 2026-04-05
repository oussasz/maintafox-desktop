# IPC Command Registry

Every Tauri IPC command exposed by the Maintafox Rust application core must be registered
in this file. Adding a command to `tauri::generate_handler![]` without a corresponding
entry here will fail the PR checklist review.

## Format

Each entry must include:
- **Command name** (exact Rust function name used in `generate_handler![]`)
- **Location** (file path in `src-tauri/src/commands/`)
- **Input types** (Rust struct name and TypeScript equivalent in `shared/ipc-types.ts`)
- **Output type** (Rust type and TypeScript equivalent)
- **Auth required** (whether the command requires a valid session)
- **Phase** (which phase or sprint introduced this command)
- **Description** (plain-language purpose of the command)

---

## Registered Commands

### `health_check`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/mod.rs` |
| **Input** | None |
| **Output** | `AppResult<HealthCheckResponse>` → `{ status, version, db_connected, locale }` |
| **TS Type** | `HealthCheckResponse` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `config` (read) |
| **TS Service** | `app.service.ts::healthCheck` |
| **Phase** | Phase 1 · Sub-phase 01 · File 01 · Sprint S1 (expanded in Sub-phase 02 · File 02 · S1) |
| **Description** | Returns application health status, version, DB connectivity, and default locale. Used by the frontend to verify the IPC bridge and managed state are operational on startup. |

---

### `get_app_info`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/app.rs` |
| **Input** | None |
| **Output** | `AppResult<AppInfoResponse>` → `{ version, build_mode, os, arch, app_name, default_locale }` |
| **TS Type** | `AppInfoResponse` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `config` (read) |
| **TS Service** | `app.service.ts::getAppInfo` |
| **Phase** | Phase 1 · Sub-phase 02 · File 02 · Sprint S3 |
| **Description** | Returns static build metadata (version, debug/release, OS, architecture) and runtime config (app name, default locale). Always callable before authentication. |

---

### `get_task_status`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/app.rs` |
| **Input** | None |
| **Output** | `AppResult<Vec<TaskStatusEntry>>` → `TaskStatusEntry[]` |
| **TS Type** | `TaskStatusEntry` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `tasks` (read) |
| **TS Service** | `app.service.ts::getTaskStatus` |
| **Phase** | Phase 1 · Sub-phase 02 · File 02 · Sprint S3 |
| **Description** | Returns the current status of all tracked background tasks. Returns an empty array in Phase 1 (no tasks spawned yet). Used for diagnostics and the startup experience. |

---

### `shutdown_app`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/app.rs` |
| **Input** | None |
| **Output** | None (process exits) |
| **TS Type** | — |
| **Auth required** | No (local desktop, any session) |
| **AppState fields** | `tasks` (shutdown) |
| **TS Service** | `invoke("shutdown_app")` |
| **Phase** | Phase 1 · Sub-phase 02 · File 04 · Sprint S3 |
| **Description** | Gracefully shuts down the application. Cancels all background tasks via the supervisor, then calls `app.exit(0)`. Intended for the quit menu item and tray "Quit" action. |
| **PRD Ref** | §14.2 — Reliability and Recovery |

---

### `list_lookup_domains`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/lookup.rs` |
| **Input** | `filter?: LookupDomainFilter`, `page?: PageRequest` |
| **Output** | `AppResult<Page<LookupDomainSummary>>` |
| **TS Type** | `Page<LookupDomainSummary>` in `shared/ipc-types.ts` |
| **Auth required** | No (Phase 1; RBAC added in Sub-phase 04) |
| **AppState fields** | `db` (read) |
| **TS Service** | `lookup-service.ts::listLookupDomains` |
| **Phase** | Phase 1 · Sub-phase 03 · File 03 · Sprint S3 |
| **Description** | Returns a paginated list of all lookup domains. Called by the Lookup Manager admin page and any filter panel that needs to enumerate available governed vocabularies. |
| **PRD Ref** | §6.13 Lookup Reference Data Manager |

---

### `get_lookup_values`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/lookup.rs` |
| **Input** | `domain_key: string` |
| **Output** | `AppResult<Vec<LookupValueOption>>` → `LookupValueOption[]` |
| **TS Type** | `LookupValueOption` in `shared/ipc-types.ts` |
| **Auth required** | No (read-only reference data) |
| **AppState fields** | `db` (read) |
| **TS Service** | `lookup-service.ts::getLookupValues` |
| **Phase** | Phase 1 · Sub-phase 03 · File 03 · Sprint S3 |
| **Description** | Returns all active values for a given domain key. Primary call for populating dropdowns, filter chips, and badge resolvers across all modules. |
| **PRD Ref** | §6.13 |

---

### `get_lookup_value_by_id`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/lookup.rs` |
| **Input** | `value_id: i32` |
| **Output** | `AppResult<LookupValueRecord>` |
| **TS Type** | `LookupValueRecord` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `db` (read) |
| **TS Service** | `lookup-service.ts::getLookupValueById` |
| **Phase** | Phase 1 · Sub-phase 03 · File 03 · Sprint S3 |
| **Description** | Resolves a single lookup value by its integer id. Called when rendering a stored FK as a labeled badge or detail field. |
| **Errors** | `NOT_FOUND`, `DATABASE_ERROR` |
| **PRD Ref** | §6.13 |

---

### `run_integrity_check`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/diagnostics.rs` |
| **Input** | None |
| **Output** | `AppResult<IntegrityReport>` → `{ is_healthy, is_recoverable, issues, seed_schema_version, domain_count, value_count }` |
| **TS Type** | `IntegrityReport` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `db` (read) |
| **TS Service** | `diagnostics-service.ts::runIntegrityCheck` |
| **Phase** | Phase 1 · Sub-phase 03 · File 04 · Sprint S2 |
| **Description** | Runs the database integrity check and returns a structured report. Called by the frontend on startup and from the diagnostics panel. Checks table existence, seed version, required domains, and minimum value counts. |
| **Errors** | `DATABASE_ERROR` |
| **PRD Ref** | §14.2 Reliability and Recovery |

---

### `repair_seed_data`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/diagnostics.rs` |
| **Input** | None |
| **Output** | `AppResult<IntegrityReport>` → post-repair `IntegrityReport` |
| **TS Type** | `IntegrityReport` in `shared/ipc-types.ts` |
| **Auth required** | No (only callable during startup recovery screen) |
| **AppState fields** | `db` (read + write) |
| **TS Service** | `diagnostics-service.ts::repairSeedData` |
| **Phase** | Phase 1 · Sub-phase 03 · File 04 · Sprint S2 |
| **Description** | Re-applies the system seed data idempotently and re-runs the integrity check. Used for self-repair when the integrity check found recoverable issues. Safe to call even if seed data is already present. |
| **Errors** | `DATABASE_ERROR` |
| **PRD Ref** | §14.2 Reliability and Recovery |

---

### `login`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/auth.rs` |
| **Input** | `LoginRequest { username: string, password: string }` |
| **Output** | `AppResult<LoginResponse>` → `{ session_info: SessionInfo }` |
| **TS Type** | `LoginRequest`, `LoginResponse`, `SessionInfo` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `db` (read + write), `session` (write) |
| **TS Service** | `auth-service.ts::login` |
| **Phase** | Phase 1 · Sub-phase 04 · File 01 · Sprint S2 |
| **Description** | Authenticate with a local username and password. On success, creates an in-memory session and returns session info. On failure, always returns the same opaque error message to prevent user enumeration. |
| **Errors** | `AUTH_ERROR: "Identifiant ou mot de passe invalide."` |
| **PRD Ref** | §6.1 Authentication & Session Management |

---

### `logout`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/auth.rs` |
| **Input** | None |
| **Output** | `AppResult<()>` → `null` |
| **TS Type** | — |
| **Auth required** | No (safe to call without session) |
| **AppState fields** | `session` (write) |
| **TS Service** | `auth-service.ts::logout` |
| **Phase** | Phase 1 · Sub-phase 04 · File 01 · Sprint S2 |
| **Description** | Clears the active session. Always succeeds, even if no session is active. |
| **Errors** | None |

---

### `get_session_info`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/auth.rs` |
| **Input** | None |
| **Output** | `AppResult<SessionInfo>` → `SessionInfo` |
| **TS Type** | `SessionInfo` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `session` (read) |
| **TS Service** | `auth-service.ts::getSessionInfo` |
| **Phase** | Phase 1 · Sub-phase 04 · File 01 · Sprint S2 |
| **Description** | Returns the current session state. Always returns a valid `SessionInfo` with `is_authenticated: false` when no session is active. Called by the frontend on startup to determine which screen to show. |
| **Errors** | None |
| **PRD Ref** | §6.1 |

---

### `get_device_trust_status`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/auth.rs` |
| **Input** | None |
| **Output** | `AppResult<DeviceTrustStatus>` → `DeviceTrustStatus` |
| **TS Type** | `DeviceTrustStatus` in `shared/ipc-types.ts` |
| **Auth required** | Yes |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `auth-service.ts::getDeviceTrustStatus` |
| **Phase** | Phase 1 · Sub-phase 04 · File 02 · Sprint S3 |
| **Description** | Returns the trust status of the current device for the logged-in user, including whether offline access is currently allowed and how many hours remain in the grace window. |
| **Errors** | `AUTH_ERROR` if no session |
| **PRD Ref** | §6.1 Trusted Device Registration |

---

### `revoke_device_trust`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/auth.rs` |
| **Input** | `{ device_id: string }` |
| **Output** | `AppResult<()>` → `null` |
| **TS Type** | — |
| **Auth required** | Yes + adm.users permission (SP04-F03) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `auth-service.ts::revokeDeviceTrust` |
| **Phase** | Phase 1 · Sub-phase 04 · File 02 · Sprint S3 |
| **Description** | Revokes offline trust for a specific device by row id. The device can still log in online. Used when a laptop is lost or stolen to prevent offline access with cached credentials. |
| **Errors** | `NOT_FOUND`, `AUTH_ERROR` |
| **PRD Ref** | §6.1, §12 Security |

---

### `get_my_permissions`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/rbac.rs` |
| **Input** | None |
| **Output** | `AppResult<Vec<PermissionRecord>>` → `PermissionRecord[]` |
| **TS Type** | `PermissionRecord` in `shared/ipc-types.ts` |
| **Auth required** | Yes |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `rbac-service.ts::getMyPermissions` |
| **Phase** | Phase 1 · Sub-phase 04 · File 03 · Sprint S3 |
| **Description** | Returns the effective permission set for the currently authenticated user. Called after login to populate the frontend permission store so that `<PermissionGate>` and `usePermissions().can()` work without per-check round-trips. |
| **Errors** | `AUTH_ERROR` if no session |
| **PRD Ref** | §6.7 Role-Based Access Control |

---

### `verify_step_up`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/rbac.rs` |
| **Input** | `StepUpRequest { password: string }` |
| **Output** | `AppResult<StepUpResponse>` → `{ success: boolean, expires_at: string }` |
| **TS Type** | `StepUpRequest`, `StepUpResponse` in `shared/ipc-types.ts` |
| **Auth required** | Yes |
| **AppState fields** | `db` (read), `session` (read + write) |
| **TS Service** | `rbac-service.ts::verifyStepUp` |
| **Phase** | Phase 1 · Sub-phase 04 · File 03 · Sprint S2 |
| **Description** | Verifies the user's password for dangerous-action step-up authorization. On success, records the verification timestamp in the in-memory session. The step-up window (120 seconds) starts from this moment. Subsequent calls to `require_step_up!` or `require_permission!` (for dangerous permissions) will pass within this window. |
| **Errors** | `AUTH_ERROR` if no session or wrong password |
| **PRD Ref** | §6.7 Dangerous Action Guards |

---

### `get_locale_preference`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/locale.rs` |
| **Input** | None |
| **Output** | `AppResult<LocalePreference>` |
| **TS Type** | `LocalePreference` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **AppState fields** | `db` (read) |
| **TS Service** | `locale-service.ts::getLocalePreference` |
| **Phase** | Phase 1 · Sub-phase 05 · File 01 · Sprint S3 |
| **Description** | Resolves locale through the precedence chain: user preference, tenant default, OS locale, then hard fallback (`fr`). Exposed without session so the login screen can initialize language correctly. |
| **PRD Ref** | §2.2 Strategic Objectives (Multilingual by design), §6.18 Settings |

---

### `set_locale_preference`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/locale.rs` |
| **Input** | `SetLocalePayload { locale: string, as_tenant_default?: boolean }` |
| **Output** | `AppResult<LocalePreference>` |
| **TS Type** | `LocalePreference` in `shared/ipc-types.ts` |
| **Auth required** | Yes (`require_session!`) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `locale-service.ts::setLocalePreference` |
| **Phase** | Phase 1 · Sub-phase 05 · File 01 · Sprint S3 |
| **Description** | Persists locale preference in `system_config`. Writes `locale.user_language` for user-level changes, or `locale.default_language` for tenant-level changes with `adm.settings` permission. Returns resolved locale payload after update. |
| **Errors** | `AUTH_ERROR` if no session, `PERMISSION_DENIED` for tenant-default without `adm.settings`, `VALIDATION_ERROR` for unsupported locale |
| **PRD Ref** | §2.2 Strategic Objectives (Multilingual by design), §6.18 Settings, §6.7 RBAC |

---

### `list_org_structure_models`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | None |
| **Output** | `AppResult<Vec<OrgStructureModel>>` → `OrgStructureModel[]` |
| **TS Type** | `OrgStructureModel` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.view` |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `org-service.ts::listOrgStructureModels` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Returns all structure models ordered by version number descending. Used by the org configuration UI to display model history and manage drafts. |
| **PRD Ref** | §6.2 Organization & Site Operating Model |

---

### `get_active_org_structure_model`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | None |
| **Output** | `AppResult<Option<OrgStructureModel>>` → `OrgStructureModel \| null` |
| **TS Type** | `OrgStructureModel` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.view` |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `org-service.ts::getActiveOrgStructureModel` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Returns the currently active structure model, or null if none has been published. Called by the org store on initialization and by any module that needs to know the current org schema. |
| **PRD Ref** | §6.2 |

---

### `create_org_structure_model`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `CreateStructureModelPayload { description?: string }` |
| **Output** | `AppResult<OrgStructureModel>` → `OrgStructureModel` |
| **TS Type** | `CreateStructureModelPayload`, `OrgStructureModel` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::createOrgStructureModel` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Creates a new structure model in draft status. Version number auto-increments from the highest existing version. Only admins with step-up can create models. |
| **PRD Ref** | §6.2 |

---

### `publish_org_structure_model`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `model_id: i32` |
| **Output** | `AppResult<OrgStructureModel>` → `OrgStructureModel` |
| **TS Type** | `OrgStructureModel` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::publishOrgStructureModel` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Publishes a draft model as the new active model. The previously active model transitions to superseded. Only draft models can be published. Validation (F04) must be called before this command. |
| **Errors** | `VALIDATION_ERROR` if model is not in draft status |
| **PRD Ref** | §6.2 |

---

### `archive_org_structure_model`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `model_id: i32` |
| **Output** | `AppResult<OrgStructureModel>` → `OrgStructureModel` |
| **TS Type** | `OrgStructureModel` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::archiveOrgStructureModel` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Archives a draft or superseded model. Active models cannot be archived — a new model must be published first. |
| **Errors** | `VALIDATION_ERROR` if model is active |
| **PRD Ref** | §6.2 |

---

### `list_org_node_types`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `structure_model_id: i32` |
| **Output** | `AppResult<Vec<OrgNodeType>>` → `OrgNodeType[]` |
| **TS Type** | `OrgNodeType` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.view` |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `org-service.ts::listOrgNodeTypes` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Returns all node types for a given structure model, ordered by depth hint and label. Used to populate the node type configuration panel and tree-builder constraints. |
| **PRD Ref** | §6.2 |

---

### `create_org_node_type`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `CreateNodeTypePayload { structure_model_id, code, label, icon_key?, depth_hint?, can_host_assets, can_own_work, can_carry_cost_center, can_aggregate_kpis, can_receive_permits, is_root_type }` |
| **Output** | `AppResult<OrgNodeType>` → `OrgNodeType` |
| **TS Type** | `CreateOrgNodeTypePayload`, `OrgNodeType` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::createOrgNodeType` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Creates a node type in a draft structure model. Validates code uniqueness within the model and enforces single root-type constraint. Only draft models accept new node types. |
| **Errors** | `VALIDATION_ERROR` if model not draft, code duplicate, or second root type |
| **PRD Ref** | §6.2 |

---

### `deactivate_org_node_type`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `node_type_id: i32` |
| **Output** | `AppResult<OrgNodeType>` → `OrgNodeType` |
| **TS Type** | `OrgNodeType` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::deactivateOrgNodeType` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Deactivates a node type. Cannot deactivate if org nodes of this type exist. Returns the updated node type with `is_active = false`. |
| **Errors** | `VALIDATION_ERROR` if nodes of this type exist |
| **PRD Ref** | §6.2 |

---

### `list_org_relationship_rules`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `structure_model_id: i32` |
| **Output** | `AppResult<Vec<OrgRelationshipRule>>` → `OrgRelationshipRule[]` |
| **TS Type** | `OrgRelationshipRule` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.view` |
| **AppState fields** | `db` (read), `session` (read) |
| **TS Service** | `org-service.ts::listOrgRelationshipRules` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Returns all relationship rules for a structure model with JOINed parent/child type labels for display. Ordered by parent label then child label. |
| **PRD Ref** | §6.2 |

---

### `create_org_relationship_rule`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `CreateRelationshipRulePayload { structure_model_id, parent_type_id, child_type_id, min_children?, max_children? }` |
| **Output** | `AppResult<OrgRelationshipRule>` → `OrgRelationshipRule` |
| **TS Type** | `CreateRelationshipRulePayload`, `OrgRelationshipRule` in `shared/ipc-types.ts` |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::createOrgRelationshipRule` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Creates a relationship rule in a draft model defining which node types can be children of which parent types. Prevents duplicate parent-child combinations. |
| **Errors** | `VALIDATION_ERROR` if model not draft or rule already exists |
| **PRD Ref** | §6.2 |

---

### `delete_org_relationship_rule`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/org.rs` |
| **Input** | `rule_id: i32` |
| **Output** | `AppResult<()>` → `null` |
| **TS Type** | — |
| **Auth required** | Yes + `org.admin` (dangerous, step-up) |
| **AppState fields** | `db` (read + write), `session` (read) |
| **TS Service** | `org-service.ts::deleteOrgRelationshipRule` |
| **Phase** | Phase 2 · Sub-phase 01 · File 01 · Sprint S3 |
| **Description** | Deletes a relationship rule from a draft model. Rules in published or superseded models cannot be deleted. |
| **Errors** | `NOT_FOUND`, `VALIDATION_ERROR` if model not draft |
| **PRD Ref** | §6.2 |

---

## Command Summary

| Command | Rust handler | Auth required | AppState fields used | TypeScript service |
|---------|-------------|--------------|---------------------|-----------------|
| `health_check` | `commands::health_check` | No | `config` (read) | `app.service.ts::healthCheck` |
| `get_app_info` | `commands::app::get_app_info` | No | `config` (read) | `app.service.ts::getAppInfo` |
| `get_task_status` | `commands::app::get_task_status` | No | `tasks` (read) | `app.service.ts::getTaskStatus` |
| `shutdown_app` | `commands::app::shutdown_app` | No | `tasks` (shutdown) | `invoke("shutdown_app")` |
| `list_lookup_domains` | `commands::lookup::list_lookup_domains` | No | `db` (read) | `lookup-service.ts::listLookupDomains` |
| `get_lookup_values` | `commands::lookup::get_lookup_values` | No | `db` (read) | `lookup-service.ts::getLookupValues` |
| `get_lookup_value_by_id` | `commands::lookup::get_lookup_value_by_id` | No | `db` (read) | `lookup-service.ts::getLookupValueById` |
| `run_integrity_check` | `commands::diagnostics::run_integrity_check` | No | `db` (read) | `diagnostics-service.ts::runIntegrityCheck` |
| `repair_seed_data` | `commands::diagnostics::repair_seed_data` | No | `db` (read + write) | `diagnostics-service.ts::repairSeedData` |
| `login` | `commands::auth::login` | No | `db` (read + write), `session` (write) | `auth-service.ts::login` |
| `logout` | `commands::auth::logout` | No | `session` (write) | `auth-service.ts::logout` |
| `get_session_info` | `commands::auth::get_session_info` | No | `session` (read) | `auth-service.ts::getSessionInfo` |
| `get_device_trust_status` | `commands::auth::get_device_trust_status` | Yes | `db` (read), `session` (read) | `auth-service.ts::getDeviceTrustStatus` |
| `revoke_device_trust` | `commands::auth::revoke_device_trust` | Yes + adm.users | `db` (read + write), `session` (read) | `auth-service.ts::revokeDeviceTrust` |
| `get_my_permissions` | `commands::rbac::get_my_permissions` | Yes | `db` (read), `session` (read) | `rbac-service.ts::getMyPermissions` |
| `verify_step_up` | `commands::rbac::verify_step_up` | Yes | `db` (read), `session` (read + write) | `rbac-service.ts::verifyStepUp` |
| `get_locale_preference` | `commands::locale::get_locale_preference` | No | `db` (read) | `locale-service.ts::getLocalePreference` |
| `set_locale_preference` | `commands::locale::set_locale_preference` | Yes | `db` (read + write), `session` (read) | `locale-service.ts::setLocalePreference` |
| `list_org_structure_models` | `commands::org::list_org_structure_models` | Yes + org.view | `db` (read), `session` (read) | `org-service.ts::listOrgStructureModels` |
| `get_active_org_structure_model` | `commands::org::get_active_org_structure_model` | Yes + org.view | `db` (read), `session` (read) | `org-service.ts::getActiveOrgStructureModel` |
| `create_org_structure_model` | `commands::org::create_org_structure_model` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::createOrgStructureModel` |
| `publish_org_structure_model` | `commands::org::publish_org_structure_model` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::publishOrgStructureModel` |
| `archive_org_structure_model` | `commands::org::archive_org_structure_model` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::archiveOrgStructureModel` |
| `list_org_node_types` | `commands::org::list_org_node_types` | Yes + org.view | `db` (read), `session` (read) | `org-service.ts::listOrgNodeTypes` |
| `create_org_node_type` | `commands::org::create_org_node_type` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::createOrgNodeType` |
| `deactivate_org_node_type` | `commands::org::deactivate_org_node_type` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::deactivateOrgNodeType` |
| `list_org_relationship_rules` | `commands::org::list_org_relationship_rules` | Yes + org.view | `db` (read), `session` (read) | `org-service.ts::listOrgRelationshipRules` |
| `create_org_relationship_rule` | `commands::org::create_org_relationship_rule` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::createOrgRelationshipRule` |
| `delete_org_relationship_rule` | `commands::org::delete_org_relationship_rule` | Yes + org.admin | `db` (read + write), `session` (read) | `org-service.ts::deleteOrgRelationshipRule` |

## Rules

1. Auth-required commands must validate the session before accessing any app data. Sub-phase 04 adds the `require_session!()` macro enforced at the top of each guarded handler.
2. Commands must NEVER return database entity objects directly. Always define a dedicated response struct.
3. All new IPC commands must be added here before they are merged to develop.

---

*Add new commands above the Rules section in the order they are implemented.*
*Do not remove entries for deprecated commands — mark them Deprecated with the replacing command.*
