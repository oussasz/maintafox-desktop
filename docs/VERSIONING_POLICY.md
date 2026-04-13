# Maintafox Desktop — Versioning Policy

## Semantic Versioning

Maintafox Desktop follows [Semantic Versioning 2.0.0](https://semver.org).

Given version `MAJOR.MINOR.PATCH`:

- **MAJOR** increments when a release breaks backward compatibility with existing
  local databases or sync protocols. In practice, MAJOR increments from 0 to 1 at the
  first production release (v1.0.0). A MAJOR increment in production requires a migration
  guide and customer communication.
- **MINOR** increments when new features, modules, or capabilities are added in a
  backward-compatible way. Maintenance engineers should expect new screens, commands, or
  options, but existing workflows continue without changes.
- **PATCH** increments for bug fixes, security patches, and translation corrections that
  do not change behavior or add features.

## Pre-Release Versioning by Phase

| Phase | Version Range | Notes |
|---|---|---|
| Phase 1 — Secure Foundation | `0.1.x` | Foundation sprint only; no user-facing features |
| Phase 2 — Core Execution Backbone | `0.2.x` | Work orders, requests, and RBAC complete |
| Phase 3 — Planning, Compliance, Material Control | `0.3.x` | Planning, inventory, PM, permits |
| Phase 4 — Control Plane and Integrations | `0.4.x` | Sync, licensing, IoT, ERP |
| Phase 5 — Reliability and Launch Hardening | `1.0.0-rc.N` → `1.0.0` | Final hardening and pilot |

## Build Metadata and Tags

Pre-release tags: `v0.2.0-dev`, `v0.3.1-beta.1`, `v1.0.0-rc.2`

Release tags: `v1.0.0`, `v1.1.0`, `v1.1.1`

All Git tags are annotated and GPG-signed (see `docs/BRANCHING_STRATEGY.md`).

## How to Bump the Version

1. In `package.json`, bump `"version"` to the new value.
2. In `src-tauri/Cargo.toml`, bump `version` to the same value.
3. In `src-tauri/tauri.conf.json`, bump `"version"` to the same value.
4. Ensure these three files are bumped in the same commit and PR.
5. Move the `## [Unreleased]` section in `CHANGELOG.md` to a new dated section:
   `## [0.2.0] — 2026-MM-DD`
6. The release CI workflow auto-tags `main` on merge when the above files are consistent.

## Current Version Sync Status

All three files are currently synchronized at `0.1.0-dev` (Phase 1).

## Minimum Required Files in Every Release PR (to `main`)

- [ ] `package.json` version updated
- [ ] `src-tauri/Cargo.toml` version updated
- [ ] `src-tauri/tauri.conf.json` version updated
- [ ] `CHANGELOG.md` has at least one entry under `[Unreleased]`
- [ ] All three version strings are identical
