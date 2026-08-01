# Phase 1 · Sub-phase 01 · File 04
# Documentation Governance and Working Agreements

## Context and Purpose

This file establishes the documentation governance model, ADR (Architecture Decision
Record) process, changelog discipline, and team working agreements that keep the project
coherent as it grows across 140 sprint files and a multi-phase delivery. Every subsequent
AI coding agent and every supervisor test session is anchored to the agreements written
here.

Documentation in this project is not bureaucracy. It is the mechanism that allows an
industrial maintenance engineer — not a programmer — to verify outputs, track decisions,
and hand the product over to a future team without organizational memory loss.

## Prerequisites

- Files 01–03 of this sub-phase completed (scaffold, CI, dev environment all operational)
- `CHANGELOG.md` does not yet exist (created in this sprint)
- `docs/` directory exists from File 01

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | ADR Process and First Architecture Decisions | ADR template, ADR index, 5 founding ADRs |
| S2 | Changelog and Version Governance | CHANGELOG.md, versioning policy, release checklist |
| S3 | Working Agreements and Handover Protocols | Team charter, onboarding guide, IPC command registry |

---

## Sprint S1 — ADR Process and First Architecture Decisions

### AI Agent Prompt

You are creating the Architecture Decision Record (ADR) process and writing the five
founding decisions for the Maintafox Desktop project. ADRs are short documents that
capture the context, decision, and consequences of every significant architectural choice.
They allow future developers and AI coding agents to understand *why* the project is built
the way it is, not only *what* was built.

---

**Step 1 — Create the ADR template.**

Create `docs/adr/ADR_TEMPLATE.md`:
```markdown
# ADR-{NUMBER}: {TITLE}

**Date:** YYYY-MM-DD
**Status:** Proposed | Accepted | Deprecated | Superseded by ADR-{N}
**Deciders:** {List of people or roles who made this decision}

---

## Context

<!-- Describe the problem or situation that required a decision.
     What forces were at play? What constraints existed?
     Write in plain language — the audience includes non-programmers. -->

## Decision

<!-- State the decision concisely. Start with: "We will..." or "We decided to..." -->

## Rationale

<!-- Why this option over the alternatives? What were the trade-offs?
     List 3–5 bullet points summarizing the reasoning. -->

## Alternatives Considered

<!-- What other options were evaluated and why were they not chosen? -->

| Alternative | Reason Not Chosen |
|---|---|
| | |

## Consequences

<!-- What becomes easier or harder as a result of this decision?
     What technical debt, if any, does this create? -->

**Positive outcomes:**
-

**Trade-offs and costs:**
-

## Linked Resources

<!-- Links to relevant PRD sections, research documents, external references, or
     other ADRs that influenced this decision. -->
```

---

**Step 2 — Create the ADR index.**

Create `docs/adr/INDEX.md`:
```markdown
# Architecture Decision Records — Index

ADRs record significant architectural choices made during Maintafox Desktop development.
They are numbered sequentially and never deleted. Superseded ADRs are marked as such and
linked to the replacement decision.

| ADR | Title | Date | Status |
|-----|-------|------|--------|
| [ADR-001](ADR-001-local-first-tauri-architecture.md) | Local-First Tauri Architecture | 2026-03-31 | Accepted |
| [ADR-002](ADR-002-sqlite-as-local-data-plane.md) | SQLite as Local Data Plane | 2026-03-31 | Accepted |
| [ADR-003](ADR-003-rust-trusted-core-and-ipc-boundary.md) | Rust Trusted Core with Narrow IPC Boundary | 2026-03-31 | Accepted |
| [ADR-004](ADR-004-sea-orm-and-migration-strategy.md) | sea-orm and Migration-Forward Schema Strategy | 2026-03-31 | Accepted |
| [ADR-005](ADR-005-french-first-i18n-architecture.md) | French-First i18n Architecture | 2026-03-31 | Accepted |

*Add new ADRs at the bottom of this table. Never renumber existing ADRs.*
```

---

**Step 3 — Write the five founding ADRs.**

