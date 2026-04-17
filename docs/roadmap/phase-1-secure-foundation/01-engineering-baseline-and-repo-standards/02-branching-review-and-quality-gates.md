# Phase 1 · Sub-phase 01 · File 02
# Branching, Review, and Quality Gates

## Context and Purpose

This file defines the version control strategy, pull request workflow, code review
standards, and the automated CI quality gate pipeline that every subsequent sprint must
pass before its output is merged. These agreements apply to all AI coding agents and to the
supervisor from Phase 1 onward.

The branching model creates a safe promotion path:
`feature branch → develop → staging (supervisor acceptance) → main (release)`.
The CI pipeline enforces that no code reaches `develop` without passing lint, type check,
Rust quality, frontend tests, and a Tauri build validation.

## Prerequisites

- File 01 of this sub-phase completed (monorepo scaffold, coding standards in place)
- GitHub repository created and remote origin configured
- pnpm workspace and Rust workspace building cleanly

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Git Branch Strategy | Branch model doc, protection config reference, branch creation scripts |
| S2 | PR Templates and Review Checklists | PR template, issue templates, label set, code review guide |
| S3 | CI Quality Gates | GitHub Actions workflow, Vitest setup, security audit job |

---

## Sprint S1 — Git Branch Strategy and Branch Protection

### AI Agent Prompt

You are establishing the complete Git branching strategy and repository protection model
for the Maintafox Desktop project. Your output is a set of documentation files and
developer utility scripts. Branch protection rules themselves must be applied manually on
GitHub using the reference document you create — GitHub API calls or repository settings
changes are not part of this sprint.

---

**Step 1 — Define the branch model.**

Create `docs/BRANCHING_STRATEGY.md` with the following content:

**Section 1 — Branch Model Diagram**
```
main ─────────────────────────────────────── production-grade, signed releases
  │
  └─ staging ──────────────────────────────── supervisor acceptance + integration
       │
       └─ develop ───────────────────────── active integration (all feature merges)
            │
            ├─ feature/p{N}-sp{NN}-{slug} ── sprint feature branches
            ├─ fix/{id}-{slug} ────────────── targeted bug fix branches
            └─ chore/{slug} ───────────────── non-functional changes
```
Hotfix branches follow a separate path:
```
main ── hotfix/{version}-{slug} ─── merged back to both main AND develop
```

**Section 2 — When to Create Each Branch Type**
- `feature/*`: every sprint's implementation work; one branch per sprint file (e.g.,
  `feature/p1-sp01-f01-scaffold`)
- `fix/*`: a defect found after the sprint is closed; references the GitHub issue number
  (e.g., `fix/MAF-42-auth-offline-grace`)
- `hotfix/*`: emergency fix needed on a released version; branches from the release tag on
  `main`, not from `develop`
- `chore/*`: dependency updates, config changes, documentation improvements that touch no
  application logic

**Section 3 — Merge-Back Rules**
- `feature/*` → `develop` (via PR, 1 approval required)
- `fix/*` → `develop` (via PR, 1 approval required)
- `develop` → `staging` (via manual promotion workflow or PR, CI must pass)
- `staging` → `main` (via PR, 2 approvals required, all CI gates must pass)
- `hotfix/*` → `main` AND `hotfix/*` → `develop` (separate PRs; applied to both)
- Direct commits to `main`, `staging`, or `develop` are forbidden

**Section 4 — Hotfix Procedure**
1. Create branch from the release tag: `git checkout -b hotfix/1.2.1-fix-slug v1.2.0`
2. Implement the minimal fix
3. Open PR to `main`; get 2 approvals; merge
4. Tag `main` with the patch version: `git tag -a v1.2.1 -m "Hotfix: description"`
5. Open a second PR from the same branch to `develop` to prevent regression
6. Close the hotfix branch after both merges

**Section 5 — Tagging Convention**
- Tags are created only on `main`
- Format: `v{MAJOR}.{MINOR}.{PATCH}` for releases, `v{X}.{Y}.{Z}-beta.{N}` for pilots
- Tags must be GPG-signed annotated tags:
  `git tag -a v1.0.0 -m "Maintafox 1.0.0 — Production release"`
