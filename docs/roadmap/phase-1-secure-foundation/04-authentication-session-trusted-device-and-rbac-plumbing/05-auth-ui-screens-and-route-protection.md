# Phase 1 · Sub-phase 04 · File 05
# Auth UI Screens and Route Protection

## Context and Purpose

Sub-phases 01–05 delivered the full engineering baseline: Tauri shell, local data
plane, authentication backend (session manager, password verification, RBAC,
device trust), and the multilingual foundation with 30 i18n namespaces.

However, a critical gap exists: the authentication **backend contracts** are fully
implemented (IPC commands `login`, `logout`, `get_session_info`, `verify_step_up`,
device trust management) but no **visual authentication surface** exists. The router
currently renders all 26 module pages unconditionally — there is no login page,
no idle lock screen, no force-password-change screen, and no route guard.

The i18n `auth.json` namespace (created in SP05-F02) already contains all required
translation keys for login, logout, session lock, force password change, step-up,
and device trust. The `useSession` hook (SP04-F01 S3) and `auth-service.ts` are
fully operational. This file connects those existing contracts to real UI screens
and a route protection layer.

Without this file, the Phase 1 preflight checklist (SP06-F04 S3) cannot pass:
- Item 2.2: "displays the login screen"
- Item 4.1: "can log in as admin"
- Item 5.1: "Login screen renders in French"
- Item 8.3: session idle-lock and force-change behaviors

This file **must be executed after SP05-F04 and before SP06-F01**.

## Architecture Rules Applied

- **Session-state driven routing.** The `AuthGuard` component reads session state
  from `useSession()` and renders the appropriate screen: login redirect,
  lock screen, force-password-change, or the authenticated shell. There is no
  token-based redirect — the Rust backend owns session state and the frontend
  queries it via IPC.
- **Bilingual from day one.** All screens use `useTranslation("auth")` with keys
  already defined in SP05-F02. No hardcoded strings.
- **Opaque error messages.** Login errors never reveal whether a username exists or
  whether the password was wrong. The backend returns a single error string;
  the frontend displays it as-is.
- **Auth screens outside the shell.** The login page, lock screen, and force-change
  screen render **outside** `ShellLayout` (no sidebar, no top bar). Only
  authenticated, non-locked, non-force-change users see the shell.
- **Minimal dependencies.** No external UI library is required for Phase 1 auth
  screens — they use standard HTML form elements styled with Tailwind. Shadcn/ui
  components are a Phase 2 deliverable.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/session_manager.rs` (patch) | `unlock_session()` method |
| `src-tauri/src/commands/auth.rs` (patch) | `unlock_session` and `force_change_password` IPC commands |
| `src-tauri/src/lib.rs` (patch) | Register two new auth commands |
| `shared/ipc-types.ts` (patch) | `UnlockRequest`, `ForceChangePasswordRequest`, `ForceChangePasswordResponse` |
| `src/services/auth-service.ts` (patch) | `unlockSession()`, `forceChangePassword()` IPC wrappers |
| `src/hooks/use-session.ts` (patch) | `unlock()`, `changePassword()` actions |
| `src/pages/auth/LoginPage.tsx` | Login form with bilingual support |
| `src/pages/auth/LockScreen.tsx` | Quick-unlock screen for idle-locked sessions |
| `src/pages/auth/ForcePasswordChangePage.tsx` | Mandatory password change screen |
| `src/components/auth/AuthGuard.tsx` | Session-state router wrapping authenticated routes |
| `src/router.tsx` (patch) | `/login` route outside shell, `AuthGuard` wrapping protected routes |
| `src/components/layout/TopBar.tsx` (patch) | User menu dropdown with logout |

## Prerequisites

- SP04-F01 complete: `auth-service.ts`, `use-session.ts`, `session_manager.rs`,
  `commands/auth.rs` (login, logout, get_session_info)
- SP04-F02 complete: device trust, offline access controls
- SP04-F03 complete: `require_session!`, `require_step_up!`, `verify_step_up` command
- SP05-F02 complete: `auth.json` namespace with login, logout, session, stepUp,
  device keys in both French and English

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Backend Auth Extensions: Unlock and Force Change Password | Rust: session_manager patch, 2 new IPC commands, lib.rs patch |
| S2 | Auth UI Screens: Login, Lock, Force Password Change | `LoginPage.tsx`, `LockScreen.tsx`, `ForcePasswordChangePage.tsx`, service/hook patches |
| S3 | AuthGuard, Router Protection, and TopBar User Menu | `AuthGuard.tsx`, `router.tsx` patch, `TopBar.tsx` patch |

---

## Sprint S1 — Backend Auth Extensions: Unlock and Force Change Password

### AI Agent Prompt

```
You are a senior Rust engineer working on the Maintafox desktop app. The authentication
system (SP04-F01 through F04) is complete: SessionManager, login/logout/get_session_info
IPC commands, device trust, and RBAC step-up are all implemented. However, two IPC
commands are missing that the frontend auth screens require:

1. `unlock_session` — verify password and unlock an idle-locked session
2. `force_change_password` — hash a new password, update the database, and clear the
   `force_password_change` flag

────────────────────────────────────────────────────────────────────
STEP 1 — PATCH src-tauri/src/auth/session_manager.rs — add unlock method
────────────────────────────────────────────────────────────────────

Add the following method to the `impl SessionManager` block, after the existing
`lock_session()` method:

```rust
    /// Unlock an idle-locked session. Returns `true` if the session was
    /// successfully unlocked, `false` if the session is expired or absent.
    /// The caller MUST verify the user's password before calling this.
    pub fn unlock_session(&mut self) -> bool {
        match &mut self.current {
            Some(session) if !session.is_expired() => {
                session.is_locked = false;
                session.last_activity_at = Utc::now();
                true
            }
            _ => false,
        }
    }
```

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src-tauri/src/commands/auth.rs — add two new IPC commands
────────────────────────────────────────────────────────────────────

Add the following at the end of the file, after the existing `revoke_device_trust`
command. These commands use imports already present in the file (`password`,
`session_manager`, `AppError`, `AppResult`, `AppState`, `Deserialize`, `Serialize`).

```rust
// ── Unlock Session ────────────────────────────────────────────────────────────

/// Input for the unlock_session command.
#[derive(Debug, Deserialize)]
pub struct UnlockSessionRequest {
    pub password: String,
}

/// Unlock an idle-locked session by verifying the user's password.
///
/// The session must exist and not be expired. If the session has expired,
/// the user is told to log in again. If the password is wrong, an opaque
/// auth error is returned.
#[tauri::command]
pub async fn unlock_session(
    payload: UnlockSessionRequest,
    state: State<'_, AppState>,
) -> AppResult<session_manager::SessionInfo> {
    // Read session to get the user — even locked sessions have a user
    let user = {
        let sm = state.session.read().await;
        match &sm.current {
            Some(s) if !s.is_expired() => s.user.clone(),
            _ => {
                return Err(AppError::Auth(
                    "Session expirée. Veuillez vous reconnecter.".into(),
                ));
            }
        }
    };

    // Verify password
    let user_record =
        session_manager::find_active_user(&state.db, &user.username).await?;

    let pw_hash = match user_record.and_then(|r| r.5) {
        Some(h) => h,
        None => {
            return Err(AppError::Auth(
                "Mot de passe incorrect.".into(),
            ));
        }
    };

    let valid = password::verify_password(&payload.password, &pw_hash)?;
    if !valid {
        warn!(username = %user.username, "unlock_session::wrong_password");
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::STEP_UP_FAILURE,
                actor_id: Some(user.user_id),
                summary: "Unlock failed: wrong password",
                ..Default::default()
            },
        )
        .await;
        return Err(AppError::Auth(
            "Mot de passe incorrect.".into(),
        ));
    }

    // Unlock the session
    let mut sm = state.session.write().await;
    if !sm.unlock_session() {
        return Err(AppError::Auth(
            "Session expirée. Veuillez vous reconnecter.".into(),
        ));
    }

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::LOGIN_SUCCESS,
            actor_id: Some(user.user_id),
            summary: "Session unlocked after idle lock",
            ..Default::default()
        },
    )
    .await;

    let info = sm.session_info();
    Ok(info)
}

// ── Force Change Password ─────────────────────────────────────────────────────

/// Input for the force_change_password command.
#[derive(Debug, Deserialize)]
pub struct ForceChangePasswordRequest {
    pub new_password: String,
}

/// Response returned after a successful password change.
#[derive(Debug, Serialize)]
pub struct ForceChangePasswordResponse {
    pub session_info: session_manager::SessionInfo,
}

/// Change the password for a user who has `force_password_change = true`.
///
/// This command is only callable when the current session has
/// `force_password_change` set. It hashes the new password with argon2id,
/// updates the database, clears the flag, and returns the updated session.
#[tauri::command]
pub async fn force_change_password(
    payload: ForceChangePasswordRequest,
    state: State<'_, AppState>,
) -> AppResult<ForceChangePasswordResponse> {
    // Read current session — must be authenticated with force_password_change
    let user = {
        let sm = state.session.read().await;
        match &sm.current {
            Some(s) if !s.is_expired() && s.user.force_password_change => {
                s.user.clone()
            }
            Some(_) => {
                return Err(AppError::Auth(
                    "Le changement de mot de passe n'est pas requis.".into(),
                ));
            }
            None => {
                return Err(AppError::Auth(
                    "Non authentifié.".into(),
                ));
            }
        }
    };

    // Validate password strength (minimum 8 characters)
    let new_password = payload.new_password.trim();
    if new_password.len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "Le mot de passe doit contenir au moins 8 caractères.".into(),
        ]));
    }

    // Hash the new password with argon2id
    let new_hash = password::hash_password(new_password)?;

    // Update user_accounts in the database
    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"UPDATE user_accounts
               SET password_hash = ?,
                   force_password_change = 0,
                   updated_at = ?
               WHERE id = ?"#,
            [
                new_hash.into(),
                now.clone().into(),
                user.user_id.into(),
            ],
        ))
        .await?;

    // Update the in-memory session
    let mut sm = state.session.write().await;
    if let Some(session) = &mut sm.current {
        session.user.force_password_change = false;
    }

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::FORCE_CHANGE_SET,
            actor_id: Some(user.user_id),
            summary: "Password changed via force-change flow",
            ..Default::default()
        },
    )
    .await;

    tracing::info!(
        user_id = user.user_id,
        "force_change_password completed"
    );

    let info = sm.session_info();
    Ok(ForceChangePasswordResponse { session_info: info })
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — PATCH src-tauri/src/lib.rs — register new commands
────────────────────────────────────────────────────────────────────

In the `tauri::generate_handler![]` macro call, add the two new commands after
the existing auth commands:

```rust
.invoke_handler(tauri::generate_handler![
    commands::health_check,
    commands::app::get_app_info,
    commands::app::get_task_status,
    commands::app::shutdown_app,
    commands::auth::login,
    commands::auth::logout,
    commands::auth::get_session_info,
    commands::auth::get_device_trust_status,
    commands::auth::revoke_device_trust,
    // SP04-F05 — auth UI support
    commands::auth::unlock_session,
    commands::auth::force_change_password,
    commands::lookup::list_lookup_domains,
    commands::lookup::get_lookup_values,
    commands::lookup::get_lookup_value_by_id,
    commands::diagnostics::run_integrity_check,
    commands::diagnostics::repair_seed_data,
    commands::rbac::get_my_permissions,
    commands::rbac::verify_step_up,
    commands::locale::get_locale_preference,
    commands::locale::set_locale_preference,
])
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` inside src-tauri passes with 0 errors
- `cargo test` passes — existing session_manager tests still pass
- The new `unlock_session()` method on SessionManager returns `true`
  when called on a locked, non-expired session and `false` on an expired one
- The `force_change_password` command rejects passwords shorter than 8 characters
  with a `ValidationFailed` error
- The `force_change_password` command updates `user_accounts.password_hash` in the
  database and sets `force_password_change = 0`
```