**`docs/adr/ADR-001-local-first-tauri-architecture.md`:**
```markdown
# ADR-001: Local-First Tauri Architecture

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

Maintafox must serve industrial maintenance teams who operate in facilities with
intermittent or absent network connectivity. Field technicians on shop floors, in remote
plant rooms, or during network outages cannot rely on a browser-based SaaS product that
depends on a live connection for every screen transition. At the same time, the product
requires centralized vendor control over licensing, update rollout, and cross-machine
synchronization.

The architecture must make a clear governance choice: which authority lives on the device,
and which lives on the server.

## Decision

We will build Maintafox Desktop as a **Tauri 2.x** application where the local device is
the authoritative runtime for all day-to-day operational workflows (work orders, requests,
planning, inventory, permits, inspections, and reliability). The VPS controls licensing,
update distribution, and sync coordination. The VPS is never the primary execution
dependency.

## Rationale

- Tauri provides a production-grade native desktop shell with capabilities-based security,
  OS-managed file and keyring access, and a clear TypeScript-to-Rust IPC boundary
- The local-first model means authenticated users on trusted devices can work through any
  network outage without data loss or workflow interruption
- All operational evidence (work actuals, failure codes, downtime, audit events) is
  captured locally before sync, ensuring no data loss from connectivity gaps
- Using Tauri over Electron reduces the bundle size, removes the bundled Chromium from the
  attack surface, and gives us Rust's memory safety in the trusted application core
- Separating the VPS as "control plane, not runtime" protects customers from vendor VPS
  downtime or service pricing changes disrupting active maintenance operations

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Browser-based SaaS (React + Node.js API) | Unacceptable dependency on connectivity; cannot operate offline; unsuitable for shared industrial workstations with locked browsers |
| Electron desktop app | Bundles Chromium (large, high CVE surface); Node.js as app core is less type-safe for critical business logic than Rust |
| Progressive Web App (PWA) | Service worker offline model insufficient for industrial data volumes; limited OS-level integration (keyring, file system, system tray) |
| Native Windows app (WinUI/Rust only) | No cross-platform path; eliminates macOS deployment option; significantly higher per-screen development cost |

## Consequences

**Positive outcomes:**
- Maintenance teams can operate without connectivity for the full offline grace period
- No circular dependency: VPS outage does not break day-to-day operations
- Rust boundary enforces a security review checkpoint for all privileged actions
- Binary is self-contained and easily distributed via the signed update system

**Trade-offs and costs:**
- CI must build on Windows, macOS, and Linux (multi-platform matrix required)
- Tauri capability model must be carefully governed — every WebView capability addition
  must be reviewed for attack surface expansion
- Hot module replacement does not extend to Rust code changes

## Linked Resources

- PRD Section 3 — System Architecture Overview
- PRD Section 5 — Responsibility Split: Local App vs. VPS
- ADR-003 — Rust Trusted Core and IPC Boundary
```

**`docs/adr/ADR-002-sqlite-as-local-data-plane.md`:**
```markdown
# ADR-002: SQLite as Local Data Plane

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

The local Maintafox application needs a durable, transactional data store that can hold
the full operational history of a maintenance site — work orders, equipment records, audit
events, inventory movements, reliability snapshots — across years of operation.

The database must work on locked-down industrial Windows workstations that cannot run a
database server process. It must support encryption at rest for confidential operational
data. It must be fast enough for the analytical queries behind dashboards and reports
without a network round-trip.

## Decision

We will use **SQLite 3.x** (WAL mode) as the local data plane, with **SQLCipher 4.x** as
an opt-in encryption layer available where tenant policy or packaging requires it.

## Rationale

- SQLite is the most widely deployed and tested embedded database in the world; no
  server process, no installation, no port conflicts on industrial workstations
- WAL mode provides concurrent reader access and dramatically better write performance
  for the mixed read/write patterns of an operational CMMS
- SQLCipher provides AES-256 encryption for the entire database file with no API change
  over standard SQLite, allowing encryption to be enabled by policy without application
  logic changes
- FTS5 full-text extension (built into SQLite) powers archive, documentation, and
  reference search without an external search engine dependency
- sea-orm and sqlx both have first-class SQLite drivers with Rust async support
- The offline-first synchronization model works naturally with SQLite's file-based
  portability: backup, restore, and export are file operations

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| PostgreSQL local (via pg Docker) | Requires server process; unacceptable on locked-down industrial machines |
| DuckDB | Excellent OLAP performance but lacks transactional guarantees required for operational workflow state machines |
| Realm / SQLite derivative | Vendor-controlled; complicates migration path and supply chain |
| IndexedDB (browser-side) | Not accessible from Rust application core; does not support encryption at rest |

## Consequences

**Positive outcomes:**
- Zero deployment complexity: database is a single file the application manages
- Backup and export are trivial file copy operations
- FTS5 eliminates the need for a separate search index for documentation and archive
- SQLCipher upgrade path is available without schema changes

**Trade-offs and costs:**
- SQLite's row-level write lock limits concurrent write throughput beyond a single writer;
  acceptable for the expected single-machine workload model
- Large analytical exports or RAMS computations must run in background tasks to avoid
  blocking the UI event loop
- `SQLX_OFFLINE` mode must be maintained for CI (query macros need a pre-compiled cache)

## Linked Resources

- PRD Section 4.3 — Local Data Plane technology stack
- PRD Section 7 — Database Architecture
- ADR-004 — sea-orm and Migration-Forward Schema Strategy
```