- No tag may be created on `staging` or `develop`
- Tags are pushed separately: `git push origin --tags`

**Section 6 — Forbidden Actions**
- Force-push to `main`, `staging`, or `develop` — never permitted under any circumstance
- Committing directly to `main` — all changes arrive via reviewed PR
- Deleting merged feature branches before 30 days — they serve as the sprint audit trail
- Amending or rebasing pushed commits on shared branches
- Bypassing CI status checks to merge a PR (the CI gate is not optional)

---

**Step 2 — Write the GitHub branch protection reference.**

Create `.github/branch-protection.md`. This file describes the exact settings that must be
manually configured on GitHub (Settings → Branches) for `main`, `staging`, and `develop`.

**For `main`:**
- Require a pull request before merging: YES
- Required number of approvals before merging: 2
- Dismiss stale pull request approvals when new commits are pushed: YES
- Require review from Code Owners: YES (when CODEOWNERS file is in place)
- Required status checks that must pass:
  - `ci / lint-and-format`
  - `ci / rust-quality`
  - `ci / frontend-tests`
  - `ci / tauri-build-check`
- Require branches to be up to date before merging: YES
- Restrict who can push directly to this branch: release-manager role only
- Require signed commits: YES
- Allow force pushes: NO
- Allow deletions: NO

**For `staging`:**
- Require a pull request before merging: YES
- Required number of approvals: 1
- Required status checks:
  - `ci / lint-and-format`
  - `ci / rust-quality`
  - `ci / tauri-build-check`
- Require branches to be up to date: YES
- Allow force pushes: NO
- Allow deletions: NO

**For `develop`:**
- Require a pull request before merging: YES
- Required number of approvals: 1
- Required status checks:
  - `ci / lint-and-format`
  - `ci / rust-quality`
- Require branches to be up to date: NO (to reduce friction on active integration)
- Allow force pushes: NO
- Allow deletions: NO

---

**Step 3 — Write branch creation scripts.**

Create `scripts/new-branch.ps1` (Windows PowerShell):
```powershell
<#
.SYNOPSIS
  Creates a new Maintafox feature, fix, hotfix, or chore branch from the correct base.
.EXAMPLE
  .\scripts\new-branch.ps1 -Type feature -Slug p1-sp01-scaffold
  .\scripts\new-branch.ps1 -Type fix -Slug MAF-42-auth-offline-grace
#>
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("feature", "fix", "hotfix", "chore")]
    [string]$Type,

    [Parameter(Mandatory=$true)]
    [string]$Slug
)

# Validate slug is lowercase kebab-case
if ($Slug -notmatch '^[a-z0-9][a-z0-9\-]*[a-z0-9]$') {
    Write-Error "Slug must be lowercase kebab-case (e.g., p1-sp01-scaffold). Received: '$Slug'"
    exit 1
}

$BranchName = "$Type/$Slug"

if ($Type -eq "hotfix") {
    Write-Host "Switching to main and pulling latest..."
    git checkout main
    git pull origin main
} else {
    Write-Host "Switching to develop and pulling latest..."
    git checkout develop
    git pull origin develop
}

Write-Host "Creating branch: $BranchName"
git checkout -b $BranchName

Write-Host ""
Write-Host "Branch created: $BranchName" -ForegroundColor Green
Write-Host "When your work is ready, open a PR targeting:"
if ($Type -eq "hotfix") {
    Write-Host "  main (then a second PR to develop)" -ForegroundColor Yellow
} else {
    Write-Host "  develop" -ForegroundColor Yellow
}
```

