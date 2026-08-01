# Issuance Activation Suspension And Handback Flows

**PRD:** §6.23

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Workflow** — Transitions issue → active → suspended → handback.
- **Checkpoints** — `permit_checkpoints` completion pipeline.
- **UI** — Mobile-friendly checkpoint panel.
- **Audit** — All transitions to audit/activity.
- **Assignee availability gate** — Validate assigned permit actors against shift availability and active critical blocks before activation.
- **Cost trace link** — Capture permit suspension/handback delays as structured causes for WO labor-cost and schedule-variance analysis.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