**`docs/adr/ADR-003-rust-trusted-core-and-ipc-boundary.md`:**
```markdown
# ADR-003: Rust Trusted Core with Narrow IPC Boundary

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

In a Tauri application, the WebView (React) and the native core (Rust) are separate
execution environments. React code runs inside a sandboxed WebView; it cannot directly
access files, databases, OS keyrings, cryptographic material, or system processes. The
boundary between the two contexts is the IPC layer.

This boundary can be made wide (expose many native operations) or narrow (expose only
typed, validated, business-meaningful commands). Each approach has different security and
maintainability implications.

## Decision

We will maintain a **narrow, explicitly typed IPC boundary**. The React layer may only
call named Tauri commands exposed in `src-tauri/src/commands/`. Each command validates
inputs, delegates to a service function in `src-tauri/src/services/`, and returns a
typed `AppResult<T>`. No raw filesystem paths, database handles, or secret material ever
cross the IPC boundary.

## Rationale

- A narrow boundary means the Tauri `capabilities/default.json` can be minimal; we never
  expose shell execution, arbitrary filesystem access, or raw OS API calls to the WebView
- Every IPC command is a reviewed, named entry point — adding a new command requires
  deliberate action (registering in `tauri::generate_handler![]`, documenting in
  `IPC_COMMAND_REGISTRY.md`, reviewing capabilities scope)
- `AppError` serialization at the boundary means the frontend always receives structured
  error codes — no raw Rust panics or OS error strings leak to the UI
- Type-safe contracts in `shared/ipc-types.ts` mirror Rust structs, making breaking
  changes fail at CI compile time rather than at runtime
- Input validation at the command layer (before the service layer) is the last enforced
  security gate before business logic executes

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Wide IPC (expose fs, sqlite, shell directly) | Expands attack surface; WebView compromise can read arbitrary files and execute commands |
| HTTP server on localhost | Violates Tauri architecture guidelines; opens port-based attack surface; requires CORS policy management |
| postMessage only (no typed commands) | No compile-time contract; brittle string-based API; no input validation enforcement |

## Consequences

**Positive outcomes:**
- WebView compromise cannot directly read database files, access OS secrets, or execute
  arbitrary commands
- Breaking changes in IPC contracts are caught at TypeScript compile time
- Every privileged operation in the application can be audited by reading `commands/`

**Trade-offs and costs:**
- Every new backend capability requires a new typed command — no shortcut to "just call
  the backend" from a component
- IPC commands must be maintained in a registry document, adding a documentation step to
  each backend feature sprint

## Linked Resources

- PRD Section 3 — System Architecture Overview (WebView and Rust trust domains)
- PRD Section 12 — Security Architecture
- `docs/CODING_STANDARDS_RUST.md` — IPC Boundary Rules
- `docs/IPC_COMMAND_REGISTRY.md`
```

**`docs/adr/ADR-004-sea-orm-and-migration-strategy.md`:**
```markdown
# ADR-004: sea-orm and Migration-Forward Schema Strategy

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

The Maintafox database schema will evolve across 5 phases and 140+ sprint files. A
migration strategy is needed that allows safe incremental changes without breaking
existing installations, supports rollback in failure scenarios, and works in both
development (frequent changes) and production (careful upgrades).

The ORM must support async Rust, work with SQLite and potentially PostgreSQL (for the VPS
mirror), provide strong typing at the entity level, and integrate naturally with sqlx for
complex query patterns.

## Decision

We will use **sea-orm** as the primary Rust ORM with **sea-orm-migration** for schema
versioning. Migration files are additive and forward-only in production. Rollback
migrations exist for development safety but are not executed automatically. Every schema
change ships as a timestamped migration file.

## Rationale

- sea-orm provides async-first entity definitions, compile-time column names, and native
  SQLite + PostgreSQL support — the two databases used in this system
- sea-orm-migration gives each migration a name and timestamp, making migration history
  scannable in the `seaql_migrations` table
- Additive-only migrations in production prevent destructive changes from silently
  dropping data on customer installations during updates
- The `down()` method in each migration serves as a safety net for development database
  resets, not for production rollback (which follows the backup-restore protocol)
- sqlx is retained alongside sea-orm for complex raw SQL paths where the query builder
  would produce inefficient or unreadable code
- `SQLX_OFFLINE=true` in `.cargo/config.toml` keeps CI from requiring a live database
  during compilation

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Diesel ORM | Synchronous by default; less ergonomic async integration; no PostgreSQL+SQLite parity without significant configuration |
| Raw sqlx only (no ORM) | Higher boilerplate for entity management across 26+ modules; no built-in migration runner |
| Prisma (via Node.js integration) | Requires Node.js in the Rust runtime path; adds cross-boundary complexity |
| Embedded migration SQL files | No compile-time column name safety; difficult to verify completeness across phases |

## Consequences

**Positive outcomes:**
- Schema history is fully audited in the `seaql_migrations` table on every installation
- Entities are typed at compile time — schema drift between Rust structs and database
  columns causes compile errors, not runtime surprises
- The VPS PostgreSQL mirror uses the same sea-orm entities, reducing schema divergence

**Trade-offs and costs:**
- `SQLX_OFFLINE` requires running `cargo sqlx prepare` after schema changes to update the
  offline query cache; this step must be part of the migration PR checklist
- sea-orm's query builder can become verbose for complex multi-join analytical queries;
  those are written in raw sqlx with explanatory comments

## Linked Resources

- PRD Section 7 — Database Architecture
- ADR-002 — SQLite as Local Data Plane
- `docs/CODING_STANDARDS_RUST.md` — Module Organization
```

