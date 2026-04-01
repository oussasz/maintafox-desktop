# Maintafox Desktop — Release Checklist

Use this checklist before promoting any build from `staging` to `main` and creating a
release tag. It is designed to be completed by the maintenance engineer supervisor and
the release manager together.

## Pre-Promotion Checks (staging environment)

### CI Green
- [ ] All CI jobs pass on the `staging` branch: lint, Rust quality, frontend tests, Tauri
  build check
- [ ] Security audit job shows 0 high-severity vulnerabilities

### Changelog and Versioning
- [ ] `CHANGELOG.md` [Unreleased] section has entries for every sprint included in this
  release
- [ ] `package.json`, `Cargo.toml`, and `tauri.conf.json` all carry the same version
  string
- [ ] The version string follows the Phase versioning table in `VERSIONING_POLICY.md`

### Supervisor Acceptance
- [ ] Every sprint in this release has a completed Supervisor Feedback issue marked
  "Approved to Continue"
- [ ] No open issues labeled `priority:critical` or `type:security` targeting this release
- [ ] The supervisor has run the application on a clean Windows machine (not the
  development machine) and confirmed it starts correctly

### Database and Migration Safety
- [ ] All migrations in this release are additive (no DROP TABLE, no DROP COLUMN without
  prior deprecation cycle)
- [ ] `db:reset` script completes on a clean environment after applying this release
- [ ] SQLX offline cache (`.sqlx/`) is up to date (run `cargo sqlx prepare` if schema
  changed)

### Security
- [ ] `cargo audit` reports 0 known critical/high CVEs in Rust dependencies
- [ ] `pnpm audit --audit-level=high` reports 0 high-severity issues in npm dependencies
- [ ] Gitleaks scan reports 0 leaks
- [ ] No hardcoded credentials or signing keys in any tracked file

## Promotion and Tagging Procedure

1. Open a PR from `staging` → `main`.
2. Get 2 approvals from the release-manager team.
3. Merge to `main`.
4. Create an annotated signed tag on `main`:
   ```
   git tag -a v{VERSION} -m "Maintafox {VERSION} — {brief description}"
   git push origin --tags
   ```
5. The release workflow generates the signed bundle automatically from the tag.
6. Post-tag: confirm the GitHub release page shows the correct installer artifacts.

## Post-Release Monitoring (first 48 hours)

- [ ] No critical bug reports from pilot users within 2 hours of release
- [ ] Sync coordinator shows no error spikes (if VPS is active)
- [ ] Application launches correctly on at least 2 independent Windows machines
- [ ] Supervisor confirms the release notes match the observable changes

## Hotfix Trigger Criteria

A hotfix must be started immediately if any of the following occur after release:
- Application crash on startup affecting more than one machine
- Data loss or corruption in the local database
- Authentication bypass or unauthorized access vector discovered
- Any work order or intervention request cannot be created or closed