Create `scripts/new-branch.sh` (Unix/macOS):
```bash
#!/usr/bin/env bash
set -euo pipefail

TYPE="${1:-}"
SLUG="${2:-}"

if [[ -z "$TYPE" || -z "$SLUG" ]]; then
  echo "Usage: ./scripts/new-branch.sh <type> <slug>"
  echo "  type: feature | fix | hotfix | chore"
  echo "  slug: lowercase-kebab-case"
  exit 1
fi

if [[ ! "$TYPE" =~ ^(feature|fix|hotfix|chore)$ ]]; then
  echo "Error: type must be one of: feature, fix, hotfix, chore"
  exit 1
fi

if [[ ! "$SLUG" =~ ^[a-z0-9][a-z0-9\-]*[a-z0-9]$ ]]; then
  echo "Error: slug must be lowercase kebab-case. Received: '$SLUG'"
  exit 1
fi

BRANCH_NAME="$TYPE/$SLUG"

if [[ "$TYPE" == "hotfix" ]]; then
  echo "Switching to main and pulling latest..."
  git checkout main && git pull origin main
else
  echo "Switching to develop and pulling latest..."
  git checkout develop && git pull origin develop
fi

echo "Creating branch: $BRANCH_NAME"
git checkout -b "$BRANCH_NAME"

echo ""
echo "Branch created: $BRANCH_NAME"
```

---

**Acceptance criteria:**
- `docs/BRANCHING_STRATEGY.md` is present with all 6 sections and the ASCII branch diagram
- `.github/branch-protection.md` specifies rules for `main`, `staging`, and `develop`
- `scripts/new-branch.ps1` runs on Windows without errors when given valid arguments
- `scripts/new-branch.sh` is present with executable permissions (`chmod +x`)

---

### Supervisor Verification — Sprint S1

**V1 — Branching strategy document is complete.**
In Explorer, open `docs/BRANCHING_STRATEGY.md`. The document should show a visual diagram
of the branch structure (lines with arrows) and contain 6 titled sections including
"Hotfix Procedure" and "Forbidden Actions". If the document is empty or missing more than
one section, flag it.

**V2 — Branch protection reference exists.**
Open `.github/branch-protection.md`. Confirm it describes protection settings for `main`,
`staging`, and `develop`. The `main` section should explicitly list required status checks
and require signed commits. If the file is absent or only describes `main`, flag it.

**V3 — Branch creation script works on Windows.**
In the PowerShell terminal, run:
```powershell
.\scripts\new-branch.ps1 -Type chore -Slug test-branch-init
```
The script should print "Branch created: chore/test-branch-init" in green text. If the
script throws a red error, copy the message and flag it. After confirming success, delete
the test branch:
```
git checkout develop
git branch -d chore/test-branch-init
```

**V4 — GitHub branch protection (manual action required).**
Go to the GitHub repository → Settings → Branches. For each of the three branches (`main`,
`staging`, `develop`), create a branch protection rule following `.github/branch-protection.md`.
This is a manual task for the team lead or project administrator. Mark this item as
"pending manual setup" if GitHub access is not yet available.

---

## Sprint S2 — Pull Request Templates and Review Checklists

### AI Agent Prompt

You are creating the complete pull request process documentation for the Maintafox Desktop
project. Every PR — whether opened by an AI coding agent or a human developer — must follow
this template. The maintenance engineer supervisor must be able to read each PR description
and understand what changed without reading any code.

---

**Step 1 — Create the PR template.**

