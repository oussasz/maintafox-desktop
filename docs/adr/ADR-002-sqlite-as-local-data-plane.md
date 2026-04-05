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
