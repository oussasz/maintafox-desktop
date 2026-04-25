# Phase 1 · Sub-phase 06 · File 02
# Updater Skeleton and Release Channel Contracts

## Context and Purpose

File 01 delivered the settings persistence layer. The default settings seeded in F01
include `updater.release_channel` (default `"stable"`) and `updater.auto_check` (default
`true`). This file wires those settings into a functional in-app updater subsystem.

The Tauri auto-updater (PRD §11.1–§11.4) allows the application to check for new
releases from a remote manifest, show release notes to the user, and install updates
in a single user-approved action. Phase 1 does not require a live manifest URL — it
requires that:

1. The plumbing is correct — the Tauri plugin is declared, the commands are registered,
   and the frontend service and store exist.
2. The release channel setting injected in F01 is consumed by the updater plumbing so
   it can be wired to a real URL in Phase 2 DevOps work.
3. The updater signing key architecture is documented clearly so the Phase 2 signing
   setup does not accidentally reuse the session or entitlement key material.

This file delivers all three. The update-check response in a development environment
will return `available: false` from a stub manifest — this is correct behavior for
Phase 1.

## Architecture Rules Applied

- **Tauri plugin:** `tauri-plugin-updater` v2 is the official Tauri 2 update plugin.
  It enforces HTTPS for the manifest URL and cryptographic signature verification for
  the update bundle. Non-HTTPS is disabled in production builds (PRD §11.3).
- **Release channel → manifest URL mapping:** The mapping from `stable | pilot | internal`
  to actual manifest URLs is defined in `tauri.conf.json` (environment-specific) and
  read at runtime via the `TAURI_UPDATER_URL` or similar environment variable. The
  commands in this file read the channel from settings and pass it to the plugin.
- **No session required for `check_for_update`.** The update check runs before login
  and in the background at device startup. It must not be blocked by session state.
  However `install_pending_update` does require an active session (the user must be
  authenticated to trigger an install that restarts the application).
- **Updater key separation:** The updater bundle signing key is a different key pair
  from the session signing key (JWT RS256) and from the device entitlement key (SP04).
  The public half is embedded in `tauri.conf.json`. A compromise of the session key
  cannot allow a malicious actor to push a fake update.
- **Stub manifest in Phase 1:** `tauri.conf.json` in the development profile should
  point to a local JSON file that always returns no update. The Phase 2 CI/CD task
  will replace this with the production manifest URL.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/Cargo.toml` (patch) | Add `tauri-plugin-updater` dependency |
| `src-tauri/tauri.conf.json` (patch) | Add updater plugin configuration block |
| `src-tauri/src/commands/updater.rs` | IPC: `check_for_update`, `install_pending_update` |
| `src-tauri/src/lib.rs` (patch) | Register updater plugin + IPC commands |
| `src/services/updater-service.ts` | Frontend IPC wrappers |
| `src/stores/updater-store.ts` | Zustand store: available/version/notes/isInstalling |
| `src/hooks/use-updater.ts` | `useUpdater()` hook: interval check, auto-notify |
| `docs/UPDATER_SIGNING.md` | Key separation architecture reference |

## Prerequisites

- SP06-F01 complete: `updater.release_channel` and `updater.auto_check` default settings
  exist in `app_settings`
- SP04-F01 complete: `require_session!` macro available
- `settings-service.ts` complete: `getSetting()` available for reading release channel

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Tauri Updater Plugin and Rust Commands | Cargo.toml patch, tauri.conf.json patch, `commands/updater.rs` |
| S2 | Frontend Service and Store | `updater-service.ts`, `updater-store.ts`, `use-updater.ts` |
| S3 | Signing Key Architecture Doc and Integration Test | `docs/UPDATER_SIGNING.md`, startup check integration |

---

## Sprint S1 — Tauri Updater Plugin and Rust Commands

### AI Agent Prompt

```
You are a senior Rust and Tauri engineer. The settings system from SP06-F01 is in place.
Your task is to add the Tauri updater plugin and write the IPC commands that expose
update checking to the frontend.