Create `.github/PULL_REQUEST_TEMPLATE.md`:
```markdown
## Summary

<!-- What does this PR do? Write 2–3 sentences in plain language. Assume the reader
     is not a programmer — describe the observable outcome, not the code changed. -->

## Phase / Subphase / Sprint Reference

<!-- e.g., Phase 1 · Sub-phase 01 · File 02 · Sprint S2 -->

## Changes Made

<!-- Bullet list of specific things that were created, modified, or removed.
     Be concrete: file names, features added, behaviors changed. -->

-

## Supervisor Verification Checklist

<!-- Copy the Supervisor Verification steps from the roadmap sprint file and mark
     each one as PASS, FAIL, or NOT APPLICABLE. -->

| Step | Result | Notes |
|------|--------|-------|
| V1   |        |       |
| V2   |        |       |
| V3   |        |       |
| V4   |        |       |

## Supervisor Acceptance

*To be completed by the maintenance engineer after running the verification steps above.*

- [ ] Verified on local machine — all verification steps run
- [ ] User-facing behavior is correct (if applicable to this sprint)
- [ ] No regressions observed in previously working features
- [ ] Accepted for merge to `develop`

## Breaking Changes

<!-- Does this PR break anything that was working before this change? YES / NO.
     If YES, describe exactly what breaks and what the migration path is. -->

## New Dependencies Added

<!-- List any new npm packages or Rust crates added in this PR.
     For each, note: name, version, purpose, and result of security audit. -->

## Security Notes

<!-- Does this PR touch authentication, session management, permissions, IPC commands,
     or file system access? If YES, describe the security consideration. -->

## Definition of Done Checklist

- [ ] CI passes: lint, typecheck, Rust quality, tests, and build check all green
- [ ] No `TODO` / `FIXME` left without a linked GitHub issue number
- [ ] All user-visible strings use `t()` translation function
- [ ] No hardcoded French or English text outside `src/i18n/`
- [ ] No `unwrap()` / `expect()` in Rust without an explanatory inline comment
- [ ] No secrets or credentials in any file in this PR
- [ ] New IPC commands added to `docs/IPC_COMMAND_REGISTRY.md`
- [ ] `CHANGELOG.md` `[Unreleased]` section updated
- [ ] PR title follows format: `[P{N}-SP{NN}-F{NN}-S{N}] Description`
```

---

**Step 2 — Create issue templates.**

Create `.github/ISSUE_TEMPLATE/bug_report.md`:
```markdown
---
name: Bug report
about: Report a malfunction observed during supervisor testing or normal use
title: "[BUG] "
labels: type:bug, status:in-progress
assignees: ''
---

## Module Affected

<!-- Which module or screen did the bug occur in? e.g., Work Orders, Authentication, Equipment Registry -->

## Steps to Reproduce

<!-- Number each step. Write in plain language. Assume the reader will follow these steps exactly. -->

1.
2.
3.

## What You Expected to See

<!-- Describe the correct behavior in one or two sentences. -->

## What Actually Happened

<!-- Describe the broken behavior. Copy any error messages exactly as they appeared. -->

## Phase / Sprint Context

<!-- Which sprint file was being tested when this bug was found? -->

## Screenshot or Video

<!-- Optional but very helpful. Drag and drop an image or screen recording here. -->

## Environment

- OS:
- Maintafox version:
- Last working sprint (if known):
```

Create `.github/ISSUE_TEMPLATE/supervisor_feedback.md`:
```markdown
---
name: Supervisor Feedback
about: Log acceptance feedback from the maintenance engineer after testing a sprint
title: "[FEEDBACK] P?-SP??-F??-S? — "
labels: status:awaiting-supervisor
assignees: ''
---

## Sprint Reference

<!-- Phase / Subphase / File / Sprint — e.g., P1-SP01-F02-S2 -->

## Verification Result

<!-- Choose one: PASS / PARTIAL PASS / FAIL -->

## Verification Steps Completed

| Step | Result | Notes |
|------|--------|-------|
| V1   |        |       |
| V2   |        |       |
| V3   |        |       |
| V4   |        |       |

## Items Flagged

<!-- Describe any step that failed or produced unexpected results.
     Be specific: what you did, what you saw, what you expected. -->

## Suggested Improvement

<!-- Optional. If something worked but could be better, note it here. -->

## Blocking?

<!-- Is any flagged item a blocker that prevents the next sprint from starting?
     YES (blocking) / NO (non-blocking, can continue) -->

## Approved to Continue?

- [ ] Yes — all blockers resolved, next sprint may begin
- [ ] No — blockers must be addressed before proceeding
```

---

**Step 3 — Define the GitHub label set.**

Create `.github/labels.yml`. GitHub CLI (`gh label create`) or the repository Labels page
can import from this list. Include the following labels:

```yaml
# Phase labels
- name: "phase:1"
  color: "0075ca"
  description: "Phase 1 — Secure Foundation"
- name: "phase:2"
  color: "0075ca"
  description: "Phase 2 — Core Execution Backbone"
- name: "phase:3"
  color: "0075ca"
  description: "Phase 3 — Planning, Compliance, and Material Control"
- name: "phase:4"
  color: "0075ca"
  description: "Phase 4 — Control Plane and Integrations"
- name: "phase:5"
  color: "0075ca"
  description: "Phase 5 — Advanced Reliability and Launch Hardening"

# Type labels
- name: "type:feature"
  color: "0e8a16"
  description: "New feature or sprint implementation"
- name: "type:bug"
  color: "d73a4a"
  description: "Something isn't working"
- name: "type:chore"
  color: "e4e669"
  description: "Maintenance, dependencies, config"
- name: "type:security"
  color: "b60205"
  description: "Security-related issue or improvement"
- name: "type:performance"
  color: "d93f0b"
  description: "Performance improvement"
- name: "type:docs"
  color: "0075ca"
  description: "Documentation only"

# Status labels
- name: "status:in-progress"
  color: "fbca04"
  description: "Actively being worked on"
- name: "status:blocked"
  color: "b60205"
  description: "Cannot proceed — dependency or blocker"
- name: "status:awaiting-supervisor"
  color: "c5def5"
  description: "Waiting for supervisor acceptance testing"
- name: "status:accepted"
  color: "0e8a16"
  description: "Accepted by supervisor, ready to merge"

# Priority labels
- name: "priority:critical"
  color: "b60205"
  description: "Must be resolved before next sprint starts"
- name: "priority:high"
  color: "d93f0b"
  description: "Important, address this sprint"
- name: "priority:normal"
  color: "fbca04"
  description: "Standard priority"
- name: "priority:low"
  color: "e4e669"
  description: "Nice to have, address when capacity allows"

# Sprint planning labels
- name: "sprint:current"
  color: "0e8a16"
  description: "In scope for the current sprint"
- name: "sprint:next"
  color: "bfd4f2"
  description: "Queued for the next sprint"
- name: "sprint:backlog"
  color: "e4e669"
  description: "In backlog — not yet scheduled"
```

---

**Step 4 — Write the code review guide.**

Create `docs/CODE_REVIEW_GUIDE.md` with these five sections:

**Section 1 — Review Roles**
- **AI coding agent**: writes the code, runs all CI checks locally, fills in the PR
  template completely before requesting review, labels the PR correctly
- **Maintenance engineer supervisor**: reviews the Supervisor Verification section of the
  roadmap file and tests the observable output; does not review code syntax
- **Senior reviewer (optional)**: reviews architectural changes and additions to `src-tauri/`,
  new IPC commands, database schema changes, and permission model changes; required for merges
  to `main`

**Section 2 — Review Criteria Checklist (25 items)**
The reviewer checks that the PR:
1. Has a clear plain-language summary in the PR description
2. Lists all files changed in the "Changes Made" section
3. Has a filled-in Supervisor Verification table
4. Has the Supervisor Acceptance section signed off by the engineer
5. CI is fully green (all 4+ jobs passing)
6. No TypeScript strict suppression (`@ts-ignore`) without a linked issue
7. No `any` type without explicit justification in a comment
8. No direct `invoke()` call outside `src/services/`
9. All user-visible strings wrapped in `t()`
10. No hardcoded French or English text in component files
11. No Rust `unwrap()` or `expect()` without an inline safety comment
12. Error variants use `AppError` consistently — no raw `anyhow` bubbling to IPC
13. New database columns have explicit `NOT NULL` or default constraints
14. New Tauri IPC commands are in `IPC_COMMAND_REGISTRY.md`
15. New npm or crate dependencies are listed in the PR with a security audit note
16. No secrets, tokens, or credentials in any file
17. No `.env` or `.env.local` files in the changeset
18. Changes to `tauri.conf.json` capabilities are explained in the Security Notes section
19. `CHANGELOG.md` [Unreleased] section has a relevant entry
20. Migration files (if any) are additive and have a rollback path noted
21. No commented-out code blocks left in the PR
22. No `console.log` in production-path React code
23. No `println!` in Rust production-path code
24. Branch follows the naming convention from `BRANCHING_STRATEGY.md`
25. PR title follows `[P{N}-SP{NN}-F{NN}-S{N}] Description` format

