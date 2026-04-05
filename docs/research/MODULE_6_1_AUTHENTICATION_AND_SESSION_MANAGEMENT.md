# Module 6.1 Research

## Authentication and Session Management

### 1. Research Objective

This brief documents how a modern CMMS or EAM authentication module should work when the product must support three difficult realities at the same time:

- strict security expectations
- offline-first desktop usage in industrial environments
- shared workstations used by technicians, supervisors, and administrators

Maintafox already defines Module 6.1 as an offline-capable login system with Argon2id, short-lived access tokens, refresh tokens stored in the OS keychain, inactivity lock, and support for multiple users on the same machine. The purpose of this research is to validate that direction against international security guidance and competitor behavior, then identify what should be added to make the module production-grade.

### 2. Why This Module Is Critical

Authentication in a CMMS is not only a gate to the application. It is also the control plane for:

- who can see equipment, work orders, and intervention requests
- who can approve, reject, close, or archive operational records
- who can work offline and later synchronize back to the central system
- who can unlock a shared device without exposing another user's cached operational data
- who can administer configuration, ERP connectors, or security policy

A weak authentication model in this context creates operational risk, data-integrity risk, and audit risk. A good module must therefore separate identity verification, session continuity, offline entitlement, device trust, and reauthentication for sensitive actions.

### 3. Standards Baseline

The strongest common baseline from OWASP and NIST is clear: authentication must be usable enough that users do not work around it, but strict enough that shared devices, reused passwords, stale sessions, and cached secrets do not undermine the system.

#### 3.1 OWASP Guidance Relevant to Maintafox

OWASP's Authentication Cheat Sheet supports the following design principles:

- allow long passwords and passphrases rather than short, complex-only passwords
- avoid arbitrary password composition rules that reduce usability without improving security
- use generic error messages for failed sign-in so username validity is not leaked
- throttle repeated login failures and record them in audit logs
- require reauthentication for sensitive actions, not only at initial login
- log authentication, session, lockout, and recovery events for monitoring and investigation

Practical implication for Maintafox: the login screen should be simple, but the system behind it must enforce throttling, audit logging, and step-up authentication for critical actions such as changing permissions, exporting sensitive data, restoring backups, or modifying ERP credentials.

#### 3.2 NIST SP 800-63B Guidance Relevant to Maintafox

NIST guidance adds several operationally important constraints:

- minimum password length should be usable and compatible with passphrases
- verifiers should compare candidate passwords against known compromised or weak-password lists
- periodic forced password changes should not be the default unless compromise is suspected
- reauthentication should occur after inactivity and within a bounded maximum session duration for stronger assurance contexts
- biometrics should not be treated as a standalone identity proof; they are appropriate as an unlock or second factor mechanism

Practical implication for Maintafox: PIN and biometric unlock should be treated as local session-unlock tools after a full authenticated sign-in, not as the only factor for initial identity establishment.

### 4. Official Platform Security Storage Guidance

For a desktop product, secure local session continuity depends on the operating system, not only the application database.

#### 4.1 Windows

Microsoft's Credential Locker guidance confirms three points that matter directly to Maintafox:

- credentials should be stored in Windows secure credential storage, not in plain application data
- storage is appropriate for small secrets such as passwords, refresh tokens, or secure session material, not large data blobs
- apps should only save credentials after a successful sign-in and only when the user or policy allows persistence

Practical implication: Maintafox should store refresh tokens, unlock secrets, and device-bound authentication material in secure OS storage on Windows rather than in SQLite or local config files.

#### 4.2 macOS

Apple's Keychain Services guidance confirms the same pattern on macOS:

- the keychain is an encrypted system store for small secrets such as passwords, keys, certificates, and authentication material
- access control should be delegated to the OS keychain model rather than reimplemented in app-level storage

Practical implication: Maintafox should rely on native secure storage abstractions on macOS as the primary persistence location for refresh tokens and similar secrets.

### 5. Competitor Benchmark Findings

The competitor documentation shows that strong CMMS products do not treat authentication as a single login form. They treat it as a lifecycle spanning first login, offline cache preparation, session renewal, user switching, and enterprise identity integration.

#### 5.1 IBM Maximo Mobile

