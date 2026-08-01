# Maintafox Desktop — Branching Strategy

> Effective from Phase 1 · Sub-phase 01 · File 02 · Sprint S1.
> This document governs all version control operations for the Maintafox Desktop project.
> Agent automation detail (always-on): `.cursor/rules/github-git-strategy.mdc` — keep both in sync.

---

## Section 1 — Branch Model Diagram

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

---

## Section 2 — When to Create Each Branch Type

- **`feature/*`**: every sprint's implementation work; one branch per sprint file
  (e.g., `feature/p1-sp01-f01-scaffold`)
- **`fix/*`**: a defect found after the sprint is closed; references the GitHub issue
  number (e.g., `fix/MAF-42-auth-offline-grace`)
- **`hotfix/*`**: emergency fix needed on a released version; branches from the release
  tag on `main`, not from `develop`
- **`chore/*`**: dependency updates, config changes, documentation improvements that
  touch no application logic

---

## Section 3 — Merge-Back Rules

| Source | Target | Method | Approvals | CI Required |
|--------|--------|--------|-----------|-------------|
| `feature/*` | `develop` | PR | 1 | Yes |
| `fix/*` | `develop` | PR | 1 | Yes |
| `develop` | `staging` | Manual promotion workflow or PR | 1 | Yes — all gates |
| `staging` | `main` | PR | 2 | Yes — all gates |
| `hotfix/*` | `main` | PR | 2 | Yes — all gates |
| `hotfix/*` | `develop` | Separate PR | 1 | Yes |

- Direct commits to `main`, `staging`, or `develop` are **forbidden**.

---

## Section 4 — Hotfix Procedure

1. Create branch from the release tag:
   ```bash
   git checkout -b hotfix/1.2.1-fix-slug v1.2.0
   ```
2. Implement the minimal fix.
3. Open PR to `main`; get 2 approvals; merge.
4. Tag `main` with the patch version:
   ```bash
   git tag -a v1.2.1 -m "Hotfix: description"
   ```
5. Open a second PR from the same branch to `develop` to prevent regression.
6. Close the hotfix branch after both merges.

---

## Section 5 — Tagging Convention

- Tags are created **only** on `main`.
- Format: `v{MAJOR}.{MINOR}.{PATCH}` for releases, `v{X}.{Y}.{Z}-beta.{N}` for pilots.
- Tags must be annotated tags:
  ```bash
  git tag -a v1.0.0 -m "Maintafox 1.0.0 — Production release"
  ```
- No tag may be created on `staging`, `develop`, `feature/*`, `cleanup/*`, or reconstruction tips.
- Tags are pushed separately: `git push origin --tags`

### Releases must be reproducible

- Every release must be reproducible from a tagged commit on `main`.
- Only **merge-tested** commits (through `develop` → `staging` → `main`) may receive a version tag.
- Never tag temporary cleanup/reconstruction branches or unmerged feature tips.

---

## Section 6 — Forbidden Actions

| Action | Status |
|--------|--------|
| Force-push to `main`, `staging`, or `develop` | **Never permitted** |
| Committing directly to `main` | **Forbidden** — all changes arrive via reviewed PR |
| Deleting merged feature branches before 30 days | **Forbidden** — they serve as the sprint audit trail |
| Amending or rebasing pushed commits on shared branches | **Forbidden** |
| Bypassing CI status checks to merge a PR | **Forbidden** — the CI gate is not optional |
| Using a stash as the only copy of product code | **Forbidden** |
| Version-tagging cleanup/reconstruction or feature tips | **Forbidden** |
| Leaving stable work uncommitted across sessions | **Forbidden** |

---

## Section 7 — Branch health and one-feature discipline

Agent automation detail: `.cursor/rules/github-git-strategy.mdc`. This section is the **human-readable** project policy.

### One branch = one feature

- Each `feature/*` owns one coherent feature or sprint slice.
- Do not pile unrelated modules or recoveries onto the same branch.
- Before creating a branch: search existing local/remote branches; if one already covers the feature/sprint, **continue there**; if unsure, **ask**.

### Temporary integration branches (`cleanup/*`, reconstruction)

- Short-lived only — not standing development lines.
- Merge to `develop` promptly, then **archive or delete**.
- After merge, new features branch from **`develop`**, not from cleanup.
- Acceptable only briefly during rebuild: `develop → cleanup → feature`. Steady state: `develop` with parallel `feature/*` children.

### Session commit discipline

- No feature may remain uncommitted for more than **one logical work session**.
- When work is stable: feature branch → commit → push.
- Do not accumulate unrelated cross-module changes in one dirty working tree.

### Stashes are temporary only

- A stash must never be the SSOT for product code.
- Before drop/pop/replace: every unique change must already be reachable from a named ref (branch/tag).
- If unique code exists only in the stash: dedicated feature branch → restore → commit → then drop.
- Never `git stash drop` / `git stash clear` without an explicit **SAFE TO DELETE** reachability report.
- Never rely on dangling objects as backup.

### Reachability before delete

Before deleting any branch, stash, or scratch ref (`safety/*`, `compare/*`, `recovery/*`): verify commits/blobs are reachable from a permanent branch (or intentional archive). Preserve unique code first.

### Recovery branches

If recovered work is unrelated to current WIP: do **not** merge it in. Create a dedicated feature branch, restore exact content, commit, continue current work separately.

### Pre-reconstruction / pre-recovery snapshot

Before any reconstruction or recovery operation, run and **save**:

```bash
git status
git branch --all
git stash list
git log --graph --decorate --oneline --all -50
git tag
```

Persist output under `docs/engineering/git-snapshots/` (dated filename) and/or the session report before continuing.

### Branch health check and repository inventory

- After every sprint / before the next: Branch Health Report (active, temporary, archived, scratch, stale, merge order, dependency graph; recommend deletions).
- After every major feature/sprint: repository inventory (branches by status, dangling commits, `git gc` risk).

### Exact-recovery hooks exception

Skipping hooks (`--no-verify`) is forbidden except when the user requested **byte-identical** historical recovery and lint/format would alter the blob; document the reason in the commit message or report.

### Suggested merge order (rebuild + isolated recoveries)

```
cleanup/… → develop
feature/… → develop   # e.g. DI lifecycle
(later) one-feature recovery branches → develop   # e.g. AssetPicker, then ProcurementArchive
```
