# Maintafox Desktop — Branching Strategy

> Effective from Phase 1 · Sub-phase 01 · File 02 · Sprint S1.
> This document governs all version control operations for the Maintafox Desktop project.
> Agent automation detail (always-on): `.cursor/rules/github-git-strategy.mdc` — keep both in sync.

Maintafox is a **tightly integrated product**. Daily work expects a single runnable tree: **`develop` must contain every completed feature**.

---

## Section 1 — Daily integration model (`develop` first)

```
main ─────────────────────────────────────── production-grade, signed releases
  │
  └─ staging ──────────────────────────────── supervisor acceptance
       │
       └─ develop ─────────────────────────── DAILY INTEGRATION (run the app from here)
            │
            ├─ feature/* ── short-lived only (large / risky work)
            ├─ fix/* ────── short-lived defect fixes
            └─ chore/* ──── short-lived non-product changes
```

Hotfix path (unchanged):

```
main ── hotfix/{version}-{slug} ─── merged back to both main AND develop
```

### Rules (non-negotiable)

1. **`develop` is the daily integration branch.** Finished features merge into `develop` **immediately after verification**. Normal day-to-day work and “run the whole app” continue from `develop`.
2. **Feature branches are temporary.** Create them only for **large or risky** changes that may temporarily break the application. Lifetime is normally **hours or a few days**, not weeks. After verification (`cargo check`, `pnpm typecheck`, `pnpm rbac:check` when RBAC changed), **merge to `develop` and delete or archive** the branch.
3. **A finished feature not merged into `develop` is unfinished.** Do not leave completed work only on long-lived feature branches or in stashes.
4. **Stash is emergency storage only** — never a source of truth.
5. **When launching from `develop`, every completed feature must be present.** Do not keep finished features intentionally separated unless the product owner **explicitly** asks.
6. **Before creating a new feature branch:** search existing branches, search stashes, verify the work is not already present, and **continue existing work** instead of duplicating it.
7. **`cleanup/*` / reconstruction branches are one-time maintenance only.** They must **never** become the development base. Merge to `develop`, then delete/archive.

Promotion to production remains: `develop` → `staging` → `main` (version tags only on `main`).

---

## Section 2 — When to create each branch type

- **Prefer working toward `develop`:** small, safe, verified slices should land on `develop` quickly via a short-lived `feature/*` / `chore/*` / `fix/*` PR — not sit isolated for weeks.
- **`feature/*`**: large or risky implementation that may break the app while in progress; one coherent feature per branch (e.g. `feature/p2-sp04-di-lifecycle-disposition`). Not a long-running personal workspace.
- **`fix/*`**: defect fix; reference the issue id when applicable (e.g. `fix/MAF-42-auth-offline-grace`).
- **`hotfix/*`**: emergency fix from a **release tag on `main`**, not from `develop`.
- **`chore/*`**: tooling, config, docs with no product runtime change (e.g. this governance update).
- **`cleanup/*` / reconstruction**: one-time history/maintenance only — never the daily base.

### Before creating a branch

1. `git branch -a` — search for the same feature/sprint/slug.
2. `git stash list` — ensure the work is not already parked in a stash.
3. Confirm the change is not already on `develop` or another active branch.
4. If a matching branch or recoverable stash exists → **continue / restore there**, do not duplicate.
5. If unsure → **ask** before branching.

---

## Section 3 — Merge-back rules

| Source | Target | Method | Approvals | CI Required |
|--------|--------|--------|-----------|-------------|
| `feature/*` | `develop` | PR — merge **promptly after verification** | 1 | Yes |
| `fix/*` | `develop` | PR — merge promptly after verification | 1 | Yes |
| `chore/*` | `develop` | PR — merge promptly after verification | 1 | Yes |
| `cleanup/*` | `develop` | PR — then **delete** the cleanup branch | 1 | Yes |
| `develop` | `staging` | Manual promotion workflow or PR | 1 | Yes — all gates |
| `staging` | `main` | PR | 2 | Yes — all gates |
| `hotfix/*` | `main` | PR | 2 | Yes — all gates |
| `hotfix/*` | `develop` | Separate PR | 1 | Yes |