**`docs/adr/ADR-005-french-first-i18n-architecture.md`:**
```markdown
# ADR-005: French-First i18n Architecture

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

Maintafox targets industrial maintenance teams where French is the primary operational
language at launch. English is required from day one as a secondary locale for
international deployments and for the engineering and support workflow. Future locale
expansion (Arabic, Portuguese, Spanish) is a realistic mid-term requirement.

The product contains dense technical vocabulary: failure modes, maintenance states,
equipment classifications, permit types, inspection checkpoints. Translation quality
matters for safety and regulatory compliance, not only UX polish.

The question is whether to treat i18n as a post-launch concern or as a day-one
architectural constraint.

## Decision

We will implement **French-first i18n as a day-one architectural constraint**. French is
the default locale. Every user-visible string in the application is a translation key.
No French or English text may appear as a literal string in React component files. The
i18n resource model uses namespace files scoped to module boundaries. Missing translation
keys fail the build in production mode.

## Rationale

- Adding i18n to an existing codebase that was built with hardcoded strings is a full
  rewrite of every user-facing file — doing it correctly from sprint 1 costs less
  overall than retrofitting it after 50 modules are built
- French-as-default forces correct i18n behavior from the first line of UI code; English
  becomes an equally serviced locale rather than the implied fallback
- Namespace files scoped to modules (e.g., `workOrder.json`, `equipment.json`) allow
  each module sprint to own its translations without polluting a single global file
- A failing build on missing keys prevents the classic "key shown instead of translation"
  bug from reaching the supervisor acceptance stage
- The maintenance engineering domain requires precise terminology — translation governance
  belongs in the architecture, not as a manual QA step

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Hardcode French, add i18n later | Creates a rewrite project mid-delivery; all component files must be touched; tested-and-accepted behavior breaks |
| English-first with French overlay | English becomes implicitly "correct"; French becomes a translation of a translation; domain vocabulary lose precision |
| Runtime locale loading from VPS | Adds VPS connectivity dependency for UI rendering; breaks offline operation |
| Single large `translations.json` | Untraceable key ownership; merge conflicts on every concurrent sprint; no per-module governance |

## Consequences

**Positive outcomes:**
- French and English are production-quality peers from the first release
- Module teams own their namespace files — no centralized translation bottleneck
- Future locale addition follows a known pattern: add namespace file, nothing else changes

**Trade-offs and costs:**
- Every sprint that adds a UI element must also add the translation key to both
  `fr/` and `en/` namespace files — adds 5–10 minutes per sprint
- The supervisor must review French copy on every acceptance test; errors in French text
  require a fix sprint, not just a comment

## Linked Resources

- PRD Section 2.2 — Strategic Objective 7: Multilingual by design
- PRD Section 6.26 — Configuration Engine (tenant terminology customization)
- `docs/CODING_STANDARDS_FRONTEND.md` — i18n Rule (Section 7)
```

---

**Acceptance criteria:**
- `docs/adr/ADR_TEMPLATE.md` is present with all required sections
- `docs/adr/INDEX.md` lists all 5 ADRs with correct file links
- All 5 ADR files exist under `docs/adr/` with the correct naming convention
- Each ADR contains the mandatory sections: Context, Decision, Rationale, Alternatives,
  Consequences, Linked Resources
- Running `Get-ChildItem docs/adr/` (or `ls docs/adr/`) shows 7 files total (template +
  index + 5 ADRs)

---

### Supervisor Verification — Sprint S1

**V1 — All ADR files are present.**
In VS Code Explorer, navigate to `docs/adr/`. You should see 7 files:
- `ADR_TEMPLATE.md`
- `INDEX.md`
- `ADR-001-local-first-tauri-architecture.md`
- `ADR-002-sqlite-as-local-data-plane.md`
- `ADR-003-rust-trusted-core-and-ipc-boundary.md`
- `ADR-004-sea-orm-and-migration-strategy.md`
- `ADR-005-french-first-i18n-architecture.md`

If any file is missing, flag it by name.

**V2 — ADR index links are correct.**
Open `docs/adr/INDEX.md`. It shows a table with 5 rows. Each row has a link in the first
column. Click each link — every link should open the corresponding ADR file. If clicking
a link shows "File not found" or opens an empty file, flag it.

**V3 — ADRs explain decisions in plain language.**
Open `ADR-001-local-first-tauri-architecture.md`. Read the "Context" section — it should
explain the problem (connectivity in industrial settings, offline needs) in language a
maintenance engineer can understand, not just technical jargon. Read the "Alternatives
Considered" table — it should have at least 4 rows. Flag if the document is less than one
page, or if the Context section is only one sentence.

**V4 — French-first decision is clearly justified.**
Open `ADR-005-french-first-i18n-architecture.md`. Read the "Decision" paragraph. Confirm
it explicitly states French is the default locale and that no French or English text may
appear as a literal string in component files. This is the commitment that governs all
future sprints — if this section is vague or missing, flag it.

---

## Sprint S2 — Changelog and Version Governance

### AI Agent Prompt

You are establishing the changelog and versioning governance for the Maintafox Desktop
project. Every subsequent sprint that adds, changes, or fixes user-visible behavior must
add an entry to `CHANGELOG.md` before the PR may be merged to `main`. The version
governance document defines how version numbers advance through the delivery phases.

