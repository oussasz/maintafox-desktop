# Maintafox Desktop — Branching Strategy

> Effective from Phase 1 · Sub-phase 01 · File 02 · Sprint S1.
> This document governs all version control operations for the Maintafox Desktop project.

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
- No tag may be created on `staging` or `develop`.
- Tags are pushed separately: `git push origin --tags`

---

## Section 6 — Forbidden Actions

| Action | Status |
|--------|--------|
| Force-push to `main`, `staging`, or `develop` | **Never permitted** |
| Committing directly to `main` | **Forbidden** — all changes arrive via reviewed PR |
| Deleting merged feature branches before 30 days | **Forbidden** — they serve as the sprint audit trail |
| Amending or rebasing pushed commits on shared branches | **Forbidden** |
| Bypassing CI status checks to merge a PR | **Forbidden** — the CI gate is not optional |
