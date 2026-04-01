# Maintafox Desktop — Agent Onboarding Guide

Welcome to the Maintafox Desktop development project. Before beginning any sprint, read
all sections of this document.

## What You Are Building

Maintafox Desktop is a local-first industrial maintenance management application (CMMS)
built with Tauri 2.x (desktop shell), React 18 + TypeScript 5 (frontend), and Rust
(trusted application core). The product is described in full in the PRD at
`desktop/docs/PRD.md`. The delivery plan is the roadmap in `desktop/docs/roadmap/`.

## Documents to Read Before Your First Sprint

1. `docs/adr/INDEX.md` — Understand five foundational architectural decisions
2. `docs/CODING_STANDARDS_FRONTEND.md` — Learn before touching any `.tsx` or `.ts` file
3. `docs/CODING_STANDARDS_RUST.md` — Learn before touching any Rust file
4. `docs/WORKING_AGREEMENTS.md` — Understand the sprint execution protocol and your role
5. `docs/DEV_ENVIRONMENT.md` — Set up your environment first

## Environment Setup

Run `.\scripts\setup.ps1` (Windows) or `./scripts/setup.sh` (macOS/Linux), then run
`pnpm tsx scripts/check-env.ts` and confirm all checks pass before writing any code.

## Branch Naming Convention

Every sprint uses the pattern:
```
feature/p{phase number}-sp{subphase number, 2 digits}-f{file number, 2 digits}-s{sprint number}-{short slug}
```
Example: `feature/p1-sp01-f03-s2-secrets-baseline`

Create your branch with: `.\scripts\new-branch.ps1 -Type feature -Slug p1-sp01-f03-s2-secrets-baseline`

## Sprint Execution Rules

1. Read the entire sprint prompt before writing any code
2. Implement ALL steps — do not skip acceptance criteria items
3. Run quality checks locally before pushing:
   ```
   pnpm run typecheck
   pnpm run lint:check
   pnpm run format:check
   pnpm run test
   cd src-tauri ; cargo clippy -- -D warnings ; cargo fmt --check ; cargo test ; cd ..
   ```
4. Fill in the entire PR template — every blank section is a rejection reason
5. Add every new IPC command to `docs/IPC_COMMAND_REGISTRY.md`
6. Add every new user-visible string to both `fr/` and `en/` locale namespace files
7. Update `CHANGELOG.md` [Unreleased] before pushing

## What You Must Never Do

- Add features not specified in the sprint prompt
- Refactor code outside the sprint scope
- Commit to `main`, `staging`, or `develop` directly
- Use `.unwrap()` or `.expect()` in Rust without an inline safety comment
- Call `invoke()` directly from a React component — use `src/services/` only
- Leave hardcoded French or English text in any `.tsx` or `.ts` file
- Commit `.env.local`, `.env`, or any file containing secrets
- Skip the PR template or leave sections blank

## Your Deliverable

A merged PR to `develop` where:
- CI is fully green
- The supervisor has verified the sprint output and marked it approved
- CHANGELOG.md reflects the work done