---

**Step 1 — Create `CHANGELOG.md` at the project root.**

```markdown
# Changelog

All notable changes to Maintafox Desktop are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Important:** The `[Unreleased]` section must contain at least one entry before a PR
> may be merged to `main`. The release workflow moves `[Unreleased]` content to a dated
> version entry and creates a Git tag.

---

## [Unreleased]

### Added
- Monorepo scaffold: Tauri 2.x + React 18 + TypeScript 5 workspace structure
- Rust application core with `AppError` typed error system and `AppResult<T>` alias
- SQLite local data plane with WAL mode and initial system schema migrations
- pnpm workspace with full TypeScript strict configuration
- ESLint 9 flat config, Prettier, Biome, and lint-staged pre-commit hooks
- Vitest test setup with Tauri IPC mock and jsdom environment
- GitHub Actions CI pipeline: lint, Rust quality, frontend tests, Tauri build check,
  security audit jobs
- Git branching strategy: feature/fix/hotfix/chore branches with protect rules reference
- PR template with Supervisor Acceptance block and Definition of Done checklist
- Issue templates: Bug Report and Supervisor Feedback
- One-command development environment setup scripts (PowerShell and Bash)
- Environment preflight checker with PASS/FAIL table output
- Secrets management policy and `.env.example` with security prohibitions documented
- Architecture Decision Records (ADR-001 through ADR-005)
- Coding standards documents: frontend (7 sections) and Rust (6 sections)
- French-first i18n architecture with `fr/` and `en/` locale namespaces
- `docs/IPC_COMMAND_REGISTRY.md` with initial `health_check` command entry

---

## Version History

*No released versions yet. Version 1.0.0 targets Phase 5 completion.*
```

---

**Step 2 — Write `docs/VERSIONING_POLICY.md`.**

```markdown
# Maintafox Desktop — Versioning Policy

## Semantic Versioning

Maintafox Desktop follows [Semantic Versioning 2.0.0](https://semver.org).

Given version `MAJOR.MINOR.PATCH`:

- **MAJOR** increments when a release breaks backward compatibility with existing
  local databases or sync protocols. In practice, MAJOR increments from 0 to 1 at the
  first production release (v1.0.0). A MAJOR increment in production requires a migration
  guide and customer communication.
- **MINOR** increments when new features, modules, or capabilities are added in a
  backward-compatible way. Maintenance engineers should expect new screens, commands, or
  options, but existing workflows continue without changes.
- **PATCH** increments for bug fixes, security patches, and translation corrections that
  do not change behavior or add features.

## Pre-Release Versioning by Phase

| Phase | Version Range | Notes |
|---|---|---|
| Phase 1 — Secure Foundation | `0.1.x` | Foundation sprint only; no user-facing features |
| Phase 2 — Core Execution Backbone | `0.2.x` | Work orders, requests, and RBAC complete |
| Phase 3 — Planning, Compliance, Material Control | `0.3.x` | Planning, inventory, PM, permits |
| Phase 4 — Control Plane and Integrations | `0.4.x` | Sync, licensing, IoT, ERP |
| Phase 5 — Reliability and Launch Hardening | `1.0.0-rc.N` → `1.0.0` | Final hardening and pilot |

## Build Metadata and Tags

Pre-release tags: `v0.2.0-dev`, `v0.3.1-beta.1`, `v1.0.0-rc.2`

Release tags: `v1.0.0`, `v1.1.0`, `v1.1.1`

All Git tags are annotated and GPG-signed (see `docs/BRANCHING_STRATEGY.md`).

## How to Bump the Version

1. In `package.json`, bump `"version"` to the new value.
2. In `src-tauri/Cargo.toml`, bump `version` to the same value.
3. In `src-tauri/tauri.conf.json`, bump `"version"` to the same value.
4. Ensure these three files are bumped in the same commit and PR.
5. Move the `## [Unreleased]` section in `CHANGELOG.md` to a new dated section:
   `## [0.2.0] — 2026-MM-DD`
6. The release CI workflow auto-tags `main` on merge when the above files are consistent.

## Minimum Required Files in Every Release PR (to `main`)

- [ ] `package.json` version updated
- [ ] `src-tauri/Cargo.toml` version updated
- [ ] `src-tauri/tauri.conf.json` version updated
- [ ] `CHANGELOG.md` has at least one entry under `[Unreleased]`
- [ ] All three version strings are identical
```

---

**Step 3 — Write `docs/RELEASE_CHECKLIST.md`.**

```markdown
# Maintafox Desktop — Release Checklist

Use this checklist before promoting any build from `staging` to `main` and creating a
release tag. It is designed to be completed by the maintenance engineer supervisor and
the release manager together.

## Pre-Promotion Checks (staging environment)

### CI Green
- [ ] All CI jobs pass on the `staging` branch: lint, Rust quality, frontend tests, Tauri
  build check
- [ ] Security audit job shows 0 high-severity vulnerabilities

### Changelog and Versioning
- [ ] `CHANGELOG.md` [Unreleased] section has entries for every sprint included in this
  release
