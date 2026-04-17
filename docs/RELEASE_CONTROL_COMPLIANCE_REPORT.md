# Release Control Compliance Report

**Repository:** `oussasz/maintafox-desktop`  
**Audit date:** 2026-04-17  
**Scope:** Alignment with `docs/BRANCHING_STRATEGY.md`, `.github/branch-protection.md`, and `.github/workflows/ci.yml`  
**Auditor role:** Automated verification via GitHub API + repository inspection  

---

## Executive summary

The repository is **public**, default branch **`main`**, and **branch protection is active** on `main`, `staging`, and `develop`. CI gates match the documented job names (see naming note below). The promotion train (`feature → develop → staging → main`) was repaired and completed via pull requests; obsolete duplicate branches (`master`, temporary repair branches) were removed, with legacy history preserved on `archive/master-legacy`.

**Overall posture:** **Compliant** with the documented branching model and PR-first workflow, with **operational notes** for a sole developer/release manager (approvals, CODEOWNERS, status check display names).

---

## 1. Repository settings (verified)

| Item | Value |
|------|--------|
| Visibility | `public` |
| Default branch | `main` |
| Squash merge | Enabled |
| Merge commit | Enabled |
| Rebase merge | Enabled |
| Delete branch on merge | Disabled (global) |

*Source: `gh api repos/oussasz/maintafox-desktop`.*

---

## 2. Branch protection — as deployed

### 2.1 `main`

| Policy | Documented (`.github/branch-protection.md`) | As deployed |
|--------|---------------------------------------------|-------------|
| Require PR before merge | Yes | Yes (implicit via protection) |
| Approvals required | 2 | **2** |
| Dismiss stale reviews | Yes | **Yes** |
| Require CODEOWNERS review | When present | **Yes** (see §5) |
| Require branch up to date | Yes | **Yes** (`strict: true`) |
| Required status checks | `lint-and-format`, `rust-quality`, `frontend-tests`, `tauri-build-check` | **Lint and Format**, **Rust Quality**, **Frontend Tests**, **Tauri Build Check** (see §4) |
| Signed commits | Yes | **Yes** |
| Force pushes | No | **Disabled** |
| Branch deletion | No | **Disabled** |
| Enforce admins | — | **Enabled** |

### 2.2 `staging`

| Policy | Documented | As deployed |
|--------|------------|-------------|
| Approvals required | 1 | **1** |
| Require branch up to date | Yes | **Yes** (`strict: true`) |
| Required checks | lint, rust, tauri build | **Lint and Format**, **Rust Quality**, **Tauri Build Check** |
| Force pushes / deletion | No | **Disabled** |
| Enforce admins | — | **Enabled** |

### 2.3 `develop`

| Policy | Documented | As deployed |
|--------|------------|-------------|
| Approvals required | 1 | **1** |
| Require branch up to date | No | **No** (`strict: false`) |
| Required checks | lint, rust | **Lint and Format**, **Rust Quality** |
| Force pushes / deletion | No | **Disabled** |
| Enforce admins | — | **Enabled** |

*Source: `gh api repos/oussasz/maintafox-desktop/branches/{main,staging,develop}/protection`.*

---

## 3. CI workflow alignment

| Workflow | State | Notes |
|----------|--------|--------|
| **CI** (`ci.yml`) | Active | Runs on `push`/`pull_request` to `develop`, `staging`, `main`; weekly security audit schedule |
| **Promote to Staging** | Active | Manual promotion workflow per documentation |
| **Startup Time Gate** | Active | Additional quality gate |

Job `name` fields in `ci.yml` match the **required check contexts** configured in branch protection (e.g. `lint-and-format` job → display name **Lint and Format**).

---

## 4. Status check naming (documentation vs GitHub UI)

`.github/branch-protection.md` lists checks as `ci / lint-and-format` (workflow slug + job id). GitHub’s required-status API stores the **check run name** (the job’s `name:` field), e.g. **Lint and Format**. Functionally they refer to the same CI jobs; only the **label** differs from the older reference table.

**Recommendation:** Update `.github/branch-protection.md` in a follow-up chore to list both the workflow job id and the check run title to avoid confusion.

---

## 5. CODEOWNERS and code owner reviews