────────────────────────────────────────────────────────────────────
STEP 1 — PATCH src-tauri/Cargo.toml
────────────────────────────────────────────────────────────────────
Add to [dependencies]:
```toml
tauri-plugin-updater = "2"
```

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src-tauri/tauri.conf.json
────────────────────────────────────────────────────────────────────
Add the updater plugin configuration. In Phase 1, the manifest URL points to a local
stub file that always returns no update. The real channel-specific URLs are injected
in Phase 2 by the CI/CD pipeline.

In the `"plugins"` section of `tauri.conf.json`, add:
```json
{
  "plugins": {
    "updater": {
      "pubkey": "PLACEHOLDER_UPDATER_PUBLIC_KEY_REPLACE_IN_PHASE2",
      "endpoints": [
        "https://updates.maintafox.local/stub/{{target}}/{{arch}}/{{current_version}}"
      ],
      "dialog": false,
      "windows": {
        "installMode": "basicUi"
      }
    }
  }
}
```

NOTE on the pubkey placeholder: The Phase 2 DevOps sprint will generate the actual
key pair with `tauri signer generate` and replace this placeholder with the real
public key. Until then, the plugin will fail signature verification on any update
it encounters — which is the correct safe behavior for Phase 1 (no live manifest).

────────────────────────────────────────────────────────────────────
STEP 3 — CREATE src-tauri/src/commands/updater.rs
────────────────────────────────────────────────────────────────────
```rust
//! Updater commands.
//!
//! check_for_update   — runs before login, no session required
//! install_pending_update — requires active session (app will restart)
//!
//! The release channel is read from app_settings at check time so an admin
//! can switch channels through the Settings UI without restarting.

use crate::{
    auth::AuthState,
    errors::{AppError, AppResult},
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

/// Check the update manifest for a newer version.
/// Does NOT require an active session. Safe to call from startup or login screen.
/// Returns `UpdateCheckResult` — `available: false` when no update is found or
/// when the manifest endpoint is unreachable (graceful degradation).
#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
) -> AppResult<UpdateCheckResult> {
    let updater = app.updater().map_err(|e| {
        AppError::Internal(format!("updater plugin not initialized: {}", e))
    })?;

    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!(
                new_version = %update.version,
                "update available"
            );
            Ok(UpdateCheckResult {
                available: true,
                version: Some(update.version.clone()),
                notes: update.body.clone(),
                pub_date: update.date.map(|d| d.to_string()),
            })
        }
        Ok(None) => {
            tracing::debug!("no update available");
            Ok(UpdateCheckResult {
                available: false,
                version: None,
                notes: None,
                pub_date: None,
            })
        }
        Err(e) => {
            // Update check failures are non-fatal — the app works without updates.
            tracing::warn!("update check failed (non-fatal): {}", e);
            Ok(UpdateCheckResult {
                available: false,
                version: None,
                notes: None,
                pub_date: None,
            })
        }
    }
}

/// Download and install a pending update.
/// Requires an active authenticated session — the user must be present to
/// approve an action that will restart the application.
/// The frontend must show a confirmation dialog before calling this command.
#[tauri::command]
pub async fn install_pending_update(
    app: tauri::AppHandle,
    state: State<'_, AuthState>,
) -> AppResult<()> {
    let _user = require_session!(state);

    let updater = app.updater().map_err(|e| {
        AppError::Internal(format!("updater plugin not initialized: {}", e))
    })?;

    let update = updater.check().await.map_err(|e| {
        AppError::Internal(format!("failed to check for update before install: {}", e))
    })?;

    match update {
        Some(update) => {
            tracing::info!(
                new_version = %update.version,
                "installing update — application will restart"
            );
            // Download + verify signature + apply. If signature verification fails,
            // tauri-plugin-updater returns an error here and does NOT apply the update.
            update
                .download_and_install(|_chunk, _total| {}, || {})
                .await
                .map_err(|e| AppError::Internal(format!("update install failed: {}", e)))?;
            Ok(())
        }
        None => Err(AppError::NotFound {
            entity: "pending_update".to_string(),
            id: "current".to_string(),
        }),
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 4 — PATCH src-tauri/src/lib.rs
────────────────────────────────────────────────────────────────────
Register the updater plugin and the new commands.

In the Tauri builder, add `.plugin(tauri_plugin_updater::Builder::new().build())`:

```rust
tauri::Builder::default()
    // ... existing plugins from SP01–SP05 ...
    .plugin(tauri_plugin_updater::Builder::new().build())
    .invoke_handler(tauri::generate_handler![
        // ... existing handlers ...
        commands::updater::check_for_update,
        commands::updater::install_pending_update,
    ])