- [ ] `package.json`, `Cargo.toml`, and `tauri.conf.json` all carry the same version
  string
- [ ] The version string follows the Phase versioning table in `VERSIONING_POLICY.md`

### Supervisor Acceptance
- [ ] Every sprint in this release has a completed Supervisor Feedback issue marked
  "Approved to Continue"
- [ ] No open issues labeled `priority:critical` or `type:security` targeting this release
- [ ] The supervisor has run the application on a clean Windows machine (not the
  development machine) and confirmed it starts correctly

### Database and Migration Safety
- [ ] All migrations in this release are additive (no DROP TABLE, no DROP COLUMN without
  prior deprecation cycle)
- [ ] `db:reset` script completes on a clean environment after applying this release
- [ ] SQLX offline cache (`.sqlx/`) is up to date (run `cargo sqlx prepare` if schema
  changed)

### Security
- [ ] `cargo audit` reports 0 known critical/high CVEs in Rust dependencies
- [ ] `pnpm audit --audit-level=high` reports 0 high-severity issues in npm dependencies
- [ ] Gitleaks scan reports 0 leaks
- [ ] No hardcoded credentials or signing keys in any tracked file

## Promotion and Tagging Procedure

1. Open a PR from `staging` → `main`.
2. Get 2 approvals from the release-manager team.
3. Merge to `main`.
4. Create an annotated signed tag on `main`:
   ```
   git tag -a v{VERSION} -m "Maintafox {VERSION} — {brief description}"
   git push origin --tags
   ```
5. The release workflow generates the signed bundle automatically from the tag.
6. Post-tag: confirm the GitHub release page shows the correct installer artifacts.

## Post-Release Monitoring (first 48 hours)

- [ ] No critical bug reports from pilot users within 2 hours of release
- [ ] Sync coordinator shows no error spikes (if VPS is active)
- [ ] Application launches correctly on at least 2 independent Windows machines
- [ ] Supervisor confirms the release notes match the observable changes

## Hotfix Trigger Criteria

A hotfix must be started immediately if any of the following occur after release:
- Application crash on startup affecting more than one machine
- Data loss or corruption in the local database
- Authentication bypass or unauthorized access vector discovered
- Any work order or intervention request cannot be created or closed
```

---

**Acceptance criteria:**
- `CHANGELOG.md` is present at the project root with a non-empty `[Unreleased]` section
- `docs/VERSIONING_POLICY.md` exists with the phase version table and bump procedure
- `docs/RELEASE_CHECKLIST.md` exists with the pre-promotion, promotion, and post-release
  sections and the hotfix trigger criteria
- `CHANGELOG.md` follows Keep a Changelog format (verifiable by reading it)

---

### Supervisor Verification — Sprint S2

**V1 — CHANGELOG is present and has content.**
In VS Code Explorer, open `CHANGELOG.md` in the project root. The file should have an
`[Unreleased]` section with a list of bullet points under "Added". Count the bullet points
— there should be at least 10 entries describing the work done in Sub-phase 01. If the
file is empty or the Unreleased section has no bullets, flag it.

**V2 — Versioning policy explains phase progression.**
Open `docs/VERSIONING_POLICY.md`. Find the table in the "Pre-Release Versioning by Phase"
section. Confirm it lists all 5 phases with their version ranges. The table must show
Phase 5 reaching `1.0.0`. If the table has fewer than 5 rows or Version 1.0.0 is not
mentioned, flag it.

**V3 — Release checklist requires supervisor sign-off.**
Open `docs/RELEASE_CHECKLIST.md`. Find the "Supervisor Acceptance" section. It should
contain a checkbox for "Every sprint in this release has a completed Supervisor Feedback
issue marked Approved to Continue". This confirms your sign-off is a formal gate before
any release. If this checkbox is missing, flag it.

**V4 — Hotfix criteria are defined.**
Still in `RELEASE_CHECKLIST.md`, scroll to the bottom. Find "Hotfix Trigger Criteria". It
should list at least 3 conditions that require an immediate hotfix. Confirm that "Data
loss or corruption" is one of them. If the section is missing, flag it.

---

## Sprint S3 — Working Agreements and Handover Protocols

### AI Agent Prompt

You are writing the team working agreements, handover protocols, and the IPC command
registry for the Maintafox Desktop project. These documents are the human and AI
collaboration contracts that make the project sustainable across a long multi-phase
delivery. Every new sprint, every onboarding session, and every handover to a new AI
coding agent begins with these documents.

---

**Step 1 — Create `docs/IPC_COMMAND_REGISTRY.md`.**

```markdown
# IPC Command Registry

Every Tauri IPC command exposed by the Maintafox Rust application core must be registered
in this file. Adding a command to `tauri::generate_handler![]` without a corresponding
entry here will fail the PR checklist review.

## Format

Each entry must include:
- **Command name** (exact Rust function name used in `generate_handler![]`)
- **Location** (file path in `src-tauri/src/commands/`)
- **Input types** (Rust struct name and TypeScript equivalent in `shared/ipc-types.ts`)
- **Output type** (Rust type and TypeScript equivalent)
- **Auth required** (whether the command requires a valid session)
- **Phase** (which phase or sprint introduced this command)
- **Description** (plain-language purpose of the command)

