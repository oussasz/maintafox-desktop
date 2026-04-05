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