**Section 3 — How to Leave Effective Feedback**
- Be specific: reference the file path and approximate line number in comments
- Use plain language: assume a non-programmer may read the review thread
- Distinguish blocking from non-blocking: prefix blocking comments with `[BLOCKING]`
- Do not leave vague comments such as "this could be better" — describe what should change
  and why

**Section 4 — Response Time Expectations**
- First review response: within 24 hours of PR being opened
- Re-review after changes: within 8 hours of the "ready for re-review" comment
- Supervisor acceptance testing: within 48 hours of the "Sprint output complete" comment

**Section 5 — Merge Authority**
- `feature/*` → `develop`: any approved reviewer can click Merge
- `develop` → `staging`: any approved reviewer; must confirm CI green
- `staging` → `main`: designated `release-manager` account only; 2 approvals required;
  must verify that the `CHANGELOG.md` [Unreleased] section is non-empty and that a
  version bump has been run

---

**Acceptance criteria:**
- `.github/PULL_REQUEST_TEMPLATE.md` is present with all required sections including the
  Supervisor Acceptance block
- Both issue templates exist in `.github/ISSUE_TEMPLATE/`
- `.github/labels.yml` defines at least 20 labels across all categories
- `docs/CODE_REVIEW_GUIDE.md` contains all 5 sections and the 25-item checklist

---

### Supervisor Verification — Sprint S2

**V1 — PR template has supervisor section.**
Navigate to `.github/PULL_REQUEST_TEMPLATE.md` in Explorer. Open it and scroll to find a
section titled "Supervisor Acceptance". It should contain checkboxes including "Verified on
local machine" and "Accepted for merge to develop". If this section is missing or the
checkboxes are absent, flag it.

**V2 — Supervisor feedback template is usable.**
Open `.github/ISSUE_TEMPLATE/supervisor_feedback.md`. Confirm it contains a table for
logging verification step results (V1, V2, V3, V4 rows) and a section for "Approval to
Continue". This is the form you will fill in after each sprint acceptance. If the table is
missing or the approval checkbox is absent, flag it.

**V3 — Code review guide contains the 25-item checklist.**
Open `docs/CODE_REVIEW_GUIDE.md`. Find the section titled "Review Criteria Checklist".
Count the numbered items — there should be 25. If any of the following are missing, flag
by name: "No secrets or credentials in any file", "All user-visible strings wrapped in
`t()`", "New Tauri IPC commands are in IPC_COMMAND_REGISTRY.md".

**V4 — Test the PR process on GitHub (action required).**
On the GitHub repository, click "Pull requests" → "New pull request". Select `develop` as
base and any recent feature branch as compare. When the description box appears, it should
automatically contain the full PR template (the sections and checkboxes). If the box is
empty instead, the template is not registered — flag it.

---

## Sprint S3 — CI Quality Gates

### AI Agent Prompt

You are building the complete automated CI quality gate pipeline for the Maintafox Desktop
project. This pipeline runs on every pull request and on every push to `develop`, `staging`,
and `main`. It must catch every class of quality regression before code is merged. The
pipeline uses GitHub Actions.

---

**Step 1 — Create the main CI workflow.**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [develop, staging, main]
  pull_request:
    branches: [develop, staging, main]

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  RUST_BACKTRACE: 1
  CARGO_TERM_COLOR: always
  SQLX_OFFLINE: "true"

