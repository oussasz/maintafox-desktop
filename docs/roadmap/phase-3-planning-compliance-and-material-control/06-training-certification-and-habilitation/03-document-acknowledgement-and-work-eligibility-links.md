# Document Acknowledgement And Work Eligibility Links

**PRD:** §6.20 / §6.15

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Ack state** — `awaiting_document_ack` handling.
- **Gates** — Planning/WO/permit eligibility checks call training resolver.
- **Overrides** — `qualification_overrides` with audit.
- **Assignment integration** — WO assignee picker and planner calendar must surface eligibility status inline (ready, expiring soon, blocked).
- **Temporal validity checks** — Enforce qualification validity at planned start time (not only assignment time) with automatic revalidation on reschedule.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