---

### Supervisor Verification — Sprint S1

**V1 — Cargo builds cleanly.**
Run `cargo check` inside the `src-tauri` directory. Zero errors means the new
commands compile and integrate with existing auth imports.

**V2 — Existing tests still pass.**
Run `cargo test` inside `src-tauri`. All existing session_manager and auth tests
must pass. The new `unlock_session` method should not break any existing behavior.

**V3 — IPC commands are registered.**
Open `src-tauri/src/lib.rs` and search for `unlock_session` and `force_change_password`
in the `generate_handler!` block. Both must be present. If missing, the IPC calls
from the frontend will fail with "command not found".

---

## Sprint S2 — Auth UI Screens: Login, Lock, Force Password Change

### AI Agent Prompt

```
You are a TypeScript and React engineer. The Rust backend now has `login`, `logout`,
`get_session_info`, `unlock_session`, and `force_change_password` IPC commands. The
i18n `auth.json` namespace has all required keys in both French and English.

Your task is to:
1. Add IPC types and service wrappers for the two new commands
2. Extend the useSession hook with unlock and changePassword actions
3. Create the three auth UI screens: LoginPage, LockScreen, ForcePasswordChangePage

────────────────────────────────────────────────────────────────────
STEP 1 — PATCH shared/ipc-types.ts — add auth UI types
────────────────────────────────────────────────────────────────────

Add the following types at the end of the "Authentication & Session" section:

```typescript
// ─── Auth UI Commands ──────────────────────────────────────────────────────

