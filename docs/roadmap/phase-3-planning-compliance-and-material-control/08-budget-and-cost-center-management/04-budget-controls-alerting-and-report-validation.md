# Budget Controls Alerting And Report Validation

**PRD:** §6.24

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Alert Configuration, Threshold Semantics, And Escalation Controls
- Add `budget_alert_configs` management with configurable threshold semantics by cost center, bucket, alert type, recipient set, and active state.
- Implement default threshold rules aligned with the PRD and research: 80%, 100%, and 120% of frozen control budget, plus forecast-overrun risk before year-end.
- Add specialized labor-alert templates for personnel/team labor-hour overrun, overtime spike, contractor-cost drift, and unplanned emergency-spend concentration within the same period.
- Emit assignment-risk alerts when planned WO commitment materially exceeds available qualified labor capacity for the same period, using planning readiness and availability evidence rather than static staffing assumptions.
- Route alerts through SP07 notification payloads with acknowledgement state, dedupe protection, and retained alert history so repeated budget-warning banners do not become noise.
- Enforce alert-source discipline: only frozen control baseline plus current commitments/posted actuals/approved forecasts may drive budget alerts; draft and what-if versions must not trigger production alerts.

**Cursor Prompt (S1)**
```text
Implement budget alerting with configurable threshold rules, labor-specific warning templates, assignment-risk detection from real planning capacity evidence, and notification dedupe/acknowledgement controls. Only frozen control baselines may drive production alerts.
```

### S2 - Report Packs, Export Views, And Financial Explainability
- Build budget report query layer for PDF/Excel export covering baseline budget, commitments, posted actuals, forecast, variance amount/percent, driver codes, corrective-vs-preventive mix, and top spending WOs/assets.
- Add report pack sections for workforce efficiency appendix: planned vs actual labor hours, utilization, reassignment churn, cost per completed WO, overtime ratio, and contractor share.
- Include financial explainability metadata in every report export: budget version used, currency basis, posting status filter, forecast method mix, and report generation timestamp.
- Add report filters by entity, cost center, fiscal period, bucket, asset family, work category, labor lane, and variance-driver code so finance reviews are audit-friendly rather than static.
- Reuse dashboard/report primitives where appropriate, but keep finance report derivations in the finance service layer to avoid metric drift between screen and export.
- Enforce `fin.report` permission for report generation and export; users with only `fin.view` can inspect summaries but cannot generate governed exports.

**Cursor Prompt (S2)**
```text
Build budget report packs for PDF and Excel with full financial explainability: baseline, commitments, posted actuals, forecast, variance, spend mix, top WOs/assets, and workforce efficiency appendix. Keep derivations consistent between on-screen views and exported reports, and guard export with fin.report.
```

### S3 - Validation Matrix, Operator Checklist, And Completion Gate
- Add automated tests for threshold firing, alert deduplication, acknowledgement flow, permission boundaries, report filter correctness, and export payload integrity.
- Add integration tests for end-to-end budget chain: baseline freeze -> commitment/actual ingestion -> forecast generation -> variance review creation -> alert emission -> report export.
- Add edge-case tests for provisional-only periods, no-commitment periods, reversed actuals, multi-currency display, and duplicate ERP-linked cost center codes.
- Add performance tests for finance dashboard/report queries under realistic multi-period data volumes and high event counts from WO and purchasing rollups.
- Gate completion on clean `cargo check` and `pnpm typecheck`, plus updated completion note in this file.

**Cursor Prompt (S3)**
```text
Finalize budget controls with a full validation suite covering alert thresholds, dedupe, acknowledgements, report correctness, export integrity, multi-currency edge cases, and end-to-end budget workflow from frozen baseline through report generation. Confirm clean cargo check and pnpm typecheck before sign-off.
```

---

## Cross-Module Validation Matrix