- **CODEOWNERS file:** Not present in the repository at audit time.
- **Branch protection:** `main` has **Require code owner reviews** enabled.

GitHub behavior: if no `CODEOWNERS` file exists, code owner rules may not resolve to specific reviewers. **Action:** Either add a `CODEOWNERS` file mapping critical paths (e.g. `src-tauri/`, `.github/`) to the release manager, or turn off “Require code owner reviews” on `main` until owners are defined—whichever matches your governance intent.

---

## 6. Sole developer / sole release manager

**Documented `main` policy:** two approving reviews.

**Operational reality:** A single human cannot supply two distinct approvals on GitHub. Typical mitigations:

1. **Temporary:** Reduce `main` required approvals to **1** while the team is a single maintainer (matches your statement that PR-based constraints are sufficient for now).
2. **Process:** Use a second trusted reviewer account only when policy requires two humans.
3. **Emergency:** Repository admin merge or adjust protection (documented, rare).

**Recommendation:** Record the chosen policy in `docs/BRANCHING_STRATEGY.md` or this report’s appendix when you finalize approval count for `main`.

---

## 7. Governance branches — current SHAs (audit snapshot)

Captured from local `origin` refs at audit time:

| Branch | Commit (short) | Role |
|--------|----------------|------|
| `main` | `3602799…` | Release line |
| `staging` | `9a68ef8…` | Pre-release integration |
| `develop` | `0e9708d…` | Active integration |

Re-run: `git fetch --all --prune` and `git rev-parse origin/main origin/staging origin/develop` for an updated snapshot.

---

## 8. Promotion history (repair window)

The following pull requests re-established a continuous promotion path consistent with `docs/BRANCHING_STRATEGY.md`:

| PR | Flow | Purpose |
|----|------|---------|
| #9 | `main` → `develop` | Sync integration branch with release line |
| #8 | `develop` → `staging` | Staging promotion |
| #10 | `staging` → `main` | Release promotion |

Redundant or superseded repair PRs were closed where applicable. Legacy commits previously reachable only via `master` are retained on **`archive/master-legacy`** for audit continuity.

---

## 9. Items not automated (documentation vs platform)

| Item | Status |
|------|--------|
| Restrict direct pushes to `release-manager` role only | **Not set via API**; relies on GitHub Teams/org roles. Optional follow-up in repo **Settings → Branches** if you use org teams. |
| GPG-signed annotated tags on `main` | **Process**; verify with `git tag -v` on release tags. |
| 30-day retention of merged feature branches | **Process**; not a branch protection rule. |

---

## 10. Compliance checklist

| Requirement | Status |
|-------------|--------|
| Default branch `main` | Pass |
| `develop` / `staging` / `main` exist | Pass |
| PR required for protected branches | Pass |
| CI checks enforced per branch tier | Pass |
| No force-push / no delete on protected branches | Pass |
| Signed commits on `main` | Pass |
| Documented merge-back rules achievable via PRs | Pass |
| Operational clarity for sole maintainer (2 approvals on `main`) | **Review** — see §6 |
| CODEOWNERS vs code-owner requirement | **Review** — see §5 |

---

## 11. Sign-off

This report is a **point-in-time compliance snapshot**. Re-verify after any change to branch protection, CI job names, or default branch.

**Maintafox Desktop — Release Control Compliance**  
Generated as part of the Phase 4 release-control hardening initiative.

---

## Appendix A — Document delivery (this file)

This report is proposed to `main` via pull request so it respects protected-branch rules (PR required, **signed commits** on `main`, required checks).  
**PR:** https://github.com/oussasz/maintafox-desktop/pull/11  

If required checks fail for reasons unrelated to this documentation change (for example `cargo audit` advisories on transitive dependencies), treat that as a **separate remediation track**; the release-control *policy* snapshot in this report remains valid as of the audit date.

---

## Appendix B — Sole maintainer merge path

`main` is configured for **two** approving reviews. As the sole developer and release manager, you may:

- Use **one** approval plus a second reviewer when available, or  
- Temporarily **lower required approvals to 1** on `main` in branch protection settings, or  
- Merge with **administrator** privileges only when policy allows and you document the exception.

**Signed commits:** Pushes to `main` require verified signatures. Configure GPG or SSH commit signing locally, or use GitHub’s merge options that satisfy your org’s signing rules.
