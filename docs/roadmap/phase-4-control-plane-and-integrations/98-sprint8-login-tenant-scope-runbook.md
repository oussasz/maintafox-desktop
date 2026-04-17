# Sprint 8 Runbook — Login and Tenant Scope Enforcement

## Purpose

Provide operators and support with a fast troubleshooting guide for tenant-scoped login after activation-first onboarding.

## Normal Path (Expected)

1. Device activation already completed (`ProductLicenseGate` status is active or degraded with claim present).
2. User logs in with valid local credentials.
3. Session is created with tenant claims derived from activation + role scope checks.
4. Route and service guards enforce least-privilege and tenant isolation.

## Failure Classes and Recovery

### 1) Unauthorized Account for Activated Tenant

- **Signal**: login error code `TENANT_SCOPE_VIOLATION`.
- **UI message**: account is not authorized for activated tenant.
- **Recovery**:
  - use a user account assigned to the activated tenant scope, or
  - re-activate the device with the intended tenant key.

### 2) Stale / Invalid Session Claims

- **Signal**: session refresh error code `SESSION_CLAIM_INVALID`.
- **UI message**: session tenant claim is stale/invalid.
- **Recovery**:
  - user signs in again after activation claim changes,
  - if claim missing, re-enter activation key and retry login.

### 3) First Login Offline Not Allowed

- **Signal**: auth rejection for first login when device trust has never been registered.
- **Recovery**:
  - connect network once, perform login to register trust, then offline policy applies on subsequent logins.

### 4) Activation Deny/Degraded States

- **Signal**: `ProductLicenseGate` deny/degraded statuses (`revoked`, `expired`, `slot_limit`, `force_update_required`, `api_down`).
- **Recovery**:
  - follow activation denial reason, update/reissue key if needed,
  - revalidate once online for degraded/API-down conditions.

## Enforced Guard Map (Code Locations)

- **Activation-first gate ordering**: `src/router.tsx`
- **Pre-auth activation UX + deny/degraded handling**: `src/components/auth/ProductLicenseGate.tsx`
- **Login/session routing**: `src/components/auth/AuthGuard.tsx`
- **Tenant claim extraction from activation record**: `src-tauri/src/commands/product_license.rs`
- **Login tenant scope authorization**: `src-tauri/src/commands/auth.rs`
- **Session tenant claim freshness check**: `src-tauri/src/commands/auth.rs`
- **Session claim payload surface**: `src-tauri/src/auth/session_manager.rs`, `shared/ipc-types.ts`, `src/services/auth-service.ts`
- **Tenant runtime API guard contract**: `src/services/vps-contract-guards.ts`
- **Route least-privilege checks**: `src/components/auth/PermissionRoute.tsx`

## Verification Matrix (Sprint 8)

- Normal login after activation succeeds.
- Tenant mismatch login is denied with actionable message.
- Cross-tenant runtime context fails contract guard.
- Degraded/offline replay metadata does not bypass tenant isolation guard.
