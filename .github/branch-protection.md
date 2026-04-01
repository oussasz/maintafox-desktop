# GitHub Branch Protection — Configuration Reference

> This file documents the exact settings to apply manually on GitHub
> (Settings → Branches → Add branch protection rule) for each protected branch.
>
> **This is a reference document, not an executable configuration.**
> A project administrator must apply these rules through the GitHub UI.

---

## Branch: `main`

| Setting | Value |
|---------|-------|
| Require a pull request before merging | **YES** |
| Required number of approvals before merging | **2** |
| Dismiss stale pull request approvals when new commits are pushed | **YES** |
| Require review from Code Owners | **YES** (when CODEOWNERS file is in place) |
| Require branches to be up to date before merging | **YES** |
| Restrict who can push directly to this branch | **release-manager** role only |
| Require signed commits | **YES** |
| Allow force pushes | **NO** |
| Allow deletions | **NO** |

**Required status checks:**

- `ci / lint-and-format`
- `ci / rust-quality`
- `ci / frontend-tests`
- `ci / tauri-build-check`

---

## Branch: `staging`

| Setting | Value |
|---------|-------|
| Require a pull request before merging | **YES** |
| Required number of approvals before merging | **1** |
| Require branches to be up to date before merging | **YES** |
| Allow force pushes | **NO** |
| Allow deletions | **NO** |

**Required status checks:**

- `ci / lint-and-format`
- `ci / rust-quality`
- `ci / tauri-build-check`

---

## Branch: `develop`

| Setting | Value |
|---------|-------|
| Require a pull request before merging | **YES** |
| Required number of approvals before merging | **1** |
| Require branches to be up to date before merging | **NO** (to reduce friction on active integration) |
| Allow force pushes | **NO** |
| Allow deletions | **NO** |

**Required status checks:**

- `ci / lint-and-format`
- `ci / rust-quality`

---

## Notes

- Status check names correspond to the job names in `.github/workflows/ci.yml`.
- These rules will be enforced once the CI workflow (File 02 · Sprint S3) is committed
  and the first workflow run completes on each branch.
- The `CODEOWNERS` file will be created in a later sprint; until then, the Code Owners
  requirement on `main` will have no effect.
