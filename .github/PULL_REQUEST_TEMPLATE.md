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