export interface UnlockSessionRequest {
  password: string;
}

export interface ForceChangePasswordRequest {
  new_password: string;
}

export interface ForceChangePasswordResponse {
  session_info: SessionInfo;
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src/services/auth-service.ts — add IPC wrappers
────────────────────────────────────────────────────────────────────

Add the following functions after the existing `getSessionInfo()`:

```typescript
/**
 * Unlock an idle-locked session by verifying the user's password.
 * Returns the updated session info on success.
 */
export async function unlockSession(password: string): Promise<SessionInfo> {
  const raw = await invoke<unknown>("unlock_session", {
    payload: { password },
  });
  return sessionInfoSchema.parse(raw);
}

/**
 * Change the password for a user with force_password_change = true.
 * Returns the updated session info on success.
 */
export async function forceChangePassword(
  newPassword: string
): Promise<SessionInfo> {
  const raw = await invoke<unknown>("force_change_password", {
    payload: { new_password: newPassword },
  });
  // The response wraps session_info
  const parsed = z
    .object({ session_info: sessionInfoSchema })
    .parse(raw);
  return parsed.session_info;
}
```

Note: `sessionInfoSchema` and `z` are already imported in this file.

────────────────────────────────────────────────────────────────────
STEP 3 — PATCH src/hooks/use-session.ts — add unlock and changePassword
────────────────────────────────────────────────────────────────────

Update the import from auth-service to include the new functions:

```typescript
import {
  getSessionInfo,
  login as authLogin,
  logout as authLogout,
  unlockSession as authUnlock,
  forceChangePassword as authForceChange,
} from "@/services/auth-service";
```

Add `unlock` and `changePassword` to the SessionActions interface:

```typescript
interface SessionActions {
  login: (req: LoginRequest) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
  unlock: (password: string) => Promise<void>;
  changePassword: (newPassword: string) => Promise<void>;
}
```

Add the implementations inside useSession(), after the existing `logoutAction`:

```typescript
  const unlock = useCallback(async (password: string) => {
    setState((s) => ({ ...s, isLoading: true, error: null }));
    try {
      const info = await authUnlock(password);
      setState({ info, isLoading: false, error: null });
    } catch (e) {
      setState((s) => ({
        ...s,
        isLoading: false,
        error: e instanceof Error ? e.message : "Échec du déverrouillage.",
      }));
      throw e;
    }
  }, []);

  const changePassword = useCallback(async (newPassword: string) => {
    setState((s) => ({ ...s, isLoading: true, error: null }));
    try {
      const info = await authForceChange(newPassword);
      setState({ info, isLoading: false, error: null });
    } catch (e) {
      setState((s) => ({
        ...s,
        isLoading: false,
        error:
          e instanceof Error
            ? e.message
            : "Échec du changement de mot de passe.",
      }));
      throw e;
    }
  }, []);
```

Update the return statement:

```typescript
  return {
    ...state,
    login,
    logout: logoutAction,
    refresh,
    unlock,
    changePassword,
  };
```

────────────────────────────────────────────────────────────────────
STEP 4 — CREATE src/pages/auth/LoginPage.tsx
────────────────────────────────────────────────────────────────────

```tsx
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { useSession } from "@/hooks/use-session";
import { useLocaleStore } from "@/stores/locale-store";
import { cn } from "@/lib/utils";

export function LoginPage() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const session = useSession();
  const { activeLocale, setLocale, supportedLocales } = useLocaleStore();

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await session.login({ username: username.trim(), password });
      navigate("/", { replace: true });
    } catch {
      // Error is captured in session.error by the hook
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm">
        {/* Brand header */}
        <div className="mb-8 text-center">
          <h1 className="text-2xl font-bold text-primary">Maintafox</h1>
          <p className="mt-1 text-sm text-text-secondary">
            {t("login.subtitle")}
          </p>
        </div>

        {/* Login form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          {/* Username */}
          <div>
            <label
              htmlFor="login-username"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("login.form.username.label")}
            </label>
            <input
              id="login-username"
              type="text"
              autoComplete="username"
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("login.form.username.placeholder")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={session.isLoading}
            />
          </div>

          {/* Password */}
          <div>
            <label
              htmlFor="login-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("login.form.password.label")}
            </label>
            <input
              id="login-password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("login.form.password.placeholder")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={session.isLoading}
            />
          </div>

          {/* Error display */}
          {session.error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm
                         text-status-danger"
            >
              {session.error}
            </div>
          )}

          {/* Submit */}
          <button
            type="submit"
            disabled={session.isLoading}
            className="btn-primary w-full py-2 text-sm font-medium"
          >
            {session.isLoading
              ? t("login.form.submitting")
              : t("login.form.submit")}
          </button>
        </form>

        {/* Locale switcher */}
        <div className="mt-6 flex justify-center gap-2">
          {supportedLocales.map((loc) => (
            <button
              key={loc}
              onClick={() => void setLocale(loc)}
              className={cn(
                "rounded px-3 py-1 text-xs font-medium transition-colors",
                loc === activeLocale
                  ? "bg-primary text-white"
                  : "text-text-secondary hover:bg-surface-2",
              )}
            >
              {loc.toUpperCase()}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 5 — CREATE src/pages/auth/LockScreen.tsx
────────────────────────────────────────────────────────────────────

```tsx
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";

interface LockScreenProps {
  displayName: string | null;
  onUnlock: (password: string) => Promise<void>;
  onLogout: () => void;
}

export function LockScreen({ displayName, onUnlock, onLogout }: LockScreenProps) {
  const { t } = useTranslation("auth");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await onUnlock(password);
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t("session.idleLocked.unlockAction"),
      );
      setPassword("");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm text-center">
        {/* User avatar */}
        <div
          className="mx-auto mb-4 flex h-16 w-16 items-center justify-center
                     rounded-full bg-primary text-2xl font-bold text-white"
        >
          {displayName ? displayName.charAt(0).toUpperCase() : "?"}
        </div>

