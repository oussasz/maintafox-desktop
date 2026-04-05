# Secrets Management

This document defines the secrets governance policy for the Maintafox Desktop project.
It covers where secrets are stored, how they are named, when they must be rotated, and
what to do if a secret is exposed.

---

## 1. Secret Categories and Where They Live

| Secret Category | Storage Location | Who Manages It |
|---|---|---|
| Developer environment secrets | `.env.local` (gitignored, per machine) | Each developer |
| CI signing and deployment keys | GitHub Actions repository secrets | Release manager |
| Staging environment secrets | GitHub Actions `staging` environment secrets (gated) | Release manager |
| Production environment secrets | GitHub Actions `production` environment secrets (2 reviewers) | Release manager |
| Runtime session material | OS keyring via Rust `keyring` crate | Rust application core |
| Local database encryption key | OS keyring via Rust `keyring` crate | Rust application core |

---

## 2. Secret Naming Convention

All secret names follow: `MAINTAFOX_{CONTEXT}_{PURPOSE}`

Examples:

- `MAINTAFOX_TAURI_SIGNING_KEY` — Tauri update bundle signing private key
- `MAINTAFOX_TAURI_SIGNING_PASSWORD` — Password for the signing key
- `MAINTAFOX_VPS_DEPLOY_KEY` — SSH key for VPS deployment operations
- `MAINTAFOX_VPS_API_TOKEN` — Bearer token for VPS API authentication

---

## 3. Rotation Policy

| Secret Type | Rotation Frequency | Trigger for Immediate Rotation |
|---|---|---|
| Tauri signing key pair | 12 months | Suspected key exposure |
| VPS API tokens | 6 months | Personnel changes, suspected exposure |
| VPS deploy keys | 12 months | Suspected compromise |
| Session tokens | Per session | Logout, device revocation, or step-up reauth |
| DB encryption key | Only on security incident | Never routine-rotated (tied to data) |

---

## 4. Prohibited Storage Locations

Secrets must NEVER be stored in:

- Git history (including deleted files — git history is permanent)
- Application log files or tracing output
- SQLite rows in plaintext form (encrypted DB rows are acceptable for session material
  under OS keyring governance)
- IPC response payloads sent to the frontend
- `tauri.conf.json` or any file tracked in the repository
- CI workflow `echo` or `run` steps that would print to log output

If a secret is found in a prohibited location, treat it as a confirmed exposure and rotate
immediately, even if the repository is private.

---

## 5. Secret Scanning

GitHub secret scanning is enabled on the repository (Settings → Security). Additionally,
the CI `security-audit` job runs `gitleaks` on every PR to `main` and `staging`, and on
a weekly schedule against the full git history.

The `.gitleaks.toml` file at the project root defines allowlisted patterns for intentional
placeholder key names in `.env.example` and documentation files.

---

## 6. Incident Response Protocol

If a secret is suspected or confirmed to have leaked:

1. **Rotate immediately.** Do not investigate before rotating — rotate first.
2. Revoke the exposed token or key at the issuing service (GitHub secrets, VPS panel).
3. Generate and register replacement credentials.
4. Scan git history: `git log --all -p | grep "{first-8-chars-of-exposed-value}"`
   If found, note the commit SHA.
5. If the secret was in a public repository or accessed by an unauthorized party,
   assess scope of exposure and notify affected parties.
6. Open a GitHub issue labeled `type:security` and `priority:critical` documenting:
   - What was exposed, when, and how it was discovered
   - What was rotated and when
   - Whether unauthorized access is suspected
7. Conduct a post-incident review within 72 hours to close the exposure vector.
