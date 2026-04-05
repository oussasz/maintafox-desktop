# SP04 Completion Checklist

**Sub-phase:** 04 — Authentication, Session, Trusted Device, and RBAC Plumbing
**Phase:** 1 — Secure Foundation
**Gate:** This checklist must be reviewed before opening a pull request to merge SP04.
**Author:** Development team lead
**Date:** ______________

---

## How to Use This Checklist

Go through each item in order. Mark ✅ if verified, ❌ if not satisfied, or N/A if
definitively inapplicable for your build environment. The PR cannot be merged if any
item is marked ❌.

---

## 1. Database Schema

| # | Check | Status |
|---|-------|--------|
| 1.1 | `user_accounts` table exists with all columns from SP04-F01 sprint S1 | |
| 1.2 | `roles` table has is_system column and 4+ system rows after seed | |
| 1.3 | `permissions` table has ≥ 56 rows all with is_system = 1 | |
| 1.4 | `role_permissions` has Administrator → all permissions mapping | |
| 1.5 | `trusted_devices` table has device_fingerprint UNIQUE constraint | |
| 1.6 | `audit_events` table is writable (can insert test row) | |
| 1.7 | `user_scope_assignments` table exists with scope_type column | |

Run in DBeaver to auto-check 1.2–1.4:
```sql
SELECT
  (SELECT COUNT(*) FROM user_accounts)            AS users,
  (SELECT COUNT(*) FROM roles WHERE is_system=1)  AS system_roles,
  (SELECT COUNT(*) FROM permissions WHERE is_system=1) AS permissions;
-- Expected: users >= 1, system_roles = 4, permissions >= 56
```

---

## 2. Rust Compilation and Tests

| # | Check | Status |
|---|-------|--------|
| 2.1 | `cargo build --release` compiles with 0 errors | |
| 2.2 | `cargo test` passes all tests in all modules | |
| 2.3 | `cargo test --test auth_integration_tests` — all 8 tests pass | |
| 2.4 | `cargo test --test security_invariant_tests` — all 6 tests pass | |
| 2.5 | No compiler warnings in auth/, audit/, commands/auth.rs, commands/rbac.rs | |

---

## 3. Authentication IPC

| # | Check | Status |
|---|-------|--------|
| 3.1 | `login` with admin/Admin#2026! succeeds and returns SessionInfo | |
| 3.2 | `login` with wrong password returns the same opaque error string as non-existent user | |
| 3.3 | `logout` clears the session and subsequent `get_session_info` returns unauthenticated | |
| 3.4 | `get_session_info` returns unauthenticated when no session is active | |
| 3.5 | Admin account has `force_password_change = 1` in the database after seed | |

Manual test for 3.2 — run both and confirm error messages are identical:
```javascript
// Both must produce the SAME error string:
invoke('login', { payload: { username: 'nonexistent_user_xyz', password: 'any' } }).catch(e => console.log('ERR1:', e));
invoke('login', { payload: { username: 'admin', password: 'wrong_password' } }).catch(e => console.log('ERR2:', e));
```

---

## 4. Device Trust

| # | Check | Status |
|---|-------|--------|
| 4.1 | First login creates a row in `trusted_devices` | |
| 4.2 | Device fingerprint is exactly 64 hex characters | |
| 4.3 | Second login updates `last_seen_at` without creating a duplicate row | |
| 4.4 | `get_device_trust_status` returns `is_trusted: true` after login | |
| 4.5 | Device secret is present in OS keyring after first launch | |
| 4.6 | `MAX_OFFLINE_GRACE_HOURS = 168` (verified by sec5 test) | |

Manual test for 4.3:
```sql
SELECT COUNT(*) FROM trusted_devices; -- Must be exactly 1 after 3+ logins
```

---

## 5. RBAC and Permissions

