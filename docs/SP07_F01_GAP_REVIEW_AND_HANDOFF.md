# SP07-F01 Gap Review and Handoff

Date: 2026-04-13
Scope: Phase 2 - Sub-phase 07 - File 01 (notifications core, commands, UI)

## Review Summary

This review checked implementation completeness against the SP07-F01 acceptance criteria, with
extra focus on frontend behavior and command-level guardrails.

## Gaps Found and Fixed

1. **Potential duplicate inbox rows when multiple active rules share a category**
   - Risk: `list_notifications` used a direct `LEFT JOIN notification_rules`, which can duplicate
     notification rows if data drift introduces >1 active rule per category.
   - Fix: replaced join-derived `requires_ack` with a correlated subquery using
     `MAX(requires_ack)` per category.

2. **Silent success when updating a non-existent notification rule**
   - Risk: `update_notification_rule` returned `Ok(())` even if no rule matched `rule_id`.
   - Fix: added `rows_affected()` guard and return `AppError::NotFound` when zero rows are updated.

3. **Legacy unread-count hook command name drift**
   - Risk: `use-notification-count` still referenced `get_unread_notification_count` while backend
     command is `get_unread_count`.
   - Fix: updated hook invocation and its inline docs.

4. **Shared IPC contract coverage was incomplete for notifications**
   - Risk: notification payloads were typed only in local service schemas, not in shared IPC
     contracts.
   - Fix: added notification interfaces to `shared/ipc-types.ts`.

## Acceptance Coverage Status

- `cargo check`: pass
- `pnpm typecheck`: pass
- `emit_event` dedupe + fire-and-log tests: pass
- Commands and UI wiring: implemented and registered

## Carry-Forward Notes for SP07-F02/F03

1. **Migration sequence continuity**
   - The roadmap text for SP07-F01/F02 uses fixed migration ordinals (`029`, `030`) that collide
     with already-applied migrations in this repository.
   - For implementation, continue using the next contiguous sequence number in
     `src-tauri/src/migrations/mod.rs` and `src-tauri/src/db/migration_integrity.rs`.

2. **Recommended next validation hardening**
   - Add backend integration tests for:
     - unread count transitions after event emission
     - non-configurable category preference updates blocked at command level
     - scheduler escalation transitions over simulated time windows

3. **Frontend usability follow-up (SP07-F03 candidate)**
   - Add optimistic-to-confirmed toast feedback for inbox actions (`Read`, `Acknowledge`, `Snooze`)
     and preference writes to improve operator trust in action persistence.
