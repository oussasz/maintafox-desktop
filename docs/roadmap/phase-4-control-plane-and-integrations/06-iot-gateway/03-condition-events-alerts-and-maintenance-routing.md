# Condition Events Alerts And Maintenance Routing

**PRD:** §6.21

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Condition Event Model And Alert Semantics
- Define condition-event contract with severity, confidence, persistence window, originating rule version, and affected asset context.
- Classify events into routing outcomes (`informational`, `operator_alert`, `DI_candidate`, `urgent_intervention`) based on policy and criticality.
- Add dedupe and correlation logic for repeated events so one persistent issue does not flood downstream workflows.
- Keep event provenance traceable from raw signal through rule evaluation to issued condition event.
- Align severity semantics with notification and work-prioritization models already used in operational modules.

**Cursor Prompt (S1)**
```text
Implement condition-event and alert semantics with severity/governance rules, dedupe/correlation controls, and full provenance from signal to emitted event.
```

### S2 - Maintenance Routing Into DI/WO/Notification Workflows
- Implement routing policies that convert high-confidence condition events into DI records, notifications, or escalated maintenance actions.
- Add routing prechecks for asset state, existing open intervention, and duplicate suppression windows.
- Support policy choice between auto-create and operator-approval routing paths by event class.
- Add context-rich payload transfer into DI/WO creation (signal snapshot, trend excerpt, threshold breach details, device ID).
- Ensure routed records retain trace link back to originating IoT condition event for audit and post-mortem analysis.

**Cursor Prompt (S2)**
```text
Deliver IoT-to-maintenance routing with policy-driven auto-create/approval paths, duplicate suppression, and full traceability into DI/WO workflows.
```

### S3 - IoT Operator UX And Cross-Module Validation
- Build IoT status UI showing active alerts, condition trends, routed actions, and unresolved anomaly queues.
- Add operator controls for acknowledge, suppress, escalate, and create manual follow-up from condition events.
- Validate cross-module behavior with DI/notification/activity feed integration so one event triggers coherent downstream actions.
- Add tests for routing correctness, dedupe windows, and stale-asset binding safeguards.
- Gate completion on end-to-end scenario tests proving alert-to-action workflow reliability in realistic operational cases.

**Cursor Prompt (S3)**
```text
Finalize condition-event operations with IoT operator UX, actionable controls, and cross-module tests for reliable alert-to-intervention routing behavior.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
