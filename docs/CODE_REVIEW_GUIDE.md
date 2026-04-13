# Maintafox Desktop — Code Review Guide

> Effective from Phase 1 · Sub-phase 01 · File 02 · Sprint S2.
> This guide governs the review process for every pull request in the Maintafox Desktop
> repository, whether authored by an AI coding agent or a human developer.

---

## Section 1 — Review Roles

### AI Coding Agent

- Writes the code and runs all CI checks locally before opening the PR.
- Fills in every section of the PR template completely — no empty placeholders.
- Labels the PR with the correct phase, type, and status labels from `.github/labels.yml`.
- Does **not** merge its own PRs; a human reviewer must approve.

### Maintenance Engineer Supervisor

- Reviews the **Supervisor Verification** section of the roadmap sprint file.
- Tests the observable output on a local machine (runs the app, checks behavior).
- Does **not** review code syntax — focuses on whether the described outcome matches reality.
- Signs off on the **Supervisor Acceptance** block in the PR before merge.

### Senior Reviewer (optional)

- Reviews architectural changes: additions to `src-tauri/`, new IPC commands, database
  schema changes, and permission model changes.
- Required for any PR targeting `main` (release merges).
- Ensures security-sensitive changes are properly justified in the Security Notes section.

---

## Section 2 — Review Criteria Checklist

The reviewer checks that the PR satisfies **all 25 items** below. Any failure is grounds
for requesting changes.

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

---

## Section 3 — How to Leave Effective Feedback

- **Be specific**: reference the file path and approximate line number in comments.
- **Use plain language**: assume a non-programmer may read the review thread.
- **Distinguish blocking from non-blocking**: prefix blocking comments with `[BLOCKING]`.
  Non-blocking suggestions should be prefixed with `[NIT]` or `[SUGGESTION]`.
- **Do not leave vague comments** such as "this could be better" — describe what should
  change and why.
- **One concern per comment**: avoid bundling multiple unrelated items in a single thread.

---

## Section 4 — Response Time Expectations

| Action | Maximum Time |
|--------|-------------|
| First review response after PR is opened | 24 hours |
| Re-review after "ready for re-review" comment | 8 hours |
| Supervisor acceptance testing after "Sprint output complete" comment | 48 hours |

If a reviewer cannot meet these timelines, they must notify the team and hand off the
review to another qualified reviewer.

---

## Section 5 — Merge Authority

| Merge Path | Who Can Merge | Requirements |
|------------|---------------|--------------|
| `feature/*` → `develop` | Any approved reviewer | 1 approval, CI green |
| `fix/*` → `develop` | Any approved reviewer | 1 approval, CI green |
| `develop` → `staging` | Any approved reviewer | 1 approval, CI green (all gates) |
| `staging` → `main` | Designated `release-manager` only | 2 approvals, all CI gates green, `CHANGELOG.md` [Unreleased] section non-empty, version bump verified |
| `hotfix/*` → `main` | Designated `release-manager` only | 2 approvals, all CI gates green |
| `hotfix/*` → `develop` | Any approved reviewer | 1 approval, CI green |

> **Note:** The merge authority rules align with the branch protection settings defined in
> `.github/branch-protection.md` and the branching model in `docs/BRANCHING_STRATEGY.md`.
