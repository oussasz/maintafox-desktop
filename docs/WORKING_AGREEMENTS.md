# Maintafox Desktop — Working Agreements

This document defines the collaboration model between the maintenance engineer supervisor,
AI coding agents, and the release manager across the full multi-phase delivery of
Maintafox Desktop. Every participant in the project is bound by these agreements. No
sprint begins without all parties understanding their role as described here.

---

## Section 1 — Project Roles and Responsibilities

### Maintenance Engineer Supervisor (the client)

The supervisor is the domain authority and the final acceptance gate for every sprint
delivered in this project. The supervisor's responsibilities are:

- Tests every sprint output using the Supervisor Verification steps defined in the
  corresponding roadmap file. Each verification step results in a PASS or FAIL judgment.
- Approves or rejects each sprint before the next one starts. No sprint may begin while
  the previous sprint carries an unresolved FAIL.
- Files `supervisor_feedback` GitHub issues for every sprint, recording the verification
  results, observations, and any blocking items discovered during testing.
- Has no coding responsibility — all feedback is behavioral and operational. The
  supervisor evaluates whether the application behaves correctly, whether terminology
  matches maintenance engineering conventions, and whether workflows reflect real
  industrial practice.
- Is the source of truth on whether the application matches maintenance engineering
  expectations for terminology, workflow sequencing, and user experience. If the
  supervisor says a label, a state name, or a workflow transition is wrong, it is wrong
  — regardless of what the specification says. The specification is then updated through
  the Change Request Protocol (Section 4).

### AI Coding Agent (the builder)

The AI coding agent is responsible for implementing each sprint precisely as specified.
The agent's responsibilities are:

- Implements each sprint according to the detailed prompt in the roadmap file. The sprint
  prompt is the specification — not a suggestion.
- Must not add features, refactor unrelated code, or make architectural decisions not
  covered in the roadmap file or an accepted ADR. If the agent believes a design change
  is necessary, the agent raises it as a comment in the PR — the agent does not
  unilaterally implement it.
- Fills in every section of the PR template before requesting review. Blank sections are
  grounds for immediate rejection.
- Adds all user-visible strings to the appropriate i18n namespace file (`fr/` and `en/`).
  No French or English text may appear as a literal string in any `.tsx` or `.ts` file.
- Runs all CI checks locally before pushing (`pnpm run typecheck`, `pnpm run lint:check`,
  `pnpm run test`, and `cd src-tauri; cargo clippy -- -D warnings; cargo fmt --check;
  cargo test`) and resolves all failures before the PR is opened.
- Documents every new IPC command in `docs/IPC_COMMAND_REGISTRY.md` in the same PR that
  introduces the command. A PR that adds a Tauri command without a registry entry will
  not pass the Definition of Done checklist.

### Release Manager (the coordinator)

The release manager controls the flow of code from development to production. The release
manager's responsibilities are:

- Reviews and merges PRs to `develop` after the supervisor has signed off. No PR merges
  to `develop` without supervisor approval.
- Controls staging promotions (`develop` → `staging`) and production merges
  (`staging` → `main`). The `RELEASE_CHECKLIST.md` governs these promotions.
- Maintains GitHub environments, secrets, and branch protection rules. The release
  manager is the only role authorized to modify repository settings that affect the CI
  pipeline or deployment configuration.
- Owns the CHANGELOG promotion process at release time: moves `[Unreleased]` entries to
  a dated version section and ensures the three version files (`package.json`,
  `Cargo.toml`, `tauri.conf.json`) are synchronized before the merge to `main`.

---

## Section 2 — Sprint Execution Protocol

Every sprint follows this exact sequence. No steps may be skipped or reordered.

1. The release manager or supervisor opens the current roadmap file and assigns the sprint
   to an AI coding agent.
2. The agent reads the full sprint prompt including prerequisites, the acceptance criteria,
   and the Supervisor Verification section.