jobs:
  # ─────────────────────────────────────────────────────────────────────────────
  # Job 1: Frontend lint, format, and type check
  # ─────────────────────────────────────────────────────────────────────────────
  lint-and-format:
    name: Lint and Format
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: "pnpm"

      - name: Install dependencies
        run: pnpm install --frozen-lockfile

      - name: TypeScript type check
        run: pnpm run typecheck

      - name: ESLint
        run: pnpm run lint:check

      - name: Prettier format check
        run: pnpm run format:check

      - name: Check CHANGELOG has Unreleased entry (PRs to main only)
        if: github.base_ref == 'main'
        run: |
          if ! grep -qA 5 "## \[Unreleased\]" CHANGELOG.md | grep -q "^- \|^### "; then
            echo "ERROR: CHANGELOG.md must have at least one entry under [Unreleased] before merging to main."
            exit 1
          fi

  # ─────────────────────────────────────────────────────────────────────────────
  # Job 2: Rust lint, format, and tests
  # ─────────────────────────────────────────────────────────────────────────────
  rust-quality:
    name: Rust Quality
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache Cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            src-tauri/target
          key: cargo-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: cargo-${{ runner.os }}-

      - name: Install system dependencies (for sea-orm sqlite)
        run: sudo apt-get install -y libsqlite3-dev

      - name: Rust format check
        working-directory: src-tauri
        run: cargo fmt --check

      - name: Clippy — deny warnings
        working-directory: src-tauri
        run: cargo clippy -- -D warnings

      - name: Rust unit tests
        working-directory: src-tauri
        run: cargo test

  # ─────────────────────────────────────────────────────────────────────────────
  # Job 3: Frontend tests with coverage
  # ─────────────────────────────────────────────────────────────────────────────
  frontend-tests:
    name: Frontend Tests
    runs-on: ubuntu-latest
    needs: lint-and-format
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: "pnpm"

      - name: Install dependencies
        run: pnpm install --frozen-lockfile

      - name: Run Vitest with coverage
        run: pnpm run test --coverage

      - name: Upload coverage report
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: coverage-report
          path: coverage/

  # ─────────────────────────────────────────────────────────────────────────────
  # Job 4: Tauri build validation (Windows MSVC target)
  # ─────────────────────────────────────────────────────────────────────────────
  tauri-build-check:
    name: Tauri Build Check
    runs-on: windows-latest
    needs: rust-quality
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust stable (MSVC)
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Cache Cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            src-tauri/target
          key: cargo-win-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: cargo-win-

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: "pnpm"

      - name: Install dependencies
        run: pnpm install --frozen-lockfile

      - name: Build frontend
        run: pnpm run build

      - name: Validate Tauri build (no bundle)
        run: pnpm tauri build --ci --no-bundle
        env:
          TAURI_SIGNING_PRIVATE_KEY: ""
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ""

  # ─────────────────────────────────────────────────────────────────────────────
  # Job 5: Security audit (runs on PRs to main/staging and on weekly schedule)
  # ─────────────────────────────────────────────────────────────────────────────
  security-audit:
    name: Security Audit
    runs-on: ubuntu-latest
    if: >
      github.event_name == 'schedule' ||
      (github.event_name == 'pull_request' &&
       (github.base_ref == 'main' || github.base_ref == 'staging'))
    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-audit
        run: cargo install cargo-audit --locked

      - name: Rust dependency audit
        working-directory: src-tauri
        run: cargo audit --deny warnings

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: "pnpm"

      - name: Install dependencies
        run: pnpm install --frozen-lockfile

      - name: npm dependency audit
        run: pnpm audit --audit-level=high

      - name: Secret scanning with Gitleaks
        uses: gitleaks/gitleaks-action@v2
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GITLEAKS_LICENSE: ${{ secrets.GITLEAKS_LICENSE }}

on_schedule:
  - cron: "0 6 * * 1"
```

> **Note:** The `on_schedule` block above is illustrative. The correct placement is at the
> top-level `on:` block. Adjust the `ci.yml` so the weekly schedule for `security-audit` is
> registered at the top-level trigger, not as a separate block. Full final YAML:
> ```yaml
> on:
>   push:
>     branches: [develop, staging, main]
>   pull_request:
>     branches: [develop, staging, main]
>   schedule:
>     - cron: "0 6 * * 1"   # Every Monday at 06:00 UTC
> ```

---

**Step 2 — Create the staging promotion workflow.**

Create `.github/workflows/staging-promote.yml`:
```yaml
name: Promote to Staging