        <h2 className="text-lg font-semibold text-text-primary">
          {t("session.idleLocked.title")}
        </h2>
        <p className="mt-1 text-sm text-text-secondary">
          {t("session.idleLocked.message")}
        </p>

        {displayName && (
          <p className="mt-2 text-sm font-medium text-text-primary">
            {displayName}
          </p>
        )}

        <form onSubmit={handleSubmit} className="mt-6 space-y-4">
          <div>
            <label htmlFor="lock-password" className="sr-only">
              {t("session.idleLocked.unlockPrompt")}
            </label>
            <input
              id="lock-password"
              type="password"
              autoComplete="current-password"
              autoFocus
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("session.idleLocked.unlockPrompt")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary text-center
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={loading}
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm
                         text-status-danger"
            >
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={loading}
            className="btn-primary w-full py-2 text-sm font-medium"
          >
            {loading
              ? t("session.idleLocked.unlocking")
              : t("session.idleLocked.unlockAction")}
          </button>
        </form>

        {/* Sign out link */}
        <button
          onClick={onLogout}
          className="mt-4 text-xs text-text-muted hover:text-text-secondary
                     transition-colors"
        >
          {t("logout.label")}
        </button>
      </div>
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
STEP 6 — CREATE src/pages/auth/ForcePasswordChangePage.tsx
────────────────────────────────────────────────────────────────────

```tsx
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

interface ForcePasswordChangePageProps {
  onComplete: (newPassword: string) => Promise<void>;
}

export function ForcePasswordChangePage({
  onComplete,
}: ForcePasswordChangePageProps) {
  const { t } = useTranslation("auth");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);

    if (newPassword.length < 8) {
      setError(t("session.forcePasswordChange.newPassword") + " — min. 8");
      return;
    }

    if (newPassword !== confirmPassword) {
      setError(t("session.forcePasswordChange.confirmPassword"));
      return;
    }

    setLoading(true);
    try {
      await onComplete(newPassword);
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t("login.error.unknown"),
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm">
        <div className="mb-6 text-center">
          <h1 className="text-2xl font-bold text-primary">Maintafox</h1>
        </div>

        <div className="mb-6">
          <h2 className="text-lg font-semibold text-text-primary">
            {t("session.forcePasswordChange.title")}
          </h2>
          <p className="mt-1 text-sm text-text-secondary">
            {t("session.forcePasswordChange.message")}
          </p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label
              htmlFor="new-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("session.forcePasswordChange.newPassword")}
            </label>
            <input
              id="new-password"
              type="password"
              autoComplete="new-password"
              autoFocus
              required
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={loading}
            />
          </div>

          <div>
            <label
              htmlFor="confirm-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("session.forcePasswordChange.confirmPassword")}
            </label>
            <input
              id="confirm-password"
              type="password"
              autoComplete="new-password"
              required
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={loading}
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm
                         text-status-danger"
            >
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={loading}
            className="btn-primary w-full py-2 text-sm font-medium"
          >
            {loading
              ? t("login.form.submitting")
              : t("session.forcePasswordChange.submit")}
          </button>
        </form>
      </div>
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- `LoginPage` renders with username/password fields and a submit button
- All visible strings come from `useTranslation("auth")` — no hardcoded user-facing text
- `LockScreen` displays the user's initial in an avatar circle and a password input
- `ForcePasswordChangePage` has two password fields and validates min-length and match
- Locale switcher buttons on LoginPage change the displayed language
```

---

### Supervisor Verification — Sprint S2

**V1 — TypeScript compiles.**
Run `pnpm run typecheck`. Zero errors confirms that the new IPC types, service
wrappers, and React components are all type-consistent.

**V2 — LoginPage uses auth namespace.**
Open `src/pages/auth/LoginPage.tsx` and search for `useTranslation("auth")`.
Confirm that all visible labels use `t("login.form.username.label")`,
`t("login.form.password.label")`, etc. No French strings should appear in the TSX.

**V3 — Lock screen shows display name.**
Open `src/pages/auth/LockScreen.tsx`. Confirm that the `displayName` prop is
displayed and that the avatar circle shows the first character of the name.

**V4 — Password validation in force-change page.**
Open `src/pages/auth/ForcePasswordChangePage.tsx`. Confirm that:
1. Passwords shorter than 8 characters show an error
2. Non-matching passwords show an error
3. The submit button calls `onComplete(newPassword)` only when valid

---

## Sprint S3 — AuthGuard, Router Protection, and TopBar User Menu

### AI Agent Prompt

```
You are a React and TypeScript engineer. The three auth screens (Login, Lock,
ForcePasswordChange) and the backend IPC commands are complete. Your task is to:

1. Create the AuthGuard component that routes session states to the right screen
2. Update router.tsx to use AuthGuard for route protection
3. Add a user menu dropdown with logout to TopBar

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src/components/auth/AuthGuard.tsx
────────────────────────────────────────────────────────────────────

```tsx
import { useCallback } from "react";
import { Navigate, Outlet } from "react-router-dom";

import { useSession } from "@/hooks/use-session";
import { logout as authLogout } from "@/services/auth-service";
import { ForcePasswordChangePage } from "@/pages/auth/ForcePasswordChangePage";
import { LockScreen } from "@/pages/auth/LockScreen";

/**
 * AuthGuard: session-state router.
 *
 * Sits between the router root and the ShellLayout. Renders one of:
 * 1. Loading spinner — while session state is being fetched
 * 2. Navigate to /login — if not authenticated and not locked
 * 3. LockScreen — if session is idle-locked (has user but locked)
 * 4. ForcePasswordChangePage — if authenticated but must change password
 * 5. <Outlet /> — normal authenticated state → ShellLayout renders
 *
 * Each sub-screen receives callbacks that trigger a session refresh,
 * causing AuthGuard to re-evaluate and potentially show a different screen.
 */
export function AuthGuard() {
  const session = useSession();

  const handleUnlock = useCallback(
    async (password: string) => {
      await session.unlock(password);
    },
    [session],
  );

  const handleForceChange = useCallback(
    async (newPassword: string) => {
      await session.changePassword(newPassword);
    },
    [session],
  );

  const handleLogout = useCallback(async () => {
    await authLogout();
    // After logout, session.info becomes UNAUTHENTICATED on next render
    await session.refresh();
  }, [session]);

  // 1. Loading
  if (session.isLoading) {
    return (
      <div className="flex h-screen items-center justify-center bg-surface-0">
        <div
          className="h-8 w-8 animate-spin rounded-full border-2
                     border-surface-3 border-t-primary"
        />
      </div>
    );
  }

  const info = session.info;

  // 2. Locked session — show lock screen (before auth check because
  //    is_authenticated is false when locked)
  if (info?.is_locked && info.user_id !== null) {
    return (
      <LockScreen
        displayName={info.display_name ?? info.username}
        onUnlock={handleUnlock}
        onLogout={handleLogout}
      />
    );
  }

  // 3. Not authenticated — redirect to login
  if (!info?.is_authenticated) {
    return <Navigate to="/login" replace />;
  }

  // 4. Force password change required
  if (info.force_password_change) {
    return <ForcePasswordChangePage onComplete={handleForceChange} />;
  }

  // 5. Normal authenticated state
  return <Outlet />;
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — PATCH src/router.tsx — add login route and AuthGuard
────────────────────────────────────────────────────────────────────

Replace the entire `routes` array and add the necessary imports. The key changes:
- Import `LoginPage` (lazy-loaded)
- Import `AuthGuard`
- `/login` route sits outside the shell (no sidebar, no top bar)
- All existing routes are wrapped inside `AuthGuard > ShellLayout`

Add to the lazy imports section:

```typescript
const LoginPage = lazy(() =>
  import("@/pages/auth/LoginPage").then((m) => ({ default: m.LoginPage })),
);
```

Add the static import for AuthGuard (not lazy — it's the route boundary):

```typescript
import { AuthGuard } from "@/components/auth/AuthGuard";
```

Replace the `routes` array:

```typescript
const routes: RouteObject[] = [
  // ── Public routes (no shell, no auth required) ───────────────────────
  {
    path: "login",
    element: (
      <Suspense
        fallback={
          <div className="flex h-screen items-center justify-center bg-surface-0">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
          </div>
        }
      >
        <LoginPage />
      </Suspense>
    ),
  },
  // ── Protected routes (auth required → shell layout) ──────────────────
  {
    element: <AuthGuard />,
    children: [
      {
        element: <ShellLayout />,
        children: [
          { index: true, element: <DashboardPage /> },
          {
            element: <PageSuspense />,
            children: [
              { path: "equipment", element: <EquipmentPage /> },
              { path: "requests", element: <RequestsPage /> },
              { path: "work-orders", element: <WorkOrdersPage /> },
              { path: "planning", element: <PlanningPage /> },
              { path: "pm", element: <PmPage /> },
              { path: "permits", element: <PermitsPage /> },
              { path: "inspections", element: <InspectionsPage /> },
              { path: "training", element: <TrainingPage /> },
              { path: "inventory", element: <InventoryPage /> },
              { path: "analytics", element: <AnalyticsPage /> },
              { path: "reliability", element: <ReliabilityPage /> },
              { path: "budget", element: <BudgetPage /> },
              { path: "personnel", element: <PersonnelPage /> },
              { path: "users", element: <UsersPage /> },
              { path: "org", element: <OrgPage /> },
              { path: "lookups", element: <LookupsPage /> },
              { path: "notifications", element: <NotificationsPage /> },
              { path: "documentation", element: <DocumentationPage /> },
              { path: "iot", element: <IotPage /> },
              { path: "erp", element: <ErpPage /> },
              { path: "archive", element: <ArchivePage /> },
              { path: "activity", element: <ActivityPage /> },
              { path: "settings", element: <SettingsPage /> },
              { path: "configuration", element: <ConfigurationPage /> },
              { path: "diagnostics", element: <DiagnosticsPage /> },
              { path: "profile", element: <ProfilePage /> },
            ],
          },
        ],
      },
    ],
  },
];
```

────────────────────────────────────────────────────────────────────
STEP 3 — PATCH src/components/layout/TopBar.tsx — user menu dropdown
────────────────────────────────────────────────────────────────────

Add a user menu dropdown to TopBar so authenticated users can log out.
The dropdown uses local state and a ref for click-outside handling.

Add these imports at the top of TopBar.tsx:

```typescript
import { useState, useRef, useEffect, useCallback } from "react";
import { useNavigate, Link } from "react-router-dom";
import { Menu, Bell, RefreshCw, AlertCircle, User, LogOut, Settings, UserCircle } from "lucide-react";
import { logout as authLogout } from "@/services/auth-service";
```

Replace the existing "User menu trigger" button section with:

```tsx
        {/* User menu */}
        <div ref={userMenuRef} className="relative">
          <button
            onClick={() => setUserMenuOpen((v) => !v)}
            aria-label={displayName ?? t("user.menu")}
            aria-expanded={userMenuOpen}
            aria-haspopup="true"
            className="btn-ghost flex items-center gap-2 px-2 py-1.5"
          >
            <div
              className="flex h-6 w-6 items-center justify-center
                         rounded-full bg-primary text-xs font-semibold text-white"
            >
              {displayName ? displayName.charAt(0).toUpperCase() : <User className="h-3.5 w-3.5" />}
            </div>
            {displayName && (
              <span className="hidden lg:inline text-sm text-text-secondary max-w-32 truncate">
                {displayName}
              </span>
            )}
          </button>

          {/* Dropdown */}
          {userMenuOpen && (
            <div
              className="absolute right-0 top-full mt-1 w-48 rounded-md border
                         border-surface-border bg-surface-1 py-1 shadow-lg z-50"
              role="menu"
            >
              {displayName && (
                <div className="px-3 py-2 text-sm font-medium text-text-primary
                                border-b border-surface-border truncate">
                  {displayName}
                </div>
              )}
              <Link
                to="/profile"
                onClick={() => setUserMenuOpen(false)}
                className="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary
                           hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <UserCircle className="h-4 w-4" />
                {t("user.profile")}
              </Link>
              <Link
                to="/settings"
                onClick={() => setUserMenuOpen(false)}
                className="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary
                           hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <Settings className="h-4 w-4" />
                {t("user.settings")}
              </Link>
              <div className="border-t border-surface-border my-1" />
              <button
                onClick={handleLogout}
                className="flex w-full items-center gap-2 px-3 py-2 text-sm
                           text-status-danger hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <LogOut className="h-4 w-4" />
                {t("user.logout")}
              </button>
            </div>
          )}
        </div>
```

Add to the TopBar component body (before the return statement):

```typescript
  const navigate = useNavigate();
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const userMenuRef = useRef<HTMLDivElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (
        userMenuRef.current &&
        !userMenuRef.current.contains(e.target as Node)
      ) {
        setUserMenuOpen(false);
      }
    }
    if (userMenuOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [userMenuOpen]);

  const handleLogout = useCallback(async () => {
    setUserMenuOpen(false);
    try {
      await authLogout();
    } finally {
      navigate("/login", { replace: true });
    }
  }, [navigate]);
```

Also add translation keys for the user menu to both `src/i18n/fr/shell.json` and
`src/i18n/en/shell.json` if they are not already present:

For `fr/shell.json` — add inside the `"user"` key:
```json
"user": {
  "menu": "Menu utilisateur",
  "profile": "Mon profil",
  "settings": "Paramètres",
  "logout": "Déconnexion"
}
```

For `en/shell.json` — add inside the `"user"` key:
```json
"user": {
  "menu": "User menu",
  "profile": "My profile",
  "settings": "Settings",
  "logout": "Sign out"
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- `pnpm run i18n:check` exits 0 (no i18n key regressions)
- Opening the app shows the login screen (not the dashboard)
- Logging in with admin credentials navigates to the dashboard
- If `force_password_change` is `true`, the force-change screen appears after login
- After idle timeout, the lock screen appears with the user's avatar and name
- Clicking the user avatar in TopBar shows a dropdown with Profile, Settings, Sign out
- Clicking Sign out navigates to the login screen
- The login screen renders correctly in both French and English via locale switcher
```

---

### Supervisor Verification — Sprint S3

**V1 — Login flow works end-to-end.**
Run `pnpm run tauri dev`. The app should show the login screen. Enter the admin
credentials (`admin` / `Admin#2026!`). The dashboard should appear with the sidebar
and top bar. If the login screen does not appear, check that `AuthGuard` is in the
router tree and that the `/login` route is defined.

**V2 — Route protection is enforced.**
Open a new browser tab and navigate to `http://localhost:1420/equipment`. The page
should redirect to `/login` because there is no active session. If the equipment
page renders, the `AuthGuard` is not wrapping the protected routes correctly.

**V3 — User menu works.**
After logging in, click the user avatar in the top bar. A dropdown should appear with
"Mon profil", "Paramètres", and "Déconnexion" (in French). Click "Déconnexion" — the
app should navigate to the login screen.

**V4 — Lock screen appears on idle.**
After logging in, wait 30 minutes (or temporarily set `IDLE_LOCK_MINUTES` to 1 for
testing). The lock screen should appear with the user's avatar initial and a password
input. Enter the password to unlock. If the lock screen does not appear, check that
`session_info().is_locked` returns `true` when idle timeout is exceeded.

**V5 — Force password change flow.**
In the database, set `force_password_change = 1` for the admin user:
```sql
UPDATE user_accounts SET force_password_change = 1 WHERE username = 'admin';
```
Log in. The force-change screen should appear instead of the dashboard. Enter a new
password (minimum 8 characters). After success, the dashboard should appear. Verify in
the database that `force_password_change` is now `0` and `password_hash` has changed.

**V6 — Bilingual login screen.**
On the login page, click the "EN" button. All labels should switch to English. Click
"FR" — they should switch back to French. This confirms the locale store and i18n
system work correctly on the auth screens.

---

*End of Phase 1 · Sub-phase 04 · File 05*