| Area | Validation Scope | Expected Result |
|---|---|---|
| Budget lifecycle governance | draft/submitted/approved/frozen/superseded flow and single frozen baseline rule | Invalid transitions are rejected with typed errors; only one frozen control baseline exists per fiscal year and scenario. |
| Cost provenance | WO labor/parts/services/tools, purchase receipts, contract call-offs, and manual adjustments into `budget_actuals` | Every actual row retains source type, source ID, posting status, and source-record traceability without aggregation loss. |
| Posting discipline | provisional vs posted vs reversed behavior | Provisional values stay excluded from official reporting until posting criteria are met; reversals preserve original linkage and audit trail. |
| Commitment separation | approved PO/contract/shutdown commitments versus posted actuals | Commitments remain visible and queryable without being merged into actuals; future spend risk is not hidden. |
| Forecast explainability | PM/backlog/shutdown/planning-driven forecasts with method and confidence | Forecast lines store explicit method/confidence metadata and can be regenerated without duplicate or opaque results. |
| Variance causality | variance reviews linked to baseline, commitments, actuals, forecasts, and source operations | Each review can drill back to the WO/PM/inspection/shutdown records and coded driver that explain the variance. |
| Labor-cost traceability | personnel/team/rate-card labor split and contractor share | Labor spend can be sliced by assignee/team/labor lane and reconciled with planning commitments and WO actuals. |
| ERP alignment | optional cost-center import, export-ready posted actuals, reconciliation flags | ERP-linked data keeps stable external references; duplicate or invalid mappings are rejected and reconciliation issues are surfaced explicitly. |
| Alert semantics | threshold rules, labor alerts, assignment-risk warnings, acknowledgement flow | Only frozen baselines drive alerts; duplicate spam is suppressed and acknowledgement state is retained. |
| Reporting integrity | PDF/Excel filters, spend mix, workforce appendix, and version/currency metadata | Exported reports match on-screen finance summaries and include the metadata required for audit and controller review. |
| Permission boundaries | `fin.view` vs `fin.budget` vs `fin.report` | Read-only users can inspect summaries; budget mutation and report export remain independently enforced. |
| Concurrency safety | stale `row_version` on mutable finance records | Stale updates fail explicitly with typed conflict errors; no silent overwrite on budget or variance workflow writes. |

## Operator Checklist (QA/UAT)

- [ ] Create a cost center hierarchy with parent and child nodes; confirm self-parenting and cyclical parent chains are rejected.
- [ ] Create two budget versions for the same fiscal year/scenario and attempt to freeze both; confirm the second freeze is blocked with a typed validation error.
- [ ] Enter baseline lines across labor, parts, services, and shutdown buckets and verify labor dimensions (team/skill pool/contractor lane) are retained.
- [ ] Ingest WO labor, WO parts, and purchase receipt events into `budget_actuals`; confirm each row preserves source provenance and initial posting status.
- [ ] Post one provisional actual and reverse another; confirm official reporting includes only posted values and reversal history remains visible.
- [ ] Load approved PO or contract commitments and verify they appear separately from actuals in finance queries and UI drilldowns.
- [ ] Generate forecasts from PM occurrences, committed labor hours, and shutdown demand; confirm forecast method and confidence fields are populated.
- [ ] Force a material overrun and verify variance review opens with coded driver, assigned owner, and traceable snapshot context.
- [ ] Trigger threshold and labor-overrun alerts; confirm SP07 notification payloads are emitted once, duplicates are suppressed, and acknowledgement state is stored.
- [ ] Generate PDF/Excel report packs and verify baseline, commitments, posted actuals, forecast, variance drivers, spend mix, and workforce appendix appear with correct currency/version metadata.
- [ ] Test permission boundaries: `fin.view` can inspect summaries, `fin.budget` can mutate baselines/forecasts, and `fin.report` is required for export.
- [ ] Force stale `row_version` on a budget-line or variance-review update and verify conflict failure without side effects.
- [ ] Confirm `cargo check`, finance test suite, and `pnpm typecheck` are clean before sign-off.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
