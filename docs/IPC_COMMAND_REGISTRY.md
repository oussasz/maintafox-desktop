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

## Command Summary

| Command | Rust handler | Auth required | AppState fields used | TypeScript service |
|---------|-------------|--------------|---------------------|-----------------|
| `health_check` | `commands::health_check` | No | `config` (read) | `app.service.ts::healthCheck` |
| `get_app_info` | `commands::app::get_app_info` | No | `config` (read) | `app.service.ts::getAppInfo` |
| `get_task_status` | `commands::app::get_task_status` | No | `tasks` (read) | `app.service.ts::getTaskStatus` |

## Rules

1. Auth-required commands must validate the session before accessing any app data. Sub-phase 04 adds the `require_session!()` macro enforced at the top of each guarded handler.
2. Commands must NEVER return database entity objects directly. Always define a dedicated response struct.
3. All new IPC commands must be added here before they are merged to develop.

---

*Add new commands above the Rules section in the order they are implemented.*
*Do not remove entries for deprecated commands — mark them Deprecated with the replacing command.*
