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
