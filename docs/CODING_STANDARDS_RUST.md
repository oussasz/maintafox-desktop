# Maintafox Desktop — Rust Coding Standards

This document defines the mandatory conventions for all Rust code in the Maintafox Desktop
backend (`src-tauri/`). Every contributor must follow these rules. Clippy with pedantic
lints and rustfmt enforce most of them at compile time.

---

## 1. Module Organization

The Rust codebase is organized into clearly separated modules, each with a single
responsibility. No module may reach outside its designated role.

**`commands/`** — IPC handler functions only. A command function is the entry point for
a frontend request. It deserializes parameters, validates inputs, delegates immediately
to a service function, and returns the result. Command functions must not contain business
logic, database queries, or file system access. They are thin wrappers that translate
between the IPC boundary and the service layer.

**`services/`** — All business logic lives here. Service functions are `async`, accept
typed parameters, and return `AppResult<T>`. They orchestrate operations across multiple
models, apply business rules, and coordinate database transactions. Service functions are
the primary unit of testability — they can be called in integration tests without the
Tauri runtime.

**`models/`** — Domain entity structs, `serde` serialization and deserialization
implementations, and entity validation logic. Models represent the shape of data as it
moves through the system. Each entity struct derives `Serialize` and `Deserialize` and
mirrors a database table or a logical domain object. Validation methods on models return
`AppResult<()>` and use `AppError::ValidationFailed` for rejections.

**`db/`** — Database connection setup, migration runner, and low-level query helpers. This
module owns the `sea-orm` `DatabaseConnection` and provides functions to initialize it,
run pending migrations, and execute raw queries when the ORM is insufficient. No business
logic lives here — `db/` is a utility layer consumed by `services/`.

**`auth/`** — Authentication and session management. Handles password hashing with Argon2,
session token generation, session validation, and logout. Coordinates with the OS keyring
for secure credential storage.

**`security/`** — Permission checking, capability enforcement, and data encryption
utilities. Provides guard functions that services call to verify the current user has
permission to perform an action before proceeding.

**`sync/`** — VPS synchronization logic. Manages the outbound sync queue, conflict
resolution, and inbound data merging. Sync operations are idempotent and retry-safe.

**`background/`** — Background task supervisor. Manages long-running tasks (scheduled
maintenance checks, sync polling, cleanup jobs) and provides health status reporting.

**`errors.rs`** — Contains `AppError` and `AppResult` only. No other code belongs here.
The error module is the single source of truth for all error variants in the application.

---

## 2. Error Handling Rules

All functions exposed beyond their own module must return `AppResult<T>` or a type that
wraps `AppError`. This guarantees that every error in the system flows through a single
unified type and is serializable across the IPC boundary.

Never use `.unwrap()` or `.expect()` in production code paths without an inline comment
explaining why a panic is the correct behavior at that exact point. The only acceptable
use of `.expect()` is for startup invariants — loading the Tauri context, initializing the
logger, or reading a compile-time constant. Every `.expect()` must be accompanied by an
`// EXPECT:` comment explaining the invariant.

Use the `?` operator to propagate errors through the call stack. This is the primary error
propagation mechanism. For invalid user inputs, construct
`AppError::ValidationFailed(vec![...])` with a list of human-readable validation messages.
For missing records, construct `AppError::NotFound { entity, id }` with the entity type
name and the identifier that was searched.

Do not swallow errors with `let _ = result`. If a `Result` is intentionally ignored, the
line must include a `// SAFETY:` comment explaining why the error can be safely discarded.
In most cases, ignoring a `Result` is a bug — either log the error or propagate it.

---

## 3. Async Conventions

All service functions that touch the database, filesystem, or network are `async`. The
Tokio runtime powers the async executor. Functions that do not perform I/O should remain
synchronous — do not mark a function `async` unless it awaits something.