```

Ensure `src-tauri/src/commands/mod.rs` includes:
```rust
pub mod updater;
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- `pnpm run tauri dev` starts without panic
- DevTools → Console: `await window.__TAURI__.invoke('check_for_update')` returns
  `{ available: false, version: null, notes: null, pub_date: null }` (stub returns no update)
- `install_pending_update` without a session returns an auth error
```

---

### Supervisor Verification — Sprint S1

**V1 — Plugin compiles.**
Run `cargo check` inside `src-tauri/`. The output must contain 0 errors. Warnings about
unused items in the updater plugin stub code are acceptable but should be reviewed.

**V2 — check_for_update returns graceful result.**
With the application running, open DevTools → Console and run:
```javascript
await window.__TAURI__.invoke('check_for_update')
```
Expected result: `{ available: false, ... }`. If the result is a rejection or an error
object, the updater plugin failed to initialize or the manifest endpoint is blocking.

**V3 — Unauthorized install is rejected.**
Without logging in, run:
```javascript
await window.__TAURI__.invoke('install_pending_update')
```
Expected: promise rejects with an error code containing "authentication" or "session".
If it proceeds, the `require_session!` guard is missing.

---

## Sprint S2 — Frontend Service and Store

### AI Agent Prompt

```
You are a TypeScript and React engineer. The Rust updater commands are registered.
Write the frontend service, Zustand store, and React hook for the updater subsystem.

────────────────────────────────────────────────────────────────────
PATCH shared/ipc-types.ts — add updater types
────────────────────────────────────────────────────────────────────
```typescript
// Add to existing shared/ipc-types.ts

export interface UpdateCheckResult {
  available: boolean;
  version: string | null;
  notes: string | null;
  pub_date: string | null;
}
```

────────────────────────────────────────────────────────────────────
CREATE src/services/updater-service.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * updater-service.ts
 *
 * IPC wrappers for update-check and install commands.
 * RULE: Only this file calls invoke('check_for_update') and
 *       invoke('install_pending_update'). Components use the
 *       useUpdater() hook.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { UpdateCheckResult } from "@shared/ipc-types";

const UpdateCheckResultSchema = z.object({
  available: z.boolean(),
  version: z.string().nullable(),
  notes: z.string().nullable(),
  pub_date: z.string().nullable(),
});

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  const raw = await invoke<UpdateCheckResult>("check_for_update");
  return UpdateCheckResultSchema.parse(raw);
}

export async function installPendingUpdate(): Promise<void> {
  await invoke<void>("install_pending_update");
}
```

────────────────────────────────────────────────────────────────────
CREATE src/stores/updater-store.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * updater-store.ts
 *
 * Tracks update-check state across the application lifetime.
 * The notification banner reads from this store.
 */

import { create } from "zustand";

import { checkForUpdate, installPendingUpdate } from "../services/updater-service";
import type { UpdateCheckResult } from "@shared/ipc-types";

interface UpdaterState {
  /** Last check result — null before first check */
  lastCheckResult: UpdateCheckResult | null;
  /** True while a check is in progress */
  isChecking: boolean;
  /** True while an install is downloading/applying */
  isInstalling: boolean;
  /** True after install completes (app will restart shortly) */
  installComplete: boolean;
  /** Non-null if the last check or install failed */
  error: string | null;

  checkForUpdate: () => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissNotification: () => void;
}

export const useUpdaterStore = create<UpdaterState>((set) => ({
  lastCheckResult: null,
  isChecking: false,
  isInstalling: false,
  installComplete: false,
  error: null,

  checkForUpdate: async () => {
    set({ isChecking: true, error: null });
    try {
      const result = await checkForUpdate();
      set({ lastCheckResult: result, isChecking: false });
    } catch (err) {
      // Non-fatal — update check failure does not block the application
      set({
        isChecking: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  installUpdate: async () => {
    set({ isInstalling: true, error: null });
    try {
      await installPendingUpdate();
      set({ isInstalling: false, installComplete: true });
    } catch (err) {
      set({
        isInstalling: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  dismissNotification: () =>
    set({ lastCheckResult: null, error: null, installComplete: false }),
}));
```