- Direct commits to `main`, `staging`, or `develop` are **forbidden** (all land via PR).
- After a successful merge to `develop`, **delete** the temporary branch (optional short local archive). Do not keep finished features living for weeks “just in case.”

### Verification before merge to `develop`

Minimum for product-affecting changes:

- `cargo check`
- `pnpm typecheck`
- `pnpm rbac:check` when RBAC registry or gates changed
- Targeted tests when the risk surface warrants it

---

## Section 4 — Hotfix procedure

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

## Section 5 — Tagging convention

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

## Section 6 — Forbidden actions

| Action | Status |
|--------|--------|
| Force-push to `main`, `staging`, or `develop` | **Never permitted** |
| Committing directly to `main` / `staging` / `develop` | **Forbidden** — all changes arrive via reviewed PR |
| Leaving a **finished** feature unmerged on a long-lived branch or stash | **Forbidden** — unfinished until on `develop` |
| Using `cleanup/*` as the daily development base | **Forbidden** |
| Intentionally keeping finished features off `develop` without explicit owner request | **Forbidden** |
| Amending or rebasing pushed commits on shared branches | **Forbidden** |
| Bypassing CI status checks to merge a PR | **Forbidden** — the CI gate is not optional |
| Using a stash as the only copy of product code | **Forbidden** |
| Version-tagging cleanup/reconstruction or feature tips | **Forbidden** |
| Leaving stable work uncommitted across sessions | **Forbidden** |

---

## Section 7 — Health, stash, recovery, and cleanup

Agent automation detail: `.cursor/rules/github-git-strategy.mdc`. Keep both in sync.

### One branch = one feature

- Each short-lived `feature/*` owns one coherent slice.
- Do not pile unrelated modules onto the same branch.

### Stash = emergency only

- Never SSOT for product code.
- Before drop/pop/replace: every unique change must already be on a named ref, or restore to a branch and commit first.
- Never `git stash drop` / `git stash clear` without an explicit **SAFE TO DELETE** reachability report.
- Never rely on dangling objects as backup.

### Reachability before delete

Before deleting any branch, stash, or scratch ref (`safety/*`, `compare/*`, `recovery/*`): verify unique commits/blobs are on a permanent branch (usually `develop`) or intentional archive. Preserve first if not.

### Recovery

If recovered work is unrelated to current WIP: dedicated short-lived feature branch → exact restore → commit → verify → **merge to `develop`** (unless the owner explicitly asks to keep it isolated) → delete the recovery branch.

### Pre-reconstruction / pre-recovery snapshot

Before any reconstruction or recovery operation, run and **save**:

```bash
git status
git branch --all
git stash list
git log --graph --decorate --oneline --all -50
git tag
```

Persist under `docs/engineering/git-snapshots/` (dated filename) and/or the session report.

### Session commit discipline

- No feature may remain uncommitted for more than **one logical work session**.
- When stable: branch (if needed) → commit → push → verify → **merge to `develop`**.
- Do not accumulate unrelated cross-module dirty trees.

### Cleanup / reconstruction

- One-time maintenance only.
- Never the long-term base for new features.
- After merge to `develop`: archive if needed, otherwise delete.

### Branch health check and repository inventory

- After every sprint / before the next: Branch Health Report (active, temporary, archived, scratch, stale, merge order, dependency graph; **recommend merges into `develop` and deletions**).
- After every major feature: repository inventory (branch status, dangling commits, `git gc` risk). Highlight finished work still not on `develop`.

### Exact-recovery hooks exception

`--no-verify` is forbidden except when the owner requested **byte-identical** historical recovery and lint/format would alter the blob; document the reason in the commit message or report.

### Default integration order

```
verified slice → develop   # daily path
cleanup/… → develop        # maintenance only, then delete cleanup
```

Do **not** leave finished product features stacked only as `cleanup → feature → feature` away from `develop`.