IBM Maximo Mobile provides one of the clearest models for offline-capable enterprise authentication:

- first login requires connectivity
- the app creates a local encrypted data store after successful authentication
- refresh and access tokens have distinct lifetimes
- offline work is explicitly supported from previously synchronized local data
- administrators can centrally disable offline login and device authentication through managed-device policy
- IBM documents an important shared-device caveat: if one user has cached local data, another user on the same device may need to authenticate online before the device safely switches context

What this means for Maintafox: offline capability must be tied to prior online bootstrap, and shared-device behavior must be explicitly designed rather than assumed.

#### 5.2 Fiix

Fiix documentation shows an enterprise-oriented pattern:

- SSO is positioned as an enterprise feature
- setup is controlled and coordinated with the vendor or deployment team
- SAML 2.0 and OpenID Connect are supported
- inactivity timeout is treated as a configurable session-policy control

What this means for Maintafox: SSO should be modeled as a tenant configuration layer with policy controls, not as a hardcoded alternative login screen.

#### 5.3 MaintainX

MaintainX documentation emphasizes operational usability around enterprise identity:

- SSO can be routed by email domain
- default roles can be assigned to new SSO users
- multiple organizations can be supported under SSO policy
- enterprise plans add custom roles, custom permissions, and stronger authentication features
- workstation mode and PIN-related permissions show that fast unlock workflows are deliberately separated from full user administration

What this means for Maintafox: authentication design should include tenant-aware routing, role mapping, and a clear difference between first-factor login and fast local unlock.

#### 5.4 UpKeep

UpKeep shows a mature SSO operations model:

- organization-specific login entry can be simplified with a company identifier or direct SSO link
- default user type can be assigned at SSO entry time
- user profile attributes can be synchronized on each login
- native login can coexist with SSO when policy allows it
- forced periodic reauthentication is treated as a policy tool

What this means for Maintafox: the system should support both identity routing and profile/role reconciliation at sign-in time.

#### 5.5 Limble

Limble documentation is strong on identity-source discipline:

- email is the primary identity anchor for SSO users
- once a user is configured for SSO, native password login can be disabled
- SCIM or automatic provisioning is preferred over manual synchronization

What this means for Maintafox: if SSO is enabled for a tenant, the product should define whether users are hybrid accounts or SSO-only accounts, and provisioning rules should be explicit.

### 6. Derived Design Principles For Maintafox

Across standards and competitors, several patterns repeat consistently:

1. First authenticated bootstrap and local unlock are not the same event.
2. Offline access should only be granted to users and devices that were previously provisioned online.
3. Shared-device support requires per-user cache isolation and a safe user-switch flow.
4. Session timeout, idle lock, and reauthentication are separate controls and should be configurable independently.
5. Enterprise identity requires mapping rules for role, organization scope, and user provisioning.
6. Secure local storage must rely on OS-managed secret stores, not only on the local database.

These principles fit Maintafox's local-first architecture very well, but they need to be made explicit in the product definition.

### 7. Recommended Maintafox Operating Model

The recommended model below is practical for a Tauri desktop product that must work online and offline.

#### 7.1 Authentication Types

Maintafox should support three primary identity modes:

- local Maintafox account with username or email plus password
- enterprise SSO account via SAML 2.0 or OpenID Connect
- local fast-unlock method such as PIN or biometric, but only after prior full authentication

Fast unlock must never be treated as the primary enrollment method for a new device.

#### 7.2 First Login On A Device

The first login on a device should always be online. The flow should be:

1. User selects tenant or the tenant is inferred from configuration.
2. User authenticates with password or SSO.
3. Server validates identity, role, organization scope, and security policy.
4. Desktop registers the device and receives a device-bound trust record.
5. Desktop stores refresh token and device secret in secure OS storage.
6. Desktop saves a minimal encrypted offline identity cache and policy snapshot.
7. Desktop performs initial data synchronization and opens the application.

This prevents unprovisioned devices from gaining offline access without an online trust ceremony.

#### 7.3 Returning Login While Online

When connectivity exists, the product should prefer silent session renewal. The user experience should be:

- if refresh token is valid, restore the session without forcing password re-entry
- if policy changed since last login, apply new policy before opening modules
- if role or organization mapping changed, rehydrate the local cache using the new scope
- if server revoked the device or account, stop restore and require a fresh authenticated sign-in

