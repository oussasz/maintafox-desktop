# Maintafox — Updater Signing Key Architecture

> **Audience:** Phase 2 DevOps engineer, security reviewer  
> **Status:** Phase 1 placeholder — key generation is a Phase 2 CI/CD task  
> **Related:** PRD §11.1–§11.4, PRD §12.5

---

## Overview

Maintafox uses **three independent key pairs** for cryptographic operations. They share
no material and cannot be used interchangeably.

| Key Pair | Algorithm | Purpose | Owner |
|----------|-----------|---------|-------|
| Session signing key | RS256 (RSA) | Signs JWT session tokens | SP04 · never leaves the device |
| Device entitlement key | Symmetric / device-specific | Trusted-device registration | SP04 · device-bound |
| **Updater signing key** | **Ed25519** | **Authenticates update bundles** | **This document** |

A compromise of the session signing key **cannot** be used to push a malicious update
bundle. A compromise of the updater key **cannot** escalate session privileges or
impersonate a user. This independence is maintained by design, not by operational policy.

---

## Why Key Separation Matters

The `tauri-plugin-updater` update flow:

1. The app fetches a manifest JSON from the configured HTTPS endpoint.
2. The manifest contains: `version`, `pub_date`, `notes`, `url`, and `signature`.
3. The plugin downloads the bundle from `url`.
4. It verifies the bundle against `signature` using the **updater public key** embedded
   in `tauri.conf.json` at compile time.
5. If verification fails, the bundle is **discarded** and the update is aborted. The
   currently-installed version continues running.

If the updater key reused the session signing key, a session key compromise would also
give an attacker the ability to forge update bundles. Separate key material closes this
attack path regardless of how the session key is compromised.

---

## Key Generation — Phase 2 DevOps Task

The updater key pair is generated **once per product**, stored in the CI/CD secrets
vault, and **never committed to source control or stored on developer machines**.

```bash
# Run ONLY in the CI/CD environment — NOT on developer machines.
# Requires Tauri CLI installed.
tauri signer generate -w ~/.tauri/maintafox-updater.key

# Output:
#   Private key: ~/.tauri/maintafox-updater.key   ← KEEP SECRET, store in CI vault
#   Public key:  printed to STDOUT (base64)        ← safe to embed in tauri.conf.json
```

### Storage locations

| Material | Where it lives |
|----------|---------------|
| Private key | CI/CD secrets vault (e.g. GitHub Actions Secret `TAURI_UPDATER_PRIVATE_KEY`) |
| Private key (optional) | Hardware Security Module (HSM) for production signing |
| Public key | `tauri.conf.json` → `plugins.updater.pubkey` (committed to source control) |

The private key is consumed during `tauri build` to sign the bundle:

```bash
# The build pipeline sets TAURI_PRIVATE_KEY from the vault secret, then:
tauri build
# tauri-cli reads TAURI_PRIVATE_KEY, signs the bundle with Ed25519, and
# embeds the signature in the release manifest automatically.
```

---

## Embedding the Public Key

After `tauri signer generate`, copy the base64 public key from STDOUT and replace the
Phase 1 placeholder in `src-tauri/tauri.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "<base64-public-key-from-tauri-signer-generate>",
      "endpoints": [
        "https://updates.maintafox.com/{{channel}}/{{target}}/{{arch}}/{{current_version}}"
      ]
    }
  }
}
```

The public key is safe to commit to source control — it is a public key. Only the
private key must remain secret.

---

## Phase 1 Placeholder

In Phase 1, `tauri.conf.json` contains:

```json
"pubkey": "PLACEHOLDER_UPDATER_PUBLIC_KEY_REPLACE_IN_PHASE2"
```

With this placeholder the plugin initialises successfully but **any update bundle it
encounters fails signature verification and is discarded**. This is the correct and safe
behaviour for Phase 1: a misconfigured or stale manifest endpoint cannot accidentally
push an unsigned binary to developer machines.

**Do not replace this placeholder during Phase 1.** Key generation requires a
production CI/CD environment and is a Phase 2 DevOps sprint deliverable.

---

## Release Channel Architecture

All three channels are signed with the **same** updater key pair. Channel isolation is
achieved through separate manifest endpoints, not separate keys.

| Channel | Audience | `updater.release_channel` value |
|---------|---------|--------------------------------|
| `stable` | All customers | `"stable"` (default) |
| `pilot` | Early-access customers | `"pilot"` |
| `internal` | Internal testing only | `"internal"` |

The active channel is stored in `app_settings` under key `updater.release_channel`
(scope: `device`). When an admin changes the channel in the Settings UI, the next
update check fires against the new manifest endpoint.

Manifest endpoint URL pattern (Phase 2 CI/CD configuration):

```
https://updates.maintafox.com/{channel}/{target}/{arch}/{current_version}
```

---

## Manifest Response Contract

All manifest endpoints must return JSON that conforms to this schema:

```json
{
  "version": "1.2.3",
  "pub_date": "2026-06-15T00:00:00Z",
  "notes": "Release notes in markdown format",
  "url": "https://updates.maintafox.com/stable/maintafox-1.2.3-x86_64.msi.zip",
  "signature": "base64-encoded-Ed25519-signature-of-bundle"
}
```

When no update is available the endpoint returns **HTTP 204 No Content** or an empty
JSON object `{}`. The plugin treats both as "no update available" and sets
`available: false` in the `UpdateCheckResult` returned to the frontend.

---

## Security Requirements

1. **HTTPS mandatory.** The updater plugin refuses plain-HTTP endpoints in production
   builds. Non-HTTPS fallback is disabled in `tauri.conf.json`. This requirement is
   enforced at the Tauri layer, not at the application layer.

2. **No update without signature.** If the `signature` field is absent, empty, or fails
   Ed25519 verification against the embedded public key, the bundle is silently
   discarded. The application continues running the installed version.

3. **Session state is not consulted during bundle verification.** `AuthState` and the
   active session are irrelevant to update authenticity. The trust anchor is solely the
   public key embedded in `tauri.conf.json` at compile time.

4. **`install_pending_update` IPC command requires an active session.** This is an
   application-level gate (`require_session!` macro, `commands/updater.rs`), separate
   from bundle signature verification. Its purpose is to ensure a user is present and
   authenticated before a restart-inducing action is taken — not to authenticate the
   bundle itself.

---

## Key Rotation Procedure

If the updater private key is compromised:

1. Generate a new Ed25519 key pair with `tauri signer generate`.
2. Update `plugins.updater.pubkey` in `tauri.conf.json` with the new public key.
3. Build and sign a security-patch release using the **existing (compromised) key** so
   currently-installed clients can verify and receive it.
4. After that release reaches customers, retire the old private key from the vault.
5. All subsequent releases are signed only with the new key.

The window between step 1 and step 3 is the risk window. Minimise it by treating key
compromise as a P0 incident with an SLA matching your fastest release cycle.

---

## References

- Tauri v2 updater plugin documentation: https://tauri.app/plugin/updater/
- `tauri-plugin-updater` crate: https://crates.io/crates/tauri-plugin-updater
- PRD §11.1–§11.4: Automatic Update System
- PRD §12.5: Key Separation Architecture
- SP06-F02: Updater Skeleton and Release Channel Contracts (this sprint)