| # | Check | Status |
|---|-------|--------|
| 5.1 | `get_my_permissions` returns ≥ 56 records for user with Administrator scope assignment | |
| 5.2 | `verify_step_up` with correct password returns `{ success: true }` | |
| 5.3 | `verify_step_up` with wrong password returns an error (not `{ success: false }`) | |
| 5.4 | `require_permission!` macro denies a command for a user without scope assignment | |
| 5.5 | `require_step_up!` macro returns `StepUpRequired` when no step-up has been verified | |
| 5.6 | `STEP_UP_DURATION_SECS = 120` (verified by sec4 test) | |
| 5.7 | Operator role does NOT have `adm.users` or `adm.roles` permissions | |

SQL check for 5.7:
```sql
SELECT p.name FROM permissions p
INNER JOIN role_permissions rp ON rp.permission_id = p.id
INNER JOIN roles r ON rp.role_id = r.id
WHERE r.name = 'Operator' AND p.name IN ('adm.users', 'adm.roles', 'adm.permissions');
-- Must return 0 rows
```

---

## 6. Audit Events

| # | Check | Status |
|---|-------|--------|
| 6.1 | Login success writes a row with event_type = 'login.success' | |
| 6.2 | Login failure writes a row with event_type = 'login.failure' | |
| 6.3 | Logout writes a row with event_type = 'logout' | |
| 6.4 | Step-up success writes a row with event_type = 'step_up.success' | |
| 6.5 | Audit emit failure does NOT block the operation (UI responds normally even if DB is read-only) | |
| 6.6 | `audit_events` table has no password_hash, token, or keyring material in any field | |

SQL check for 6.6:
```sql
SELECT * FROM audit_events WHERE
  summary LIKE '%password_hash%' OR
  detail_json LIKE '%$argon2id$%' OR
  detail_json LIKE '%token%';
-- Must return 0 rows
```

---

## 7. Security Properties

| # | Check | Status |
|---|-------|--------|
| 7.1 | `MEMORY_COST_KIB ≥ 65536` (argon2id at ≥ 64 MiB) | |
| 7.2 | `TIME_COST ≥ 3` (argon2id iterations) | |
| 7.3 | Session token appears in ZERO IPC response fields (verified by sec2 test) | |
| 7.4 | The string `"force_password_change": true` appears in the first `get_session_info` for admin | |
| 7.5 | No raw SQL string concatenation in any auth/rbac/audit source file (parameterized queries only) | |
| 7.6 | No `unwrap()` calls in any production code path in auth/ or commands/ | |

Manual check for 7.5 — run this search and confirm 0 matches:
```
grep -rn "format!.*WHERE.*{" src-tauri/src/auth/ src-tauri/src/commands/
```
The only format! calls allowed are for building parameterized query strings where
binding values are always passed separately through the values array.

---

## 8. TypeScript / Frontend

| # | Check | Status |
|---|-------|--------|
| 8.1 | `pnpm test` passes with all hook tests green | |
| 8.2 | `use-session.ts` tests — 4 tests pass | |
| 8.3 | `use-permissions.ts` tests — 4 tests pass | |
| 8.4 | `shared/ipc-types.ts` includes SessionInfo, LoginRequest, LoginResponse, PermissionRecord, StepUpRequest, StepUpResponse, DeviceTrustStatus, TrustedDevice | |
| 8.5 | `PermissionGate` renders fallback when permission is not held | |

---

## 9. Documentation

| # | Check | Status |
|---|-------|--------|
| 9.1 | `docs/AUTH_CONTRACTS.md` exists and documents all 4 session states | |
| 9.2 | `docs/DEVICE_TRUST_CONTRACTS.md` exists and documents the 168-hour cap | |
| 9.3 | `docs/RBAC_CONTRACTS.md` exists and lists all permission domain prefixes | |
| 9.4 | `docs/IPC_COMMAND_REGISTRY.md` updated with: login, logout, get_session_info, get_device_trust_status, revoke_device_trust, get_my_permissions, verify_step_up | |
| 9.5 | This checklist document exists at `docs/SP04_COMPLETION_CHECKLIST.md` | |

---

## Sign-off

| Role | Name | Signature | Date |
|------|------|-----------|------|
| Developer | | | |
| Technical Reviewer | | | |
| Security Reviewer | | | |

**Merge is blocked until all ✅ items are complete and signed off.**
