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
| **Output** | `AppResult<serde_json::Value>` → `{ status: "ok" \| "error"; version: string }` |
| **TS Type** | `HealthCheckResponse` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **Phase** | Phase 1 · Sub-phase 01 · File 01 · Sprint S1 |
| **Description** | Returns the application health status and version string. Used by the frontend to verify the IPC bridge is operational on startup. |

---

*Add new commands below this line in the order they are implemented.*
*Do not remove entries for deprecated commands — mark them Deprecated with the replacing command.*
