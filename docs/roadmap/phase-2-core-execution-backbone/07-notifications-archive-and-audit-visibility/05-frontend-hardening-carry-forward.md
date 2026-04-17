# Frontend Hardening Carry Forward

**PRD:** SP07 UX (gap closure)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Archive export errors** — `ArchiveExplorer.tsx`: try/catch, `actionError`, disable while in flight.
- **Activity/audit a11y** — `ActivityFeedPanel.tsx`, `AuditLogViewer.tsx`: labels, `htmlFor`, saved-view state.
- **Retention panel** — `RetentionPolicyPanel.tsx`: commit on blur/Enter for numeric edits.
- **Audit detail errors** — `AuditLogViewer.tsx`: show fetch failure on expand.
- **Tests** — Vitest for above; `pnpm typecheck`.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