────────────────────────────────────────────────────────────────────
CREATE src/hooks/use-updater.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * use-updater.ts
 *
 * React hook for update notifications. Performs an initial check on mount,
 * then rechecks at the configured interval. Only runs when the user is
 * authenticated (update install requires a session).
 *
 * Usage:
 *   const { available, version, notes, isInstalling, install, dismiss }
 *     = useUpdater();
 */

import { useEffect, useCallback } from "react";

import { useUpdaterStore } from "../stores/updater-store";

// Check for updates every 2 hours when the app is running
const CHECK_INTERVAL_MS = 2 * 60 * 60 * 1000;

export interface UseUpdaterResult {
  available: boolean;
  version: string | null;
  notes: string | null;
  isChecking: boolean;
  isInstalling: boolean;
  installComplete: boolean;
  error: string | null;
  checkNow: () => void;
  install: () => void;
  dismiss: () => void;
}

export function useUpdater(): UseUpdaterResult {
  const {
    lastCheckResult,
    isChecking,
    isInstalling,
    installComplete,
    error,
    checkForUpdate,
    installUpdate,
    dismissNotification,
  } = useUpdaterStore();

  // Initial check on mount + interval
  useEffect(() => {
    void checkForUpdate();
    const intervalId = setInterval(() => void checkForUpdate(), CHECK_INTERVAL_MS);
    return () => clearInterval(intervalId);
  }, [checkForUpdate]);

  const checkNow = useCallback(() => void checkForUpdate(), [checkForUpdate]);
  const install = useCallback(() => void installUpdate(), [installUpdate]);
  const dismiss = useCallback(() => dismissNotification(), [dismissNotification]);

  return {
    available: lastCheckResult?.available ?? false,
    version: lastCheckResult?.version ?? null,
    notes: lastCheckResult?.notes ?? null,
    isChecking,
    isInstalling,
    installComplete,
    error,
    checkNow,
    install,
    dismiss,
  };
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- DevTools → Application → Zustand shows `useUpdaterStore` with:
  - `available: false` (stub manifest)
  - `isChecking: false` (after initial check completes)
  - `error: null`
- No console errors when the update check runs in the background
```

---

### Supervisor Verification — Sprint S2

**V1 — Update store populates after startup.**
Run `pnpm run tauri dev`. After the login screen appears, wait ~3 seconds for the initial
background update check. Open DevTools Console and verify the Zustand `useUpdaterStore`
state contains `{ available: false, isChecking: false, error: null }`.

**V2 — Check interval timer is set.**
In DevTools → Sources → Event Listeners or Performance, confirm an interval timer is
registered. The period should be approximately 7,200,000 ms (2 hours). This confirms the
`setInterval` in `use-updater.ts` is running.

**V3 — Type check passing.**
Run `pnpm run typecheck` from the project root. Output must show 0 errors related to
the updater types.

---

## Sprint S3 — Signing Key Architecture Documentation and Integration

### AI Agent Prompt

```
You are a senior security architect and technical writer. Your task is to write the
updater signing key architecture document and integrate the update-check into the
application startup sequence referenced by Phase 2.

────────────────────────────────────────────────────────────────────
CREATE docs/UPDATER_SIGNING.md
────────────────────────────────────────────────────────────────────
```markdown
# Maintafox — Updater Signing Key Architecture

## Overview

Maintafox uses **three separate key pairs** for cryptographic operations:

| Key Pair | Purpose | Location |
|----------|---------|----------|
| Session signing key (RS256) | JWT session tokens | SP04 — never exported from device |
| Device entitlement key | Trusted-device registration | SP04 — device-specific |
| **Updater signing key (Ed25519)** | **Authenticates update bundles** | This document |

These key pairs are independent. A compromise of the session signing key cannot be used
to push a malicious update bundle, and a compromise of the updater key cannot elevate
session privileges.

## Why Key Separation Matters

The update flow in `tauri-plugin-updater` works as follows:

1. The application fetches a manifest from the configured HTTPS endpoint.
2. The manifest contains: `version`, `pub_date`, `notes`, `url`, and `signature`.
3. The plugin downloads the update bundle from `url`.
4. It verifies the bundle against `signature` using the **updater public key** embedded
   in `tauri.conf.json`.
5. If verification fails, the bundle is discarded and the update is aborted.

If the updater public key in `tauri.conf.json` were the same as the session signing
key, a session key compromise would also allow bundle forgery. Separate key material
prevents this attack path.

## Key Generation (Phase 2 DevOps Task)

The updater key pair is generated once per product and lives in the CI/CD secrets vault.
It is **not stored in source control** and **not stored on developer machines**.

```bash
# Run in the CI/CD environment (NOT on developer machines)
# Requires Tauri CLI installed
tauri signer generate -w ~/.tauri/maintafox-updater.key

# Output:
#   Private key: ~/.tauri/maintafox-updater.key   ← KEEP SECRET, store in CI vault
#   Public key:  STDOUT (base64 string)            ← embed in tauri.conf.json
```

## Embedding the Public Key

The public key (base64 string) from `tauri signer generate` is embedded in
`tauri.conf.json` under `plugins.updater.pubkey`. It is safe to commit this value
to source control — it is a public key.

The private key is stored only in:
- The CI/CD secrets vault (e.g., GitHub Actions Secret `TAURI_UPDATER_PRIVATE_KEY`)
- Optionally in a hardware security module (HSM) for production signing

The private key is uses during the build process to sign the update bundle:
```bash
tauri build --sign-update  # signs using TAURI_PRIVATE_KEY environment variable
```

## Phase 1 Placeholder

In Phase 1, `tauri.conf.json` contains:
```json
"pubkey": "PLACEHOLDER_UPDATER_PUBLIC_KEY_REPLACE_IN_PHASE2"
```

With this placeholder, the plugin initializes successfully but any update it encounters
will fail signature verification and be discarded. This is **correct and safe** behavior
for Phase 1 — it means a misconfigured manifest cannot accidentally push an unsigned
update.

## Release Channel Architecture

The release channels (stable / pilot / internal) map to different manifest endpoints,
all signed with the same updater key pair:

| Channel | Audience | URL Pattern |
|---------|---------|-------------|
| `stable` | All customers | `https://updates.maintafox.com/stable/...` |
| `pilot` | Early-access customers | `https://updates.maintafox.com/pilot/...` |
| `internal` | Internal testing only | `https://updates.maintafox.com/internal/...` |

The active channel is stored in `app_settings` under key `updater.release_channel`
(scope: `device`). An admin can change the channel in the Settings UI, which triggers a
new update check against the new manifest endpoint.

## Manifest Contract

All manifest endpoints must return JSON conforming to:

```json
{
  "version": "1.2.3",
  "pub_date": "2026-06-15T00:00:00Z",
  "notes": "Release notes in markdown format",
  "url": "https://updates.maintafox.com/stable/maintafox-1.2.3-x86_64.msi.zip",
  "signature": "base64-encoded-Ed25519-signature-of-bundle"
}
```

When no update is available, the manifest endpoint returns HTTP 204 (No Content) or
an empty JSON object `{}`. The Tauri updater plugin handles both responses as
"no update available".

## Security Notes

1. **HTTPS is mandatory.** The updater plugin refuses HTTP endpoints in production
   builds. Non-HTTPS fallback is disabled in the production `tauri.conf.json`.

2. **No update without signature.** If `signature` is missing or invalid, the update
   is silently discarded. The application continues running the existing version.

3. **Session key is never used for update verification.** The `AuthState` and active
   session are not consulted during bundle verification — the trust anchor is solely
   the embedded public key in `tauri.conf.json`.

4. **Key rotation.** If the updater private key is compromised, generate a new key pair,
   push a new public key in `tauri.conf.json`, release that build as a security patch
   via the old key, then retire the old key. All subsequent builds use the new key.

## References

- Tauri v2 updater documentation: https://tauri.app/plugin/updater/
- `tauri-plugin-updater` crate: https://crates.io/crates/tauri-plugin-updater
- PRD §11.1–§11.4: Automatic Update System
- PRD §12.5: Key Separation Architecture
```

────────────────────────────────────────────────────────────────────
CREATE src/__tests__/services/updater-service.test.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * updater-service.test.ts
 *
 * Unit tests for the updater service. All IPC calls are mocked.
 * These tests verify that:
 * 1. The Zod schema validates correct responses
 * 2. The store transitions through correct states
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// Mock invoke before importing service
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { checkForUpdate } from "../../services/updater-service";
import { useUpdaterStore } from "../../stores/updater-store";

const mockInvoke = vi.mocked(invoke);

describe("updater-service", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset store state
    useUpdaterStore.setState({
      lastCheckResult: null,
      isChecking: false,
      isInstalling: false,
      installComplete: false,
      error: null,
    });
  });

  it("returns available=false when no update exists", async () => {
    mockInvoke.mockResolvedValueOnce({
      available: false,
      version: null,
      notes: null,
      pub_date: null,
    });

    const result = await checkForUpdate();
    expect(result.available).toBe(false);
    expect(result.version).toBeNull();
  });

  it("returns update data when an update is available", async () => {
    mockInvoke.mockResolvedValueOnce({
      available: true,
      version: "1.2.0",
      notes: "Bug fixes and improvements",
      pub_date: "2026-04-15T00:00:00Z",
    });

    const result = await checkForUpdate();
    expect(result.available).toBe(true);
    expect(result.version).toBe("1.2.0");
  });

  it("store: isChecking transitions to false after check", async () => {
    mockInvoke.mockResolvedValueOnce({
      available: false,
      version: null,
      notes: null,
      pub_date: null,
    });

    const { result } = renderHook(() => useUpdaterStore());
    await act(async () => {
      await result.current.checkForUpdate();
    });

    expect(result.current.isChecking).toBe(false);
    expect(result.current.lastCheckResult?.available).toBe(false);
  });

  it("store: error is set when invoke throws", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("network unreachable"));

    const { result } = renderHook(() => useUpdaterStore());
    await act(async () => {
      await result.current.checkForUpdate();
    });

    expect(result.current.error).toContain("network unreachable");
    expect(result.current.isChecking).toBe(false);
  });
});
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run test` passes: updater-service.test.ts — 4 tests pass, 0 failures
- `docs/UPDATER_SIGNING.md` is committed to the repository
- `pnpm run typecheck` passes with 0 errors
```

---

### Supervisor Verification — Sprint S3

**V1 — Tests pass.**
Run `pnpm run test -- updater-service`. All 4 tests must pass with 0 failures.
If any test fails, check that the Zustand store reset in `beforeEach` is working and
that the `invoke` mock is correctly clearing between tests.

**V2 — Signing documentation is present.**
Confirm `docs/UPDATER_SIGNING.md` exists and contains the three-key-pair table
(session key, device entitlement key, updater key). This is a required Phase 2
reference document. If the key separation table is missing, the Phase 2 DevOps
engineer will not know to generate a separate key pair.

**V3 — Placeholder acknowledgment.**
Review `src-tauri/tauri.conf.json`. The `plugins.updater.pubkey` field must contain
the string `PLACEHOLDER_UPDATER_PUBLIC_KEY_REPLACE_IN_PHASE2`. Do not replace it
with a real key during Phase 1 — the key generation is a Phase 2 DevOps task that
requires a production environment.

**V4 — End-to-end startup trace.**
Run `pnpm run tauri dev` and observe the startup log. Within 5 seconds of app load,
look for a log line containing "update check" (either "no update available" or "update
check failed (non-fatal)"). One of these two messages should appear — confirming the
background update check timer fired. If neither appears, the `useUpdater()` hook is
not mounted or the interval is not firing.

---

*End of Phase 1 · Sub-phase 06 · File 02*
