# Authentication Contracts

Source of truth for all IPC commands and frontend service contracts in the auth domain.
Backend: `src-tauri/src/commands/auth.rs`
Frontend service: `src/services/auth-service.ts`
Frontend hook: `src/hooks/use-session.ts`

## Session Model

A session has four possible states:

| State | Condition | App behavior |
|-------|-----------|-------------|
| `unauthenticated` | `is_authenticated: false`, `is_locked: false` | Show login screen |
| `authenticated` | `is_authenticated: true`, `is_locked: false` | Normal operation |
| `idle_locked` | `is_authenticated: false`, `is_locked: true` | Show quick unlock (PIN or password) |
| `force_change` | `is_authenticated: true`, `force_password_change: true` | Redirect to password change screen before any module |

Session duration: 8 hours of activity  
Idle lock timeout: 30 minutes of no IPC activity  
Lockout threshold: 10 consecutive failed login attempts → 15-minute cooldown

## IPC Commands

### login

```
Command:   login
Payload:   { username: string, password: string }
Response:  { session_info: SessionInfo }
Errors:    AUTH_ERROR — "Identifiant ou mot de passe invalide." (always opaque)
```

Security invariants:
- The error message is ALWAYS the same string regardless of the failure reason
  (user not found / wrong password / account locked / SSO-only account).
  This prevents user enumeration.
- The password is never stored in logs, traces, or audit events.
- The argon2id hash parameters are compile-time constants (m=64MiB, t=3, p=1).
- After 10 consecutive failures, `locked_until` is set; login returns the same
  opaque error — no countdown is exposed.

### logout

```
Command:   logout
Payload:   (none)
Response:  null
Errors:    (none — logout always succeeds, even if there is no active session)
```

### get_session_info

```
Command:   get_session_info
Payload:   (none)
Response:  SessionInfo
Errors:    (none — always returns a SessionInfo; is_authenticated = false if no session)
```

## Frontend Service Contracts

All auth IPC calls go through `src/services/auth-service.ts`. Components and hooks
NEVER call `invoke()` directly for auth operations (ADR-003 compliance).

All responses are validated through Zod schemas at the service layer. A response that
doesn't match the schema throws a ZodError before the application state is modified.

## Security Rules

1. Session token is not present in any IPC response, Zod schema, or React state.
2. `force_password_change: true` must redirect to the password-change screen immediately
   after login. No module is accessible until the flag is cleared.
3. The `require_session!` macro is the single enforcement point for authenticated access
   on the Rust side. IPC commands that need auth MUST use it.
4. `get_session_info` is the only command that is callable without authentication —
   everything else that modifies state must be behind `require_session!`.
