# Sections 7-12 Platform Architecture and Security Research Brief

## 1. Research Position

These sections should not read like a generic desktop app architecture note.

After the hardening of chapter 6, Maintafox is now clearly a governed, local-first maintenance operating system with:

- structured operational evidence
- append-only audit expectations
- admin-defined but protected configuration
- offline-capable identity and trusted-device logic
- staged reliability analytics
- ERP and IoT integration contracts

That means the data, sync, licensing, updater, and security sections must now define explicit contracts rather than broad implementation intentions.

## 2. Source Signals

### 2.1 Hardened modules already imply stronger platform rules

The rewritten chapter-6 modules require the platform to support:

- stable asset and personnel identity across time and machines
- append-only audit and activity evidence
- governed configuration publish and rollback
- idempotent integration and replay-safe synchronization
- safe offline operation without weakening security controls

Those expectations are incompatible with simplistic global last-write-wins, deterministic encryption keys, or loosely defined update trust.

### 2.2 Tauri's security model reinforces narrow trust boundaries

Tauri documents a clear trust boundary between frontend WebView code and Rust core code. The IPC boundary must be strongly defined and capabilities must be explicitly granted. The updater plugin also requires signed artifacts and does not allow signatures to be disabled.

For Maintafox, that supports three concrete platform decisions:

- no local web server for ordinary app behavior
- narrow, typed IPC commands and capability scopes
- a dedicated update-signing key distinct from licensing or session secrets

### 2.3 Auth and settings research already require stronger key and session design

The authentication and settings research already established that:

- trusted-device bootstrap and offline grace must be policy-driven
- secrets belong in OS-managed stores, not only in SQLite
- sensitive actions require step-up reauthentication

Therefore data-at-rest and session security cannot rely on weakly derived secrets such as only machine fingerprint plus license string.

## 3. Platform Implications

### 3.1 Local data remains the operational source of truth

Maintafox should keep the local desktop database as the runtime source of truth for operational workflows. The VPS mirror coordinates multi-machine synchronization, licensing, updates, and centralized administration. It should not become a hidden online dependency for day-to-day work.

### 3.2 Synchronization must be idempotent and class-aware

The sync layer should distinguish:

- append-only event data
- governed reference and configuration versions
- mutable operational records

These data classes should not all use the same merge rule.

### 3.3 Local schema integrity should remain strong

SQLite should enforce local referential integrity for ordinary business data. Sync flexibility should be achieved through staging, tombstones, and conflict review rather than by disabling foreign keys globally.

### 3.4 Security keys should be separated by purpose

Maintafox should separate at least four key types:

- installation master secret for local cryptography
- session-signing secret for local HS256 session JWTs
- VPS-held signing key for entitlement or license verification
- dedicated updater-signing key for release artifacts

## 4. Recommended PRD Upgrade Summary

- replace simplistic database and sync notes with explicit local-schema, outbox, inbox, checkpoint, and conflict-governance rules
- define class-aware sync conflict policies instead of one global last-write-wins rule
- define licensing as an entitlement and activation control plane, not just a heartbeat check
- define updater trust using signed artifacts, release channels, rollback policy, and migration safety
- strengthen the security section around OS-managed secrets, SPKI pinning, Tauri trust boundaries, and step-up reauthentication

## 5. Source Set

- Tauri Security Overview: https://v2.tauri.app/security/
- Tauri Updater Plugin: https://v2.tauri.app/plugin/updater/
- Maintafox research brief: MODULE_6_1_AUTHENTICATION_AND_SESSION_MANAGEMENT.md
- Maintafox research brief: MODULE_6_18_APPLICATION_SETTINGS_AND_CONFIGURATION_CENTER.md
- Maintafox research brief: MODULE_6_21_IOT_INTEGRATION_GATEWAY.md
- Maintafox research brief: MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md
- Maintafox research brief: MODULE_6_24_BUDGET_AND_COST_CENTER_MANAGEMENT.md