This matches enterprise expectations and reduces unnecessary friction for daily users.

#### 7.4 Returning Login While Offline

Offline entry should only work when all of the following are true:

- this user has already authenticated online on this device before
- the device has not been revoked
- the user account is not marked locally disabled
- the offline grace window has not expired
- the local cache and token material pass integrity checks

If any condition fails, the application should refuse offline entry and explain that an online sign-in is required.

#### 7.5 Session Layers

The module should explicitly separate four session layers:

- authenticated account identity
- short-lived API access token
- longer-lived refresh token or renewable session grant
- local idle-unlock state for reopening a locked desktop session

Recommended defaults based on the research:

- access token: 15 to 30 minutes
- refresh token or renewable grant: 8 to 12 hours online, policy-driven
- idle lock: 15 to 30 minutes, configurable
- absolute session maximum: 12 hours by default for standard users, shorter for privileged roles if required

This is more robust than modeling all session behavior with only one JWT expiry.

#### 7.6 Sensitive Action Reauthentication

The system should require step-up authentication for high-risk actions even inside an active session. Examples:

- changing role permissions
- editing SSO configuration
- changing ERP connector credentials
- exporting personally identifiable or cost-sensitive data
- restoring database backups
- deleting users or disabling audit controls

Reauthentication can be password-only, SSO redirect, or policy-driven MFA confirmation depending on tenant configuration.

#### 7.7 Shared Workstation Model

Shared devices are common in maintenance offices and workshop kiosks. Maintafox should therefore implement an explicit user-switch workflow:

1. Current user locks or switches user.
2. All decrypted in-memory state is cleared.
3. The next user sees a neutral unlock or sign-in screen, not the previous user's data.
4. Cached data remains separated by tenant and by user profile.
5. If the new user has never authenticated online on this device, offline entry is denied.

The IBM Maximo caveat is important here: if shared-device switching is not explicitly engineered, the local offline store can expose the wrong user context. Maintafox should treat this as a design constraint, not an edge case.

#### 7.8 Password Reset And Recovery

Password recovery should not be treated as a purely offline capability. Recommended rule set:

- local-password reset requires online validation, administrator approval, or both depending on tenant policy
- SSO users are redirected to the identity provider's recovery process
- offline users who forget their password should be blocked from login until connectivity or authorized administrative intervention exists

This is operationally strict, but it avoids creating weak offline recovery loopholes.

### 8. Recommended User Experience

The quality of this module depends heavily on the clarity of the login and lock flows.

#### 8.1 Login Screen

The login entry should show:

- tenant name or site context
- username or email field
- password field
- SSO button when configured
- connectivity status
- explicit offline availability state
- neutral error messaging for invalid credentials

If the tenant has domain-routing rules, the system can detect that after the user enters email and direct them to the correct login method.

#### 8.2 Lock Screen

The lock screen should be different from the full login screen. It should:

- identify the last active user
- support PIN or biometric unlock if policy allows it
- allow fallback to password
- allow a safe switch-user action
- show unsynchronized-changes warning if the user tries to log out or switch while data is pending

This distinction improves usability without weakening first-factor authentication.

#### 8.3 Session Expiry Experience

The current PRD already includes a warning modal two minutes before expiry. That is useful, but it should be clarified:

- silent renewal should happen automatically when policy allows it and connectivity exists
- user-facing warning should appear when silent renewal is impossible, when offline grace is ending, or when the absolute session limit is reached
- if reauthentication is required, the user should remain in context after successful confirmation

#### 8.4 Failure Messages

User-visible failure states should be operationally precise without leaking identity details. Good categories include:

- Sign-in failed. Check your credentials.
- Offline sign-in is not available for this account on this device.
- Your account requires online reauthentication.
- This device is no longer trusted. Contact your administrator.
- Your session expired. Sign in again to continue.

### 9. Recommended Data And Control Model

The current PRD lists `users`, `refresh_tokens`, and `session_log`. That is a good starting point, but it is too narrow for the behavior defined above.

Recommended additions:

- `device_registrations`: device_id, user_id, tenant_id, trusted_at, last_online_at, revoked_at, offline_allowed_until, unlock_mode, policy_version
- `auth_identities`: user_id, provider_type, external_subject, email, username, sso_enforced
- `session_log`: expand action taxonomy to include login_failed, token_refreshed, lock, unlock, switch_user, reauth_required, reauth_success, revocation_applied
- `auth_policy_snapshots`: policy_version, password_policy, idle_timeout_minutes, absolute_session_minutes, offline_grace_hours, allowed_unlock_methods
- `mfa_factors` if MFA will exist beyond roadmap language

If the team wants to keep the database smaller in v1, `device_registrations` and expanded `session_log` are the most important first additions.

### 10. Required Security Controls

To be production-grade, Maintafox Module 6.1 should enforce the following controls:

- Argon2id hashing with calibrated parameters and per-user salt
- breached-password screening for local accounts
- login throttling and temporary lockout after repeated failures
- refresh-token rotation with revocation support
- local secret storage in OS secure store, not plain database fields
- encrypted local user cache tied to tenant and device context
- reauthentication for sensitive actions
- audit logs for success, failure, lock, unlock, reset, switch-user, and revocation events
- policy-driven offline grace period
- device revocation handling on reconnect

### 11. Configurability Requirements

This module should be highly configurable at tenant level. The most important admin settings are:

- enable or disable offline login globally
- idle timeout duration
- absolute session duration
- offline grace duration since last online verification
- allow password, PIN, biometric, or password-only unlock
- local login enabled, SSO enabled, or hybrid mode
- SSO provider configuration and domain routing
- default role and organization mapping for newly provisioned SSO users
- require reauthentication for selected sensitive actions
- shared-workstation mode enabled or disabled

This aligns with the broader Maintafox v3.0 direction toward full runtime configurability.

### 12. Gaps Between Current PRD And Recommended Production Behavior

The current PRD direction is strong, but the following items should still be added or clarified before implementation:

1. Add an absolute session maximum, not only idle timeout.
2. Distinguish first login from local unlock.
3. Define device registration and offline entitlement explicitly.
4. Define how SSO accounts behave offline after initial bootstrap.
5. Expand `session_log` to cover lock, unlock, refresh, revocation, and failed login.
6. Add throttling, lockout, and breached-password screening for local accounts.
7. Define shared-device cache isolation and switch-user behavior clearly.
8. Define recovery behavior for forgotten passwords and revoked devices.
9. Add step-up authentication for privileged actions.
10. Add tenant-configurable auth policies under the Configuration Engine.

### 13. Recommended Maintafox Positioning

If Maintafox follows this model, the product can legitimately claim that its authentication system is:

- offline-capable but not offline-trusting by default
- secure enough for shared industrial workstations
- ready for enterprise SSO and role mapping
- aligned with modern password and session standards
- practical for technicians who need fast unlock without constant full re-login

That positioning is stronger and more credible than simply saying the product uses JWT and Argon2id.

### 14. Source Set

Primary standards and platform references used in this research:

- OWASP Authentication Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html
- NIST SP 800-63B Digital Identity Guidelines: https://pages.nist.gov/800-63-4/sp800-63b.html
- Microsoft Credential Locker for Windows apps: https://learn.microsoft.com/en-us/windows/apps/develop/security/credential-locker
- Microsoft Credential Manager overview: https://learn.microsoft.com/en-us/windows/win32/secauthn/credential-manager
- Apple Keychain Services: https://developer.apple.com/documentation/security/keychain-services
- Apple Keychains: https://developer.apple.com/documentation/security/keychains

Official competitor references used in this research:

- IBM Maximo Mobile documentation
- IBM Maximo Mobile AppConfig documentation
- Fiix SSO and session-timeout documentation
- MaintainX SSO, enterprise security, and role-permission documentation
- UpKeep SSO configuration and login-flow documentation
- Limble SSO and provisioning documentation

### 15. Bottom Line

The research strongly supports Maintafox's existing direction, but it also shows that a serious authentication module for industrial desktop software must be modeled as a full lifecycle system. The core upgrade is conceptual: treat authentication, session renewal, offline entitlement, device trust, fast unlock, and reauthentication as separate but connected mechanisms.

If Maintafox adds those missing layers, Module 6.1 will be realistic, defensible, and competitive with established CMMS platforms.