Never block an async thread with synchronous I/O. File reads, CPU-intensive computations
(password hashing, report generation), and calls to blocking system APIs must be offloaded
to `tokio::task::spawn_blocking`. Every call to `spawn_blocking` must include an inline
comment explaining why blocking is necessary at that point. Example:

```rust
// BLOCKING: Argon2 password hashing is CPU-intensive and must not block the async executor.
let hash = tokio::task::spawn_blocking(move || hash_password(&plain)).await??;
```

Never fire-and-forget a spawned task without tracking it in the background task supervisor.
Untracked tasks that silently fail or panic are bugs. Every `tokio::spawn` call must
register the resulting `JoinHandle` with the background module so that failures are logged
and the application can report its health status accurately.

---

## 4. Logging Rules

The `tracing` crate is the only logging facility permitted in the codebase. The following
severity levels are used consistently:

**`tracing::info!`** — Normal operations. Use for significant lifecycle events that an
operator would want to see in a production log: command invoked, record created, migration
completed, sync cycle finished. Info logs should be concise and include relevant identifiers.

**`tracing::warn!`** — Recoverable anomalies. Use when something unexpected happened but
the system can continue: a network retry was triggered, a non-critical configuration value
is missing and a default was applied, a sync conflict was resolved automatically.

**`tracing::error!`** — Failures that affect correctness. Use when an operation failed and
the caller will receive an error: database write failed, authentication rejected, sync
payload was invalid. Error logs should include enough context to diagnose the problem.

**`tracing::debug!`** — Verbose diagnostic detail for development investigation. Use for
internal state dumps, query parameters, intermediate computation results. Debug logs are
disabled in production by default.

Never use `println!` or `eprintln!` in production code paths. All output goes through
`tracing`.

Never log secret values, passwords, session tokens, encryption keys, or private key
material. Log entity IDs and non-sensitive labels only. When logging a user action, log
the user ID and the action name — never the request body if it might contain credentials.

---

## 5. Security Rules

All user-supplied data entering a Tauri command must be validated before reaching the
service layer. Command functions validate input shapes, lengths, and ranges using dedicated
validation functions or Zod-equivalent Rust validation. Rejections return
`AppError::ValidationFailed` with a list of specific failure reasons.

Never construct SQL by string formatting with user input. Use `sea-orm` query builders or
`sqlx` parameterized queries exclusively. Any code that concatenates user input into a SQL
string is a critical security vulnerability and must be rejected in review. The codebase
uses prepared statements for every database interaction.

Secrets are stored in the OS keyring using the `keyring` crate. Never write passwords,
tokens, or encryption keys to SQLite rows, log output, environment variables read at
runtime, or IPC responses. The keyring is the only acceptable storage location for secret
material. When a secret is needed, it is read from the keyring into memory, used, and then
the variable is zeroized.

The `capabilities/default.json` file controls what the WebView is allowed to do. It defines
the security boundary between the frontend and the system. Never add a new capability
without documenting the reason in the PR description. Capabilities are reviewed with the
same scrutiny as dependency additions — each one expands the attack surface.

---

## 6. IPC Boundary Rules

IPC commands are the only entry point from the frontend into the Rust backend. Every
command function is registered in `tauri::generate_handler![]` in `lib.rs`. There is no
other mechanism for the frontend to execute Rust code.

Commands must validate inputs, delegate to services, and return `AppResult<T>`. No direct
database access in command functions — all data operations go through the service layer.
This separation ensures that business rules are enforced consistently regardless of whether
a service is called from an IPC command, a background job, or a test.

Every new command must be added to `docs/IPC_COMMAND_REGISTRY.md` in the same PR that
implements it. The registry documents the command name, input parameters, output type,
required permissions, and the sprint that introduced it. This serves as the canonical
reference for the IPC surface area.

IPC response types must implement `serde::Serialize`. They must also be mirrored as
TypeScript types in `shared/ipc-types.ts`. The Rust struct and the TypeScript interface
must have identical field names and compatible types. Keeping them in sync is the
responsibility of the developer implementing the command.
