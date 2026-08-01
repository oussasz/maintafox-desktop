# Isolation Checkpoints Tests And WO Gating

**PRD:** §6.23 / §6.5

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Isolations + tests** — `permit_isolations`, `permit_tests`.
- **WO gate** — Block WO `in_progress` until required PTW `active` in `wo` commands.
- **Print/QR** — Permit sheet + LOTO card generation.
- **Integration tests** — WO + permit state pairs.
- **Assignment-time gate** — Prevent WO assignee confirmation if permit-required roles are not staffed by currently available and qualified personnel.
- **Reschedule revalidation** — Re-run permit/qualification checks automatically when WO planned window or assignee changes.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