on:
  workflow_dispatch:
    inputs:
      source_branch:
        description: "Branch to promote (default: develop)"
        required: false
        default: "develop"
      release_notes:
        description: "Brief note about what this promotion includes"
        required: true

jobs:
  promote:
    name: Promote ${{ inputs.source_branch }} to staging
    runs-on: ubuntu-latest
    environment: staging
    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: Validate CI status
        run: |
          echo "Promoting: ${{ inputs.source_branch }} → staging"
          echo "Release notes: ${{ inputs.release_notes }}"

      - name: Create PR from source to staging
        run: |
          gh pr create \
            --base staging \
            --head "${{ inputs.source_branch }}" \
            --title "Promote: ${{ inputs.source_branch }} → staging" \
            --body "## Staging Promotion

          **Source:** ${{ inputs.source_branch }}
          **Triggered by:** ${{ github.actor }}
          **Release notes:** ${{ inputs.release_notes }}

          This PR was auto-created by the staging promotion workflow.
          All checks must pass before merging." \
            --label "status:in-progress"
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

---

**Step 3 — Create `.gitleaks.toml`.**

```toml
[allowlist]
description = "Known safe patterns in this repository"
paths = [
  ".env.example",
  "docs/SECRETS_MANAGEMENT.md",
  "docs/CODE_REVIEW_GUIDE.md",
]
regexes = [
  '''TAURI_SIGNING_PRIVATE_KEY=$''',
  '''# DO NOT set TAURI_SIGNING''',
]
```

---

**Step 4 — Ensure `src/test/setup.ts` is complete.**

Verify the file from Sprint S1 is in place and contains the Tauri `invoke` mock. If not,
write it now:
```typescript
import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mock the Tauri IPC runtime.
// Tests do not have access to the Tauri binary; this prevents test failures from missing
// runtime while allowing service-layer tests to assert on invoke call arguments.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));
```

---

**Acceptance criteria:**
- `.github/workflows/ci.yml` is valid YAML with 5 jobs: `lint-and-format`, `rust-quality`,
  `frontend-tests`, `tauri-build-check`, `security-audit`
- `.github/workflows/staging-promote.yml` is present with `workflow_dispatch` trigger
- `.gitleaks.toml` is present with the `.env.example` allowlist
- `src/test/setup.ts` is present with the Tauri `invoke` mock
- After pushing to `develop`, all 4 main CI jobs appear in GitHub Actions and complete

---

### Supervisor Verification — Sprint S3

**V1 — CI workflow file has all required jobs.**
In Explorer, navigate to `.github/workflows/` and open `ci.yml`. Scroll through it and
confirm you can see five job names:
- `lint-and-format`
- `rust-quality`
- `frontend-tests`
- `tauri-build-check`
- `security-audit`

If any job name is missing, flag it.

**V2 — Staging promotion workflow is present.**
Confirm `.github/workflows/staging-promote.yml` exists. Open it and confirm the first few
lines contain `workflow_dispatch`. This means the promotion can be triggered manually from
the GitHub Actions tab. Flag if absent.

**V3 — Secret scanning config is present.**
Open `.gitleaks.toml`. It should mention `.env.example` in an `allowlist` section. Flag if
the file is absent.

**V4 — CI runs on GitHub (action required — needs GitHub access).**
After any commit is pushed to `develop`, go to the GitHub repository → Actions tab. You
should see a workflow run appear within 1 minute named "CI". Click into it — you should see
the job names from V1 listed. Jobs that have no test code yet should still show as green
(they complete with zero tests, which is a pass). If any job shows a red X, click it, read
the failure message in plain language, and flag the failing step name.

**V5 — Security audit passes.**
In the GitHub Actions run, find the `security-audit` job. It should complete with a green
checkmark showing no high-severity vulnerabilities in the dependency list. If it fails with
messages mentioning a specific package name and "vulnerability", flag the package name
listed.

---

*End of Phase 1 · Sub-phase 01 · File 02*
*Next: File 03 — Dev Environment, CI, and Tooling Baseline*
