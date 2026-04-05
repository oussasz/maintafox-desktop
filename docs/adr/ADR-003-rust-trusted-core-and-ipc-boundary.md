# ADR-003: Rust Trusted Core with Narrow IPC Boundary

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

In a Tauri application, the WebView (React) and the native core (Rust) are separate
execution environments. React code runs inside a sandboxed WebView; it cannot directly
access files, databases, OS keyrings, cryptographic material, or system processes. The
boundary between the two contexts is the IPC layer.

This boundary can be made wide (expose many native operations) or narrow (expose only
typed, validated, business-meaningful commands). Each approach has different security and
maintainability implications.

## Decision

We will maintain a **narrow, explicitly typed IPC boundary**. The React layer may only
call named Tauri commands exposed in `src-tauri/src/commands/`. Each command validates
inputs, delegates to a service function in `src-tauri/src/services/`, and returns a
typed `AppResult<T>`. No raw filesystem paths, database handles, or secret material ever
cross the IPC boundary.

## Rationale

- A narrow boundary means the Tauri `capabilities/default.json` can be minimal; we never
  expose shell execution, arbitrary filesystem access, or raw OS API calls to the WebView
- Every IPC command is a reviewed, named entry point — adding a new command requires
  deliberate action (registering in `tauri::generate_handler![]`, documenting in
  `IPC_COMMAND_REGISTRY.md`, reviewing capabilities scope)
- `AppError` serialization at the boundary means the frontend always receives structured
  error codes — no raw Rust panics or OS error strings leak to the UI
- Type-safe contracts in `shared/ipc-types.ts` mirror Rust structs, making breaking
  changes fail at CI compile time rather than at runtime
- Input validation at the command layer (before the service layer) is the last enforced
  security gate before business logic executes

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Wide IPC (expose fs, sqlite, shell directly) | Expands attack surface; WebView compromise can read arbitrary files and execute commands |
| HTTP server on localhost | Violates Tauri architecture guidelines; opens port-based attack surface; requires CORS policy management |
| postMessage only (no typed commands) | No compile-time contract; brittle string-based API; no input validation enforcement |

## Consequences

**Positive outcomes:**
- WebView compromise cannot directly read database files, access OS secrets, or execute
  arbitrary commands
- Breaking changes in IPC contracts are caught at TypeScript compile time
- Every privileged operation in the application can be audited by reading `commands/`

**Trade-offs and costs:**
- Every new backend capability requires a new typed command — no shortcut to "just call
  the backend" from a component
- IPC commands must be maintained in a registry document, adding a documentation step to
  each backend feature sprint

## Linked Resources

- PRD Section 3 — System Architecture Overview (WebView and Rust trust domains)
- PRD Section 12 — Security Architecture
- `docs/CODING_STANDARDS_RUST.md` — IPC Boundary Rules
- `docs/IPC_COMMAND_REGISTRY.md`
