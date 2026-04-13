# Developer Environment Guide

This document covers everything needed to set up, run, and troubleshoot the Maintafox
Desktop development environment.

---

## 1. Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Rust stable toolchain | latest stable | Install via [rustup.rs](https://rustup.rs) |
| Node.js | 20 LTS or later | Install from [nodejs.org](https://nodejs.org) |
| pnpm | 9+ | Auto-installed by the setup script if missing |
| Microsoft Edge WebView2 Runtime | latest | Windows only — install from [Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) |
| Visual C++ Build Tools | 2022 or later | Windows only — install VS Build Tools, select "Desktop development with C++" |
| DB Browser for SQLite | latest | Optional but recommended for database inspection — [sqlitebrowser.org](https://sqlitebrowser.org) |

---

## 2. One-Command Setup

### Windows

Open PowerShell in the project root and run:

```powershell
.\scripts\setup.ps1
```

### macOS / Linux

Open a terminal in the project root and run:

```bash
chmod +x scripts/setup.sh && ./scripts/setup.sh
```

### What the script does

1. Checks that Rust, Node.js, and pnpm are installed and meet version requirements
2. Checks platform-specific dependencies (WebView2 + MSVC on Windows; system libs on Linux)
3. Installs Node.js dependencies via `pnpm install`
4. Pre-fetches Rust dependencies via `cargo fetch`
5. Creates `.env.local` from `.env.example` if it does not exist
6. Runs the environment preflight checker to confirm everything is ready

After the script completes, start the application with:

```
pnpm run dev
```

---

## 3. Database Tooling

### Reset and seed the development database

```
pnpm run db:reset
```

This removes any existing database, creates all tables, and inserts baseline seed data.

### Seed data only (tables must already exist)

```
pnpm run db:seed
```

### Visual inspection

Open DB Browser for SQLite, then File → Open Database and select
`dev-data/maintafox_dev.db`.

### Tables

The following tables are created by the migration scripts:

- `system_config` — app-level key-value settings
- `trusted_devices` — devices that have completed online first-login
- `audit_events` — immutable append-only event journal
- `app_sessions` — active local sessions
- `user_accounts` — local identity records
- `roles` — system and custom authorization roles
- `permissions` — fine-grained permission definitions
- `role_permissions` — many-to-many join between roles and permissions
- `user_scope_assignments` — scoped role assignments for users

The `dev-data/` directory is gitignored and must never be committed.

---

## 4. Running the Application

Start in development mode:

```
pnpm run dev
```

- The Vite dev server starts on port 1420 and Tauri launches the desktop window.
- Hot Module Replacement (HMR) is active for React components — saving a `.tsx` file
  updates the running window without restarting.
- Rust code changes require a full restart: press Ctrl+C, then `pnpm run dev` again.
- Use F12 or Ctrl+Shift+I to open Developer Tools in the Tauri window.

---

## 5. Running Tests

| Command | Purpose |
|---------|---------|
| `pnpm run test` | Frontend tests (Vitest, runs once) |
| `pnpm run test:watch` | Frontend tests in watch mode |
| `pnpm run test -- --coverage` | Frontend coverage report |
| `pnpm run test:rust` | Rust unit tests (or `cd src-tauri && cargo test`) |

### Full CI-equivalent check (run in order)

```
pnpm run typecheck
pnpm run lint:check
pnpm run format:check
pnpm run test
pnpm run lint:rust
pnpm run format:rust:check
pnpm run test:rust
```

---

## 6. Common Issues and Fixes

| Problem | Cause | Fix |
|---|---|---|
| WebView2 not found | Missing runtime on Windows | Download from [Microsoft WebView2 page](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) and install |
| `cl.exe` not found | MSVC Build Tools missing | Install VS Build Tools with "Desktop development with C++" |
| Port 1420 already in use | Another dev server running | Run `pnpm run clean` or kill the process using port 1420 |
| `cargo check` fails with "missing feature" | SQLite dev library missing on Linux | Run `sudo apt-get install libsqlite3-dev` |
| `.env.local` missing | Setup script not run yet | Run `.\scripts\setup.ps1` |
| DB Browser shows empty tables | Seed not run | Run `pnpm run db:reset` |
| `SQLX_OFFLINE` error in CI | `.sqlx/` query cache missing | Run `cargo sqlx prepare` inside `src-tauri/` after schema stabilizes |