3. The agent creates a feature branch following the naming convention:
   `feature/p{N}-sp{NN}-f{NN}-s{N}-{slug}`.
4. The agent implements all steps in the sprint prompt. Every step, not a selection.
5. The agent runs all local quality checks and resolves every failure. No known-failing
   check may be pushed.
6. The agent opens a PR to `develop`, filling in every section of the PR template. The PR
   description must include the Supervisor Verification table with empty PASS/FAIL cells
   for the supervisor to complete.
7. The CI pipeline runs automatically; any failure blocks the PR. The agent must fix CI
   failures before requesting review.
8. The supervisor runs the Supervisor Verification steps and fills in the verification
   table in the PR description.
9. The supervisor either approves the PR or opens a `supervisor_feedback` issue with
   blocking items.
10. If blocking items exist, the agent addresses them in the same branch; no new sprint
    starts until the blocking items are resolved.
11. Once approved, the release manager merges to `develop`.
12. The supervisor marks the associated `supervisor_feedback` issue "Approved to Continue".

---

## Section 3 — Definition of Done

A sprint is "Done" when **all** of the following are true:

- All steps in the sprint prompt are implemented and committed.
- All acceptance criteria listed at the end of the sprint prompt are met.
- CI is fully green on the PR branch (all jobs passing).
- The supervisor has completed every Supervisor Verification step and marked it PASS.
- No open issues labeled `priority:critical` or `type:security` are linked to this sprint.
- The PR description "Definition of Done Checklist" has every box checked.
- The PR is merged to `develop`.

A sprint is **not** done if:

- The supervisor marked any verification step as FAIL and the failure is unresolved.
- The CI `tauri-build-check` job failed even once without explanation.
- Any secret or credential was found in the changeset.

There is no partial credit. A sprint that meets 6 of 7 conditions is not done — it is
blocked until all 7 are satisfied.

---

## Section 4 — Change Request Protocol

During a sprint, the supervisor may observe something that should work differently from
what was specified. Changes are inevitable in a multi-phase delivery — this protocol
ensures they are tracked and do not destabilize in-flight work.

1. The supervisor opens a GitHub issue labeled `status:blocked` or `type:feature` with a
   clear description of the desired change.
2. If the change is small (a label change, a color, a word in the UI), the agent can
   include it in the current sprint PR — the supervisor confirms the fix in Supervisor
   Verification.
3. If the change affects the data model, a workflow state machine, or a security control,
   it must be logged as an issue and addressed in a dedicated fix sprint after the current
   sprint closes. These changes carry architectural risk and must not be rushed into an
   unrelated sprint.
4. No agent may modify the sprint prompt file itself during execution — the prompts are
   the project specification. Changes to them represent project scope changes that must
   be reviewed by the release manager before they take effect.

---

## Section 5 — Escalation and Blocking Conditions

The following conditions halt all sprint progress until resolved. These are non-negotiable
safety gates.

- **Security vulnerability discovered** in a dependency (`cargo audit` or `pnpm audit`
  reporting high severity): rotate keys if any were used; update the dependency; force-
  refresh the affected CI job; document in the SECURITY section of `CHANGELOG.md`. No
  code merges to any branch until the vulnerability is resolved.

- **Data migration failure** on a test database: no code may merge to `staging` until the
  migration is fixed and a clean `db:reset` completes on a fresh environment. Data
  integrity is non-negotiable in a CMMS — a failed migration that silently corrupts
  equipment records or work order history is a product-ending event.

- **Supervisor Verification FAIL** with the field "Blocking: YES" checked: all sprint
  work stops until the blocking item is addressed and re-verified. The supervisor's
  blocking judgment is final — the agent does not override it.

- **Signing key suspected compromise**: immediately revoke the key in GitHub Secrets,
  rotate to a new key, and open a `type:security`, `priority:critical` issue. Do not
  continue with release work until the incident is closed and the new key is verified
  in CI.