---

## Registered Commands

### `health_check`
| Field | Value |
|---|---|
| **Location** | `src-tauri/src/commands/mod.rs` |
| **Input** | None |
| **Output** | `serde_json::Value` → `{ status: string; version: string }` |
| **TS Type** | `HealthCheckResponse` in `shared/ipc-types.ts` |
| **Auth required** | No |
| **Phase** | Phase 1 · Sub-phase 01 · File 01 · Sprint S1 |
| **Description** | Returns the application health status and version string. Used by the frontend to verify the IPC bridge is operational on startup. |

---

*Add new commands below this line in the order they are implemented.*
*Do not remove entries for deprecated commands — mark them Deprecated with the replacing command.*
```

---

**Step 2 — Write `docs/WORKING_AGREEMENTS.md`.**

The document must contain these five sections written in full prose:

**Section 1 — Project Roles and Responsibilities**

**Maintenance Engineer Supervisor (the client)**
- Tests every sprint output using the Supervisor Verification steps in the roadmap file
- Approves or rejects each sprint before the next one starts
- Files `supervisor_feedback` GitHub issues for every sprint
- Has no coding responsibility — all feedback is behavioral and operational
- Is the source of truth on whether the application matches maintenance engineering
  expectations for terminology, workflow, and user experience

**AI Coding Agent (the builder)**
- Implements each sprint according to the detailed prompt in the roadmap file
- Must not add features, refactor unrelated code, or make architectural decisions not
  covered in the roadmap file or an accepted ADR
- Fills in every section of the PR template before requesting review
- Adds all user-visible strings to the appropriate i18n namespace file
- Runs all CI checks locally before pushing (`pnpm run typecheck`, `pnpm run lint:check`,
  `pnpm run test:rust`) and resolves failures before the PR is opened
- Documents every new IPC command in `IPC_COMMAND_REGISTRY.md` in the same PR

**Release Manager (the coordinator)**
- Reviews and merges PRs to `develop` after supervisor sign-off
- Controls staging promotions and production merges
- Maintains GitHub environments, secrets, and branch protection rules
- Owns the CHANGELOG promotion process at release time

**Section 2 — Sprint Execution Protocol**

Every sprint follows this exact sequence:

1. The release manager or supervisor opens the current roadmap file and assigns the sprint
   to an AI coding agent
2. The agent reads the full sprint prompt including prerequisites, the acceptance criteria,
   and the Supervisor Verification section
3. The agent creates a feature branch: `feature/p{N}-sp{NN}-f{NN}-s{N}-{slug}`
4. The agent implements all steps in the sprint prompt
5. The agent runs all local quality checks and resolves every failure
6. The agent opens a PR to `develop`, filling in every section of the PR template
7. The CI pipeline runs automatically; any failure blocks the PR
8. The supervisor runs the Supervisor Verification steps and fills in the verification
   table in the PR description
9. The supervisor either approves or opens a `supervisor_feedback` issue with blocking
   items
10. If blocking items exist, the agent addresses them in the same branch; no new sprint
   starts until the blocking items are resolved
11. Once approved, the release manager merges to `develop`
12. The supervisor marks the associated `supervisor_feedback` issue "Approved to Continue"

**Section 3 — Definition of Done**

A sprint is "Done" when all of the following are true:
- All steps in the sprint prompt are implemented and committed
- All acceptance criteria listed at the end of the sprint prompt are met
- CI is fully green on the PR branch (all jobs passing)
- The supervisor has completed every Supervisor Verification step and marked it PASS
- No open issues labeled `priority:critical` or `type:security` are linked to this sprint
- The PR description "Definition of Done Checklist" has every box checked
- The PR is merged to `develop`

A sprint is **not** done if:
- The supervisor marked any verification step as FAIL and the failure is unresolved
- The CI `tauri-build-check` job failed even once without explanation
- Any secret or credential was found in the changeset

**Section 4 — Change Request Protocol**

During a sprint, the supervisor may observe something that should work differently from
what was specified. The protocol is:

1. The supervisor opens a GitHub issue labeled `status:blocked` or `type:feature` with a
   clear description of the desired change
2. If the change is small (a label change, a color, a word in the UI), the agent can
   include it in the current sprint PR — the supervisor confirms it in Supervisor
   Verification
3. If the change affects the data model, a workflow state machine, or a security control,
   it must be logged as an issue and addressed in a dedicated fix sprint after the current
   sprint closes
4. No agent may modify the sprint prompt file itself during execution — the prompts are
   the project specification; changes to them represent project scope changes that must
   be reviewed by the release manager

**Section 5 — Escalation and Blocking Conditions**

The following conditions halt all sprint progress until resolved:

- **Security vulnerability discovered** in a dependency (`cargo audit` or `pnpm audit`
  reporting high severity): rotate keys if any were used; update the dependency; force-
  refresh the affected CI job; document in the SECURITY section of `CHANGELOG.md`
- **Data migration failure** on a test database: no code may merge to `staging` until the
  migration is fixed and a clean `db:reset` completes on a fresh machine
- **Supervisor Verification FAIL** with the field "Blocking: YES" checked: all sprint
  work stops until the blocking item is addressed and re-verified
- **Signing key suspected compromise**: immediately revoke in GitHub Secrets, rotate, and
  open a `type:security`, `priority:critical` issue; do not continue with release work
  until the incident is closed

---

**Step 3 — Write `docs/ONBOARDING_GUIDE.md`.**

The document is written for a new AI coding agent beginning their first sprint on this
project. It is deliberately brief because all detail lives in the sprint files.

```markdown
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
```

---

**Step 4 — Add `docs/IPC_COMMAND_REGISTRY.md` to the PR checklist in
`.github/PULL_REQUEST_TEMPLATE.md`.**

Open the PR template and verify the Definition of Done Checklist already contains:
`- [ ] New IPC commands added to docs/IPC_COMMAND_REGISTRY.md`

If this line is present from File 02 Sprint S2, no change is needed. If it is absent,
add it to the checklist section.

---

**Acceptance criteria:**
- `docs/IPC_COMMAND_REGISTRY.md` is present with the `health_check` entry
- `docs/WORKING_AGREEMENTS.md` contains all 5 sections
- `docs/ONBOARDING_GUIDE.md` is present with the branch naming convention and the
  "What You Must Never Do" list
- PR template Definition of Done checklist references `IPC_COMMAND_REGISTRY.md`

---

### Supervisor Verification — Sprint S3

**V1 — IPC Command Registry has the first entry.**
Open `docs/IPC_COMMAND_REGISTRY.md`. It should contain a table entry for `health_check`
showing fields including "Auth required: No" and the TypeScript type name
`HealthCheckResponse`. This is the registry every future sprint will add to. If the file
is empty or the health_check entry is missing, flag it.

**V2 — Working Agreements define your role clearly.**
Open `docs/WORKING_AGREEMENTS.md`. Section 1 "Project Roles and Responsibilities" should
have a subsection titled "Maintenance Engineer Supervisor (the client)". Read it — it
should say your role is to test sprint outputs, and that you have no coding
responsibility. Section 3 "Definition of Done" should define exactly when a sprint is
complete. If either section is missing or your role is not described in a dedicated
paragraph, flag it.

**V3 — Onboarding guide has the "Never Do" list.**
Open `docs/ONBOARDING_GUIDE.md`. Scroll to the section "What You Must Never Do". Count
the bullet points — there should be at least 8. Confirm that "Call `invoke()` directly
from a React component" and "Commit `.env.local`" are both present. If the section is
missing or has fewer than 6 items, flag it.

**V4 — Sub-phase 01 is complete (milestone review).**
This is the final file of Sub-phase 01 — Engineering Baseline and Repo Standards. Open
each of the four files in VS Code and confirm none is empty:

| File | Expected Content |
|---|---|
| `01-solution-structure-and-coding-standards.md` | 3 sprints covering monorepo scaffold, TypeScript standards, Rust standards |
| `02-branching-review-and-quality-gates.md` | 3 sprints covering branch model, PR templates, CI pipeline |
| `03-dev-environment-ci-and-tooling-baseline.md` | 3 sprints covering setup scripts, secrets baseline, DB tooling |
| `04-documentation-governance-and-working-agreements.md` | 3 sprints covering ADRs, changelog, working agreements |

If any file is empty or clearly incomplete (headers only, no sprint content), flag it.

**V5 — Full Sub-phase 01 sprint count.**
Count the total sprint blocks across all 4 files (look for lines starting with
`### AI Agent Prompt`). There should be exactly **12 sprints** across the 4 files (3 per
file). If you count fewer than 12, one or more sprint prompts is missing — flag which
file has fewer than 3 sprints.

---

## Sub-phase 01 Completion Summary

When all 4 files are written, all 12 sprints are implemented, all supervisor verification
steps pass, and all 12 sprint PRs are merged to `develop`, Sub-phase 01 is complete.

The team may then proceed to Sub-phase 02: **Tauri Shell, Rust Core, React Workspace Shell**.

**What Sub-phase 01 delivered:**
- A monorepo that compiles cleanly (TypeScript + Rust)
- A working Tauri application shell with the first IPC command
- Full code quality tooling enforced by pre-commit hooks and CI
- A branching and PR model ready for 130+ future sprints
- A documented, repeatable developer environment setup
- Secrets governance and security scanning baseline
- Architecture Decision Records capturing the five founding design choices
- Changelog and version governance for the full 5-phase delivery
- Team working agreements that define how the supervisor, AI agents, and release manager
  collaborate across every phase

**Nothing in this sub-phase is visible to end users.** The Tauri window opens and displays
the "Maintafox — initializing" placeholder screen. That is the correct and expected output.
All Phase 1 work is infrastructure. User-visible features begin in Phase 2.

---

*End of Phase 1 · Sub-phase 01 · File 04*
*Next session: Phase 1 · Sub-phase 02 — Tauri Shell, Rust Core, React Workspace Shell*
