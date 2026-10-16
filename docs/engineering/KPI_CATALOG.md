# KPI Catalog

Single source of truth for Maintafox measurable outcomes.  
Update this file **before** (or as part of) any schema / workflow / UI that enables or improves a KPI.

Agent rule: `.cursor/rules/kpi-catalog.mdc`

---

## How to add a KPI

1. Assign the next free `KPI-NNN` id in this file.
2. Copy the template below — every indicator uses the **same** shape.
3. List required persisted fields and formula inputs explicitly.
4. Note derived metrics, future dashboards, reports, audit, and AI hooks.
5. Set `Status` honestly (`Planned` | `Partial` | `Implemented`).

### Template

```markdown
# KPI-NNN

Nom:
…

Module:
…

Description:
…

Formula:
…

Inputs:
- …

Required fields:
- …

Semantic key:
`UPPER_SNAKE` registry id for Vue d'ensemble / shared adapters (omit when not on Overview strip)

Permission:
Canonical RBAC permission gating display and IPC (`di.view`, `fin.view`, …)

Default roles:
Capability-based Overview defaults (not role name strings): Reliability | Storekeeper | Supervisor | General

Trend sense:
`up-good` | `up-bad` | `neutral` — delta interpretation on Overview VisualMetricCard

Default target:
Prior-horizon comparison only when persisted; never invent numeric SLA targets in UI

Displayed:
- …

Status:
Planned | Partial | Implemented
```

Optional follow-on blocks (when applicable):

```markdown
Derived metrics:
- …

Future dashboards:
- …

Reports:
- …

Audit:
- …

AI opportunities:
- …
```

---

# KPI-001

Nom:
Plan Adherence %

Module:
Work Orders

Description:
Percentage of planned work executed as originally planned (tasks and parts that remain on-plan vs total planned items).

Formula:
(Planned items completed as planned / Total planned items) × 100  
(Define “as planned” per entity: tasks with `origin=planned` and non-cancelled completion; parts with `origin=planned` and `consumption_status=used`.)

Inputs:
- work_order_tasks
- work_order_parts

Required fields:
- origin
- result_code
- consumption_status
- is_completed (tasks)

Displayed:
- WO Detail (Execution / Completion Summary)
- Dashboard Maintenance (future)
- Analytics (future)

Status:
Partial

Derived metrics:
- Parts unused count (`consumption_status=not_used`)
- Parts extra / execution-added count (`origin=execution_added`)
- Tasks added during intervention (`origin=execution_added`)
- Tasks cancelled (`result_code=cancelled`)

Future dashboards:
- Maintenance plan adherence by asset / crew / week
- Top unused-part reasons (from `WORK.PART_UNUSED_REASON`)

Reports:
- Plan vs actual WO packet
- Unused parts by reason code

Audit:
- Execution Log events for part used / not used / added; task added / completed / cancelled

AI opportunities:
- Suggest likely unused reason from historical asset/family patterns
- Flag chronic over-planning (high unused %) for PM template review

---

# KPI-002

Nom:
Labor Efficiency %

Module:
Work Orders

Description:
How efficiently planned labor hours were consumed during execution.

Formula:
(Planned hours / Actual hours) × 100 when Actual > 0; else —

Inputs:
- work_orders.expected_duration_hours
- work_orders.actual_duration_hours / active_labor_hours
- work_order_interveners.hours_worked

Required fields:
- expected_duration_hours
- actual_duration_hours (or sum of labor hours_worked)

Displayed:
- WO Detail (Execution workload block)
- Completion Summary
- Dashboard Maintenance (future)

Status:
Partial

Derived metrics:
- Time variance hours = Actual − Planned (signed)
- Cost variance % (labor + parts planned vs actual)

Cost variance inputs (Partial — not yet a first-class queryable KPI):
- `personnel_rate_cards.labor_rate` (active card: effective_from ≤ today)
- Snapshot on labor capture: `work_order_interveners.hourly_rate` (resolved from rate card or explicit override; null when missing)
- Planning estimate: `schedule_commitments.estimated_labor_cost` (= duration × active labor_rate)

Future dashboards:
- Efficiency distribution by WO type / urgency / crew
- Overrun heatmap by asset criticality

Reports:
- Labor variance export by period

Audit:
- Labor add events on Execution Log (when emitted)

AI opportunities:
- Predict expected duration from similar historical WOs
- Detect optimistic planning bias per planner

---

# KPI-003

Nom:
Downtime Variance (hours)

Module:
Work Orders

Description:
Difference between planned production downtime and actual downtime recorded on the WO.

Formula:
Actual downtime hours − Planned downtime hours  
(Actual = downtime segments + waiting hours as rolled up on the WO.)

Inputs:
- work_orders.planned_downtime_hours
- work_orders.downtime_hours
- work_orders.total_waiting_hours
- work_order_downtime_segments
- work_order_delay_segments (primary cause label)

Required fields:
- planned_downtime_hours
- downtime_hours / segment started_at + ended_at
- classification_code (Phase 2 taxonomy)
- delay_reason_id → WORK.DELAY_REASONS (cause)

Displayed:
- WO Detail (Stops / Completion Summary)
- OEE / downtime dashboards (future)

Status:
Partial

Derived metrics:
- Downtime by classification (mechanical / electrical / waiting_spare / …)
- Primary delay/downtime cause rate

Future dashboards:
- Downtime Pareto by classification and delay reason
- Planned vs actual downtime by line / plant

Reports:
- Downtime variance by asset and period

Audit:
- Execution Log downtime opened / closed

AI opportunities:
- Predict downtime class from symptom / failure mode
- Recommend buffer planned downtime for recurring jobs

---

# KPI-004

Nom:
Part Unused Rate %

Module:
Work Orders / Inventory

Description:
Share of planned parts that were marked not used, with coded reasons for analytics.

Formula:
(Parts with consumption_status = not_used / Parts with origin = planned) × 100

Inputs:
- work_order_parts
- reference_values (WORK.PART_UNUSED_REASON)

Required fields:
- origin
- consumption_status
- not_used_reason_id
- not_used_comment (required when reason code = other)

Displayed:
- Completion Summary
- Inventory / planning analytics (future)

Status:
Partial

Derived metrics:
- Unused reason distribution (inspection_ok, wrong_diagnosis, …)
- Replacement rate (not_used planned + execution_added substitute)
- Parts disposition completeness: pending planned lines must be Used|Not used before WO complete
- Zero-plan confirmation rate (`parts_actuals_confirmed` when planned count = 0)

Future dashboards:
- Unused reason treemap
- Planner accuracy by family
- WOs completed with “no parts used” attestation

Reports:
- Unused parts by reason and warehouse

Audit:
- Execution Log part_not_used with reason context
- Execution Log parts_none_confirmed when technician attests no consumption

AI opportunities:
- Reduce false reservations from chronic unused patterns
- Suggest BOM alternatives when part_unavailable dominates

Gate (completion):
- Planned lines > 0 → every planned line `consumption_status` ∈ {used, not_used}
- Planned lines = 0 → at least one used line OR `parts_actuals_confirmed = 1`

---

# KPI-005

Nom:
Average Purchase Lead Time

Module:
Inventory / Procurement

Description:
Mean observed lead time from PO order/approval date to goods receipt acceptance.

Formula:
AVG(actual_lead_time_days) over goods_receipt_lines in period

Inputs:
- goods_receipt_lines.actual_lead_time_days
- purchase_orders.ordered_at / approved_at / created_at
- goods_receipts.received_at

Required fields:
- actual_lead_time_days
- ordered_qty / received qty on GR lines

Displayed:
- Supplier scorecard
- Article purchase history

Status:
Partial

Derived metrics:
- Lead time variance vs promised / catalog lead_time_days
- On-time delivery % (OTIF)

---

# KPI-006

Nom:
Supplier OTIF %

Module:
Inventory / Procurement

Description:
Share of receipts delivered on or before the expected date (on-time in-full proxy using lead time and qty accuracy).

Formula:
(Receipt lines with actual_lead_time_days ≤ promised_lead_time_days AND delivery_accuracy ≥ 100%) / Receipt lines × 100

Inputs:
- goods_receipt_lines
- inventory_suppliers.default_lead_time_days / article source lead_time_days

Required fields:
- actual_lead_time_days
- ordered_qty, received_qty
- purchase_orders.expected_delivery_date (promise date; set at PO approval)

Displayed:
- Supplier scorecard dashboard

Status:
Planned

Derived metrics:
- Promise-based OTIF (goods_receipts.received_at vs purchase_orders.expected_delivery_date), available once approved POs carry a promise date

---

# KPI-007

Nom:
Supplier Delivery Accuracy %

Module:
Inventory / Procurement

Description:
Received quantity vs ordered quantity per receipt line.

Formula:
(received_qty / ordered_qty) × 100

Inputs:
- goods_receipt_lines.ordered_qty, received_qty

Required fields:
- ordered_qty
- received_qty

Displayed:
- Supplier scorecard
- Procurement analytics

Status:
Partial

Derived metrics:
- Supplier defect / receipt variance rate (|ordered − received| / ordered)

---

# KPI-008

Nom:
Purchase Cycle Time

Module:
Inventory / Procurement

Description:
Elapsed time from requisition creation to GR acceptance.

Formula:
AVG(GR.received_at − requisition.created_at) in days

Inputs:
- procurement_requisitions.created_at
- goods_receipts.received_at

Required fields:
- requisition_id linkage on PO
- received_at

Displayed:
- Procurement dashboard (future)

Status:
Planned

---

# KPI-009

Nom:
Average Procurement Cost

Module:
Inventory / Procurement

Description:
Average unit cost on purchased lines in period.

Formula:
SUM(ordered_qty × unit_price) / SUM(ordered_qty)

Inputs:
- purchase_order_lines.unit_price, ordered_qty
- inventory_supplier_prices

Required fields:
- unit_price on PO lines

Displayed:
- Purchase history, scorecard avg price
- Supplier article sources list (`last_price`, `avg_price`, `currency_label`)

Status:
Partial

Derived metrics:
- Price drift per source (last_price − avg_price) / avg_price
- Negotiated vs realised gap (unit_price_hint − last_price)
- Price spread across the suppliers of one article (MAX − MIN of last_price)

---

# KPI-010

Nom:
Emergency Purchases %

Module:
Inventory / Procurement

Description:
Share of requisitions/POs raised with Emergency priority.

Formula:
(requisitions with purchase_priority = EMERGENCY / all requisitions) × 100

Inputs:
- procurement_requisitions.purchase_priority

Required fields:
- purchase_priority

Displayed:
- Procurement dashboard

Status:
Partial

---

# KPI-011

Nom:
Critical Spare Availability %

Module:
Inventory

Description:
Share of critical-spare articles with available stock > 0.

Formula:
(critical spare articles with available_qty > 0 / critical spare articles) × 100

Inputs:
- articles.is_critical_spare
- stock_balances.available_qty

Required fields:
- is_critical_spare

Displayed:
- Inventory controls / procurement dashboard stockouts

Status:
Partial

---

# KPI-012

Nom:
Fill Rate %

Module:
Inventory / Work Orders

Description:
Share of WO part demand fulfilled from stock (reserved or issued) without shortage.

Formula:
(planned part lines with available/reserved ≥ planned / planned part lines) × 100

Inputs:
- work_order_parts
- stock_balances / stock_reservations

Required fields:
- quantity_planned, quantity_reserved, quantity_issued

Displayed:
- WO Material Readiness

Status:
Partial

---

# KPI-013

Nom:
Service Level %

Module:
Inventory

Description:
One minus stockout frequency over a period (articles reaching zero available).

Formula:
(1 − stockout_events / exposure_days) × 100 (tenant definition)

Inputs:
- stock_balances history / inventory_transactions exits to zero

Required fields:
- available_qty / movements

Displayed:
- Inventory analytics (future)

Status:
Planned

Derived metrics:
- Stockout frequency (count of zero-available events)

---

# KPI-014

Nom:
Inventory Turnover

Module:
Inventory

Description:
Cost of goods issued / average inventory value.

Formula:
COGS (issued) / AVG(inventory valuation)

Inputs:
- inventory_transactions (issues)
- article_cost_profiles / valuation

Required fields:
- standard_unit_cost or valuation method
- issue movements

Displayed:
- Inventory analytics (future)

Status:
Planned

---

# KPI-015

Nom:
Average Repairable Turnaround

Module:
Inventory / Repairables

Description:
Mean days a repairable spends at the vendor, measured from dispatch to physical return.

Formula:
AVG(returned_at − sent_at) over repairable_orders in RETURNED_FROM_REPAIR or CLOSED

Inputs:
- repairable_orders.sent_at (stamped on SENT_FOR_REPAIR)
- repairable_orders.returned_at (stamped on RETURNED_FROM_REPAIR, preserved through CLOSED)

Required fields:
- sent_at
- returned_at
- repair_cost (cost view / repair-vs-replace)

Displayed:
- Repairable order detail (history stats)
- Article repairable history

Status:
Implemented

Derived metrics:
- Repair count per article
- Average repair cost per article
- Vendor turnaround comparison (via repairable_orders.vendor_supplier_id)

Audit:
- Lifecycle trail in inventory_state_events (entity_type `repairable_order`)

Note:
Orders created before migration `m20260731_000136` have no `sent_at` / `returned_at`; they are excluded from the average rather than estimated from `created_at` / `updated_at`.

---

# KPI-016

Nom:
Open Procurement Value

Module:
Inventory / Procurement

Description:
Sum of open PO line value (ordered − received) × unit_price.

Formula:
SUM((ordered_qty − received_qty) × unit_price) over open POs, counting priced lines only

Inputs:
- purchase_orders.status
- purchase_order_lines.ordered_qty, received_qty, unit_price

Required fields:
- unit_price on PO lines (still optional in schema, so coverage is incomplete)

Displayed:
- Purchase order detail (grand_total + grand_total_partial)
- Procurement dashboard (not yet aggregated)

Status:
Partial

Derived metrics:
- Remaining quantity per line (`remaining_qty`)
- Priced-coverage flag (`grand_total_partial`) so an understated total is never shown as exact

Note:
`grand_total` is null when no line is priced and `grand_total_partial` is true whenever at least one line lacks `unit_price`. Never present the value as a complete PO value while the flag is set.

---

# KPI-017

Nom:
Reservation Fulfillment %

Module:
Inventory

Description:
Share of reserved quantity that was issued.

Formula:
SUM(quantity_issued) / SUM(quantity_reserved) × 100

Inputs:
- stock_reservations

Required fields:
- quantity_reserved, quantity_issued

Displayed:
- Article reservation visibility / analytics

Status:
Partial

---

# KPI-018

Nom:
Material Readiness %

Module:
Work Orders / Inventory

Description:
Share of planned WO part lines that are stock-ready (available or reserved) before marking WO Ready.

Formula:
(ready part lines / planned part lines) × 100 per WO; fleet average over period

Inputs:
- work_order_parts
- stock_balances / reservations
- evaluate_wo_readiness material_readiness rule

Required fields:
- quantity_planned, quantity_reserved, article_id

Displayed:
- WO readiness checklist
- Planning panel shortage strip

Status:
Partial

Derived metrics:
- Reserved %
- Missing parts count
- Expected arrival = MIN(purchase_orders.expected_delivery_date) across open WO-linked POs

Note:
Expected arrival reads the promised delivery date only. POs approved before migration `m20260731_000136` have no promise date and are excluded, so `expected_arrival` is null rather than an order date presented as an arrival date.

---

# KPI-019

Nom:
Late PO %

Module:
Inventory / Procurement

Description:
Share of open purchase orders whose promised delivery date has passed without full receipt.

Formula:
(open POs with expected_delivery_date < now / open POs with an expected_delivery_date) × 100

Inputs:
- purchase_orders.status, expected_delivery_date
- goods_receipts.received_at

Required fields:
- purchase_orders.expected_delivery_date

Displayed:
- Procurement dashboard `overdue_pos` (currently hardcoded to 0 — not yet wired to this formula)
- Purchase order detail (promise date)

Status:
Partial

Derived metrics:
- Days late per PO (now − expected_delivery_date)
- Late PO value at risk (late POs × remaining line value, KPI-016)
- Late POs blocking WO readiness (KPI-018)

Future dashboards:
- Overdue procurement by supplier and by buyer

AI opportunities:
- Predict slippage before the promise date from supplier lead-time history (KPI-006)

Note:
The promise date is derived at approval from COALESCE(max supplier article source lead time, supplier default lead time, 7 days) and is never overwritten once set. POs approved before migration `m20260731_000136` carry no promise date and are excluded from both numerator and denominator.

---

# KPI-020

Nom:
Supplier Risk Distribution

Module:
Inventory / Procurement

Description:
Split of active suppliers across LOW / MEDIUM / HIGH risk, derived from delivery performance rather than declared status.

Formula:
Count of suppliers per risk_level, where risk_level is classified from on-time delivery %, rejected %, and actual vs promised lead time

Inputs:
- goods_receipt_lines.actual_lead_time_days, accepted_qty, rejected_qty
- inventory_suppliers.default_lead_time_days
- purchase_orders.expected_delivery_date

Required fields:
- actual_lead_time_days
- rejected_qty
- default_lead_time_days

Displayed:
- Supplier scorecard (`risk_level`)
- Supplier article sources list (`risk_level`)

Status:
Partial

Derived metrics:
- Spend concentration on HIGH-risk suppliers
- Single-sourced critical spares served by a HIGH-risk supplier

Note:
Risk is computed on the fly from the scorecard inputs; it is not persisted, so historical risk trend is not yet queryable. Suppliers with no receipt history classify as LOW by absence of evidence, not by measured performance.

---

# KPI-021

Nom:
Repair vs Replace Ratio

Module:
Inventory / Repairables

Description:
Repair cost of a repairable item as a share of the cost of replacing the same quantity, against the tenant decision threshold.

Formula:
repair_cost / (valuation unit cost × quantity), compared to `procurement.repair_replace_ratio` (default 0.85)

Inputs:
- repairable_orders.repair_cost (falls back to the article's historical average)
- inventory_valuation_policies via `valuation::evaluate_unit_cost`
- app_settings key `procurement.repair_replace_ratio`

Required fields:
- repairable_orders.repair_cost
- an active valuation policy resolving a non-zero unit cost

Displayed:
- Repairable order detail (repair_vs_replace)

Status:
Partial

Derived metrics:
- Cumulative repair spend vs replacement cost per article (repeat-repair detection)
- Share of repairs approved above the threshold

Audit:
- Recommendation is advisory: it does not gate the SCRAPPED / CLOSED transitions

AI opportunities:
- Recommend scrapping chronically repaired serials from repair count and turnaround history (KPI-015)

Note:
Recommendation is one of REPAIR | REPLACE | REVIEW | INSUFFICIENT_DATA. REVIEW is returned when the ratio sits within 0.05 of the threshold, when the repair cost is an average of past repairs rather than this order's actual cost, or when the valuation is provisional. INSUFFICIENT_DATA is returned when either side of the ratio is unavailable — no estimate is invented.

---

# KPI-022

Nom:
Supplier Sourcing Composite Rating

Module:
Inventory / Procurement

Description:
Relative 1–5 rating of every active source of one article, combining realised price, lead time and supplier risk. Supports the preferred-source decision when an article has more than one supplier.

Formula:
Rank each active source ascending on effective price (COALESCE(last_price, unit_price_hint)), lead_time_days and risk_level (LOW<MEDIUM<HIGH); sources missing a criterion take the last rank for it. Average the three ranks, then map linearly onto 5 (best average) … 1 (worst average). All sources rate 5 when there is no spread.

Inputs:
- inventory_supplier_article_sources.lead_time_days, unit_price_hint, is_active
- purchase_order_lines.unit_price (via `last_price` enrichment)
- supplier `risk_level` (KPI-020)

Required fields:
- lead_time_days
- last_price or unit_price_hint
- risk_level

Displayed:
- Article detail supplier comparison card (rating + set-preferred action)

Status:
Partial

Derived metrics:
- Preferred-source alignment: share of articles whose `is_preferred` source is also the top-rated one
- Single-sourced article count (articles with fewer than 2 active sources)
- Savings opportunity: (preferred source price − best-rated source price) × annual consumption

Future dashboards:
- Sourcing quality by article family and by criticality

AI opportunities:
- Recommend preferred-source changes when the rating gap persists across several purchase cycles

Note:
The rating is relative within one article's candidate set, not an absolute supplier score — it cannot be compared across articles. It is computed for display and is not persisted, so rating history is not queryable. Weighting is currently equal across the three criteria; a tenant-configurable weighting is not yet implemented.

---

# KPI-023

Nom:
Procurement Operational Alerts Coverage

Module:
Inventory / Procurement

Description:
Count of actionable procurement alerts by kind (critical stock, late PO, high-risk supplier, overdue repairable) surfaced on the procurement dashboard.

Formula:
Top N alerts per kind where:
- CRITICAL_STOCK: critical spare available_qty ≤ min_stock
- LATE_PO: PO status ∈ {APPROVED, PARTIALLY_RECEIVED} and expected_delivery_date < now
- SUPPLIER_DELAY: supplier scorecard risk_level = HIGH
- REPAIRABLE_OVERDUE: status = SENT_FOR_REPAIR and sent_at + lead SLA < now

Inputs:
- articles.is_critical_spare, min_stock, stock_balances.available_qty
- purchase_orders.expected_delivery_date, status
- inventory_suppliers + goods_receipt_lines (scorecard risk)
- repairable_orders.sent_at, status, vendor lead / article.lead_time_days

Required fields:
- expected_delivery_date on open POs
- sent_at on SENT_FOR_REPAIR repairables

Displayed:
- ProcurementAlertCards on procurement dashboard
- KPI strip receiving_today_count / active_suppliers_count

Status:
Partial

Derived metrics:
- Alert aging (days open)
- Share of late POs that block WO material readiness (KPI-018)

---

# KPI-024

Nom:
Article Monthly Consumption

Module:
Inventory

Description:
Issued quantity of an article per calendar month (ISSUE / ADJUST_OUT movements) for consumption trend analysis and replenishment intelligence.

Formula:
SUM(ABS(quantity)) GROUP BY year-month WHERE movement_type ∈ {ISSUE, ADJUST_OUT}

Inputs:
- inventory_transactions.quantity, movement_type, performed_at, article_id

Required fields:
- inventory_transactions.performed_at
- inventory_transactions.movement_type

Displayed:
- ArticleConsumptionChart on article detail (reservations view)

Status:
Partial

Derived metrics:
- 12-month moving average consumption
- Seasonality index (month vs 12-month average)

AI opportunities:
- Forecast reorder qty from consumption trend + lead time

---

# KPI-025

Nom:
PM BOM Parts Readiness

Module:
Preventive Maintenance / Planning

Description:
Share of PM occurrences that are material-ready for scheduling based on required_parts_json versus committed stock reservations / available stock. Today list_pm_planning_readiness only stubs missing_parts when required_parts_json is non-empty — it does not evaluate actual stock or reservations.

Formula:
(Occurrences with no missing_parts blocker / Occurrences with declared required parts) × 100
— not implemented; readiness currently flags any non-empty required_parts_json as blocked.

Inputs:
- pm_plan_versions.required_parts_json
- stock reservations / balances linked to planned PM demand (not yet wired)

Required fields:
- required_parts_json structure with article identifiers and quantities
- planning reservation contract for PM demand (missing)

Displayed:
- PM planning readiness projection blockers (`missing_parts`)

Status:
Planned

Derived metrics:
- Parts shortage qty by PM plan
- PM occurrences blocked solely by parts vs multi-blocker

Note:
Do not report Ready for parts until reservation/stock evaluation replaces the stub in `list_pm_planning_readiness`.

---

# KPI-026

Nom:
DI Conversion Rate

Module:
Intervention Requests (DI)

Description:
Share of closed DIs that resulted in a Work Order (disposition converted_to_wo).

Formula:
(COUNT closed WHERE disposition_code = 'converted_to_wo' / COUNT closed) × 100

Inputs:
- intervention_requests.status
- intervention_requests.disposition_code
- intervention_requests.converted_to_wo_id

Required fields:
- status = 'closed'
- disposition_code
- converted_to_wo_id when disposition is converted_to_wo

Displayed:
- DI dashboard conversion KPI
- DI stats payload

Status:
Partial

Derived metrics:
- Absolute converted count
- Conversion rate excluding disposition = 'duplicate'

---

# KPI-027

Nom:
DI Reject Rate

Module:
Intervention Requests (DI)

Description:
Share of closed DIs dispositioned as invalid requests.

Formula:
(COUNT closed WHERE disposition_code = 'rejected_invalid' / COUNT closed) × 100

Inputs:
- intervention_requests.status
- intervention_requests.disposition_code

Required fields:
- status = 'closed'
- disposition_code

Displayed:
- DI dashboard reject KPI

Status:
Partial

---

# KPI-028

Nom:
DI Duplicate Rate

Module:
Intervention Requests (DI)

Description:
Share of closed DIs marked as duplicates of another request.

Formula:
(COUNT closed WHERE disposition_code = 'duplicate' / COUNT closed) × 100

Inputs:
- intervention_requests.status
- intervention_requests.disposition_code
- intervention_requests.related_di_id

Required fields:
- status = 'closed'
- disposition_code = 'duplicate'
- related_di_id (mandatory for this disposition)

Displayed:
- DI dashboard duplicate KPI

Status:
Partial

Derived metrics:
- Linked survivor DI code via related_di_id

---

# KPI-029

Nom:
DI Cancel Rate

Module:
Intervention Requests (DI)

Description:
Share of closed DIs cancelled by requester or planner.

Formula:
(COUNT closed WHERE disposition_code IN ('cancelled_by_requester','cancelled_by_planner') / COUNT closed) × 100

Inputs:
- intervention_requests.status
- intervention_requests.disposition_code

Required fields:
- status = 'closed'
- disposition_code

Displayed:
- DI dashboard cancel KPI

Status:
Partial

Note:
Cancelled-by-requester may be excluded from SLA breach blame in product policy (document when SLA stats implement the exclusion).

---

# KPI-030

Nom:
DI No-Work Rate

Module:
Intervention Requests (DI)

Description:
Share of closed DIs that required no maintenance work order (no work, solved immediately, or information only).

Formula:
(COUNT closed WHERE disposition_code IN ('no_work_required','solved_immediately','information_only') / COUNT closed) × 100

Inputs:
- intervention_requests.status
- intervention_requests.disposition_code

Required fields:
- status = 'closed'
- disposition_code

Displayed:
- DI dashboard no-work KPI

Status:
Partial

---

# KPI-031

Nom:
DI Open Backlog by Status

Module:
Intervention Requests (DI)

Description:
Count of non-closed DIs grouped by process status for queue management.

Formula:
COUNT WHERE status != 'closed' GROUP BY status

Inputs:
- intervention_requests.status

Required fields:
- status

Displayed:
- DI dashboard backlog / Kanban counters

Status:
Partial

---

# KPI-032

Nom:
Emergency Reassign Ratio

Module:
Work Orders / Planning

Description:
Share of schedule-committed work orders that received an emergency reassignment (shop-floor override that skips Planning capacity/lock/double-booking gates) within a period. Measures how often the plant bypasses the gated scheduler for night/emergency dispatch.

Formula:
(COUNT distinct WO with at least one `emergency_reassigned` event in period
 / COUNT distinct WO with an active or historical `schedule_commitments` row for source_type=work_order overlapping the period)
 × 100

Inputs:
- WO change / action events with action = `emergency_reassigned` (and override_reason in details)
- `schedule_commitments` where source_type = 'work_order'
- Optional: `schedule_change_log` with reason_code = `emergency_reassign`

Required fields:
- Event action code `emergency_reassigned` persisted on emergency reassign
- `work_orders.assigned_team_id` / `primary_responsible_id` after rename
- `schedule_commitments.source_type`, `source_id`, `frozen_at` (frozen WOs must use Planning break-in, not emergency)

Displayed:
- Not yet (no dashboard tile this pass)

Status:
Partial

Derived metrics:
- Emergency reassigns without a prior commitment (dispatch-before-board)
- Frozen-block rejection count (attempts that must use break-in)

Future dashboards:
- Weekly emergency vs board-committed assignment mix by crew
- Night-shift emergency density

Reports:
- Emergency reassign audit export (actor, reason, before/after crew/person)

Audit:
- WO event `emergency_reassigned` + change-log details_json with override_reason
- Planning `schedule_change_log` reason_code `emergency_reassign` when a commitment assignee is updated

AI opportunities:
- Flag crews with chronic emergency reassign rates vs capacity gaps

---

# KPI-033

Nom:
Mean Time Between Failures (MTBF)

Module:
Reliability / RAMS & Analytics

Description:
Operating exposure (uptime) hours per eligible unplanned failure for a repairable equipment. Core RAMS reliability indicator for the Analytics hub. Non-repairable equipment (`equipment.rams_is_repairable = 0`) report MTTF instead (same ratio, stored in `reliability_kpi_snapshots.mttf`; `mtbf` is NULL).

Formula:
MTBF_hours = t_exp_hours / eligible_failure_count
Fleet: pooled ratio over the latest snapshot per equipment in the horizon whose data_quality_score ≥ 0.85 (`DQ_GREEN_THRESHOLD`):
Fleet_MTBF = Σ exposure_hours / Σ event_count (repairable equipment only). Never an average of per-asset MTBF values. See `reliability/fleet.rs`.

Inputs:
- `runtime_exposure_logs` / inferred exposure hours
- `failure_events` eligible for reliability denominators
- `reliability_kpi_snapshots.mtbf`

Required fields:
- exposure hours denominator
- eligible failure count
- `reliability_kpi_snapshots.mtbf`, `period_start`, `period_end`, `equipment_id`

Displayed:
- Deep RAMS KPI snapshots; Executive Dashboards fleet health strip (horizon average + vs-prior Δ + sparkline from snapshot dates); Top 5 bad-actor ranking by event_count with MTBF / availability / closed-WO cost

Status:
Partial

---

# KPI-034

Nom:
Mean Time To Repair (MTTR)

Module:
Reliability / RAMS & Analytics

Description:
Average active repair hours per repair-eligible failure event.

Formula:
MTTR_hours = sum(active_repair_hours) / repair_event_count
Fleet: Σ active_repair_hours_total / Σ repair_event_count over the latest qualified snapshot per equipment (pooled, not averaged).

Inputs:
- Repair duration on eligible failure / WO closeout events
- `reliability_kpi_snapshots.mttr`

Required fields:
- Active repair hours per event
- `reliability_kpi_snapshots.mttr`

Displayed:
- Deep RAMS KPI snapshots; planned Executive Dashboards (M1)

Status:
Partial

---

# KPI-035

Nom:
Operational Availability % (A_o)

Module:
Reliability / RAMS & Analytics

Description:
Operational availability A_o (PRD §6.10.2, MIL-HDBK-911 aligned): uptime over uptime plus all downtime, where downtime is the wall-clock equipment-down time of eligible failures and therefore already includes waiting / logistics / administrative delay incurred while down. `t_exp_hours` is governed operating exposure, i.e. uptime, so downtime is added to the denominator — it is never subtracted from exposure (the pre-v2 formula `(t_exp − downtime) / t_exp` double-counted it).

Inherent availability A_i is reported alongside (not under this id): A_i = t_exp / (t_exp + Σ active_repair_hours) — corrective repair time only, no logistics delay (equivalent to MTBF / (MTBF + MTTR)).

Formula:
A_o = t_exp_hours / (t_exp_hours + downtime_hours)
A_i = t_exp_hours / (t_exp_hours + active_repair_hours)
Fleet: Σ exposure_hours / (Σ exposure_hours + Σ downtime_hours_total) over the latest qualified snapshot per equipment (pooled).

Inputs:
- Exposure hours and downtime hours used by `reliability/compute.rs` (`algorithm_version = reliability_kpi_v2`)
- `reliability_kpi_snapshots.availability_operational` (also written to `availability` for existing consumers)
- `reliability_kpi_snapshots.availability_inherent`

Required fields:
- `t_exp_hours`, downtime hours, active repair hours
- `reliability_kpi_snapshots.availability_operational`, `availability_inherent`, `exposure_hours`, `downtime_hours_total`

Displayed:
- Deep RAMS KPI snapshots; planned Executive Dashboards (M1)

Status:
Partial

---

# KPI-036

Nom:
PM Adherence (Compliance) Rate

Module:
Preventive Maintenance / RAMS & Analytics

Description:
Share of PM occurrences completed on or before due within a horizon (or complementary overdue ratio from due-queue metrics). Hub executive KPI for planners and managers.

Formula:
PM_adherence_% = (COUNT occurrences completed on/before due in horizon
 / COUNT occurrences that were due in horizon) × 100
(Interim Partial: may surface overdue / due-today / next-7d counts from `get_pm_due_metrics` until on-time completion fields are queryable end-to-end)

Inputs:
- `pm_occurrences.due_at`, `status`, completion timestamps
- `get_pm_due_metrics` buckets

Required fields:
- `pm_occurrences.due_at`, `status`, completion / closed timestamps

Displayed:
- Executive Dashboards PM pipeline stacked bar (overdue / due today / next 7d counts); not a completion % until on-time fields are queryable

Status:
Partial

---

# KPI-037

Nom:
Maintenance Cost (period)

Module:
Work Orders / Finance / RAMS & Analytics

Description:
Sum of maintenance costs over a selected horizon, breakable by equipment and org node for cost distribution analytics.

Formula:
SUM(work_order / budget cost lines in period)
(Group by equipment_id or org_node_id for breakdowns)

Inputs:
- WO actual / plan cost fields
- Budget variance / cost center lines where applicable

Required fields:
- Queryable cost amounts on closed or costed WOs (and/or budget actuals)
- Equipment and org foreign keys for breakdowns

Displayed:
- Executive Dashboards (`/analytics/dashboards`) — closed WO `total_cost` on the fleet health strip and per-asset in the bad-actor table; org-scoped via WO `entity_id`
- ~~Vue d'ensemble Overview strip (`TOTAL_COST`)~~ — **removed 2026-10**: it summed closed WO
  `total_cost` instead of the finance settlement flow (`finance::settlement`); the home page shows
  no cost figure until one reads the settled actuals.

Semantic key:
— (`TOTAL_COST` removed from the Overview strip)

Permission:
`fin.view` (Executive Dashboards)

Default roles:
—

Trend sense:
up-bad

Default target:
Prior 30-day horizon total (not a numeric budget target)

Status:
Partial

---

## Overview strip (Vue d'ensemble)

Home Overview metric cards are driven by **semantic keys** persisted in `user_analytics_dashboard_layouts.overview_kpis` (namespaced beside Executive `widgets`). Missing `overview_kpis` → capability-based defaults below. Values are computed SSOT in Rust (`get_overview_kpis`); frontend does not recompute formulas.

| Semantic key | Catalog | Permission | Trend | Default roles |
|---|---|---|---|---|
| `DI_OPEN` | KPI-038 | `di.view` | up-bad | Supervisor, General |
| `OT_IN_PROGRESS` | KPI-039 | `ot.view` | up-bad | Storekeeper, General |
| `OT_OVERDUE` | KPI-040 | `ot.view` | up-bad | Supervisor, General |
| `MTTR` | KPI-034 | `rep.view` | up-bad | — |
| `PM_COMPLIANCE` | KPI-036 | `pm.view` | up-good | Supervisor |
| `PM_DUE_TODAY` | KPI-041 | `pm.view` | up-bad | General |
| `MAINT_BACKLOG` | KPI-042 | `ot.view` | up-bad | — |
| `AVAILABILITY` | KPI-035 | `rep.view` | up-good | Reliability, Supervisor |
| `MTBF` | KPI-033 | `rep.view` | up-good | Reliability |
| `CRITICAL_DOWN` | KPI-043 | `eq.view` | up-bad | — |
| `DATA_QUALITY` | KPI-044 | `rep.view` | up-good | Reliability |
| `STOCK_OUT` | KPI-045 | `inv.view` | up-bad | Storekeeper |
| `REORDER_ALERT` | KPI-046 | `inv.view` | up-bad | Storekeeper |

**Role default sets (first match):**

1. **Reliability** (`ram.view` or `rep.view`): MTBF, AVAILABILITY, DATA_QUALITY
2. **Storekeeper** (`inv.view`, not supervisor/reliability): STOCK_OUT, REORDER_ALERT, OT_IN_PROGRESS
3. **Supervisor** (`di.review` or `ot.close`): DI_OPEN, OT_OVERDUE, PM_COMPLIANCE, AVAILABILITY
4. **General** (else): DI_OPEN, OT_IN_PROGRESS, OT_OVERDUE, PM_DUE_TODAY (permission-filtered)

Drill targets: operational modules (`/requests`, `/work-orders`, `/pm`, `/inventory`, …) and Deep RAMS under `/analytics/rams/*` for fleet metrics.

---

# KPI-038

Nom:
Open Intervention Requests (DI)

Module:
Intervention Requests / Overview

Description:
Count of DIs in non-terminal operational states (open pipeline).

Formula:
COUNT intervention_requests WHERE status NOT IN (rejected, converted, closed non-executable, archived)

Inputs:
- `intervention_requests.status`, `created_at`

Required fields:
- DI status, created_at

Semantic key:
`DI_OPEN`

Permission:
`di.view`

Default roles:
Supervisor, General

Trend sense:
up-bad

Default target:
Count 7 days ago (same status filter)

Displayed:
- Vue d'ensemble Overview strip (`VisualMetricCard`)

Status:
Implemented

---

# KPI-039

Nom:
Work Orders In Progress

Module:
Work Orders / Overview

Description:
Count of WOs actively in execution (`in_progress` status only — not all open WOs).

Formula:
COUNT work_orders WHERE status.code = 'in_progress'

Inputs:
- `work_orders.status_id` → `work_order_statuses.code`

Required fields:
- WO status reference

Semantic key:
`OT_IN_PROGRESS`

Permission:
`ot.view`

Default roles:
Storekeeper, General

Trend sense:
up-bad

Default target:
Count with same filter at prior horizon (7d)

Displayed:
- Vue d'ensemble Overview strip

Status:
Implemented

---

# KPI-040

Nom:
Overdue Work Orders

Module:
Work Orders / Overview

Description:
Count of non-terminal WOs whose planned end is before now. **WO-only** — not combined DI+PM overdue.

Formula:
COUNT work_orders WHERE planned_end < now AND status NOT IN (closed, cancelled)

Inputs:
- `work_orders.planned_end`, status

Required fields:
- planned_end, status_id

Semantic key:
`OT_OVERDUE`

Permission:
`ot.view`

Default roles:
Supervisor, General

Trend sense:
up-bad

Default target:
Same query with prior-window proxy (7d)

Displayed:
- Vue d'ensemble Overview strip

Status:
Implemented

---

# KPI-041

Nom:
PM Due Today

Module:
Preventive Maintenance / Overview

Description:
Open PM occurrences due on the current calendar day.

Formula:
COUNT pm_occurrences WHERE status in active pipeline AND date(due_at) = date('now')

Inputs:
- `pm_occurrences.due_at`, `status`

Required fields:
- due_at, status

Semantic key:
`PM_DUE_TODAY`

Permission:
`pm.view`

Default roles:
General

Trend sense:
up-bad

Default target:
Prior-day bucket proxy

Displayed:
- Vue d'ensemble Overview strip

Status:
Implemented

---

# KPI-042

Nom:
Maintenance Backlog (planned hours)

Module:
Work Orders / Planning / Overview

Description:
Sum of planned/expected labor hours on open (non-terminal) work orders — backlog pressure proxy.

Formula:
SUM(expected_duration_hours) on open WOs

Inputs:
- `work_orders.expected_duration_hours`, status

Required fields:
- expected_duration_hours (Partial when many WOs lack plan hours)

Semantic key:
`MAINT_BACKLOG`

Permission:
`ot.view`

Default roles:
—

Trend sense:
up-bad

Default target:
Prior horizon sum (7d)

Displayed:
- Vue d'ensemble Overview strip (when enabled in Personnaliser)

Status:
Partial

---

# KPI-043

Nom:
Critical Equipment Down

Module:
Assets / Overview

Description:
Count of criticality-A (or equivalent) equipment in down / out-of-service lifecycle states.

Formula:
COUNT equipment WHERE criticality = critical AND lifecycle_status indicates down/out-of-service

Inputs:
- Equipment criticality reference, lifecycle_status

Required fields:
- criticality_code / ref, lifecycle_status

Semantic key:
`CRITICAL_DOWN`

Permission:
`eq.view`

Default roles:
—

Trend sense:
up-bad

Default target:
Prior count (7d)

Displayed:
- Vue d'ensemble Overview strip (when enabled)

Status:
Partial

---

# KPI-044

Nom:
Fleet Data Quality Score (avg)

Module:
Reliability / Overview

Description:
Average reliability snapshot data quality score over recent horizon.

Formula:
AVG(reliability_kpi_snapshots.data_quality_score) over horizon

Inputs:
- Snapshot rows in period

Required fields:
- data_quality_score on snapshots

Semantic key:
`DATA_QUALITY`

Permission:
`rep.view`

Default roles:
Reliability

Trend sense:
up-good

Default target:
Prior horizon average

Displayed:
- Vue d'ensemble Overview strip; drill `/analytics/rams/data-quality`

Status:
Partial

---

# KPI-045

Nom:
Stock-Out (critical articles)

Module:
Inventory / Overview

Description:
Count of active articles with zero or negative available quantity across stock balances.

Formula:
COUNT DISTINCT articles WHERE is_active AND available_qty <= 0

Inputs:
- stock_balances.available_qty, articles.is_active

Required fields:
- available_qty, article identity

Semantic key:
`STOCK_OUT`

Permission:
`inv.view`

Default roles:
Storekeeper

Trend sense:
up-bad

Default target:
Prior count

Displayed:
- Vue d'ensemble Overview strip

Status:
Partial

---

# KPI-046

Nom:
Reorder Alerts

Module:
Inventory / Overview

Description:
Count of article/warehouse combinations at or below reorder point requiring replenishment.

Formula:
COUNT evaluate_reorder recommendations (reorder_point or min_stock trigger)

Inputs:
- reorder_point, min_stock, available_qty

Required fields:
- Article stock parameters, balances

Semantic key:
`REORDER_ALERT`

Permission:
`inv.view`

Default roles:
Storekeeper

Trend sense:
up-bad

Default target:
Prior count

Displayed:
- Vue d'ensemble Overview strip

Status:
Partial

---

## Equipment registry strip (AssetRegistryPage)

Filter-scoped metrics for the Equipment page health/risk monitor. Layout (later) is separate from Overview `overview_kpis`. Values computed by `get_equipment_registry_metrics` under the active SmartFilterBar scope (class, family, status, org, search).

| Semantic key | Catalog | Permission | Viz | Trend |
|---|---|---|---|---|
| `EQUIP_STATUS_SUMMARY` | KPI-047 | `eq.view` | alert | up-bad |
| `EQUIP_CRITICAL_AT_RISK` | KPI-048 | `eq.view` | alert | up-bad |
| `EQUIP_HEALTH_SCORE` | KPI-049 | `eq.view` | gauge | up-good |
| `EQUIP_OPEN_WORK` | KPI-050 | `eq.view` | histogram | up-bad |
| `EQUIP_STALE_METERS` | KPI-051 | `eq.view` | alert | up-bad |

Registry SSOT: `shared/equipment-kpi-registry.ts` (not the Overview strip registry).

---

# KPI-047

Nom:
Equipment Status Summary (active vs down)

Module:
Assets / Equipment Registry

Description:
Within the current registry filter scope, count assets in active/in-service (or standby) states versus out-of-service / down states. Primary strip value is the down count for alert attention; active count is returned alongside for display.

Formula:
active_count = COUNT scoped WHERE status IN (IN_SERVICE, STANDBY, active_in_service, …)
down_count = COUNT scoped WHERE status IN (OUT_OF_SERVICE, down, out_of_service, inactive, …)
value = down_count

Inputs:
- equipment lifecycle / EQUIPMENT.STATUS reference
- SmartFilterBar scope

Required fields:
- lifecycle_status / equipment_status_ref_id
- deleted_at

Semantic key:
`EQUIP_STATUS_SUMMARY`

Permission:
`eq.view`

Default roles:
Equipment operators / planners with `eq.view`

Trend sense:
up-bad

Default target:
Prior 7d down count in same filter (when available)

Displayed:
- Equipment registry KPI strip (`VisualMetricCard`, alert viz)

Status:
Partial

---

# KPI-048

Nom:
Critical Assets at Risk

Module:
Assets / Equipment Registry

Description:
Count of criticality-A (or equivalent) assets in the filtered scope that are down/out-of-service OR have at least one open DI or open WO.

Formula:
COUNT distinct equipment WHERE criticality IN (A, critical, CRITICAL)
  AND (status is down/out-of-service OR EXISTS open DI OR EXISTS open WO)

Inputs:
- criticality reference
- status
- open intervention_requests / work_orders

Required fields:
- equipment_criticality_ref_id / criticality_value_id
- status
- DI/WO equipment_id + status

Semantic key:
`EQUIP_CRITICAL_AT_RISK`

Permission:
`eq.view`

Default roles:
—

Trend sense:
up-bad

Default target:
Prior 7d count

Displayed:
- Equipment registry KPI strip (alert / pulsing when > 0)

Status:
Partial

---

# KPI-049

Nom:
Average Asset Health Index

Module:
Assets / Equipment Registry

Description:
Mean composite health score (0–100) across filtered assets that have enough lifecycle/meter evidence to score — same formula family as `get_asset_health_score`.

Formula:
AVG(health_score) WHERE score IS NOT NULL over filtered scope (capped population for performance)

Inputs:
- lifecycle event recency
- meter last_read_at
- open DI/WO counts

Required fields:
- equipment_lifecycle_events, equipment_meters, DI/WO links

Semantic key:
`EQUIP_HEALTH_SCORE`

Permission:
`eq.view`

Default roles:
—

Trend sense:
up-good

Default target:
Prior horizon average when available

Displayed:
- Equipment registry KPI strip (gauge)

Status:
Partial

---

# KPI-050

Nom:
Assets with Active Work

Module:
Assets / Equipment Registry

Description:
Count of filtered assets that have at least one open DI and/or non-terminal WO.

Formula:
COUNT DISTINCT equipment_id with open DI OR open WO within filtered equipment set

Inputs:
- intervention_requests, work_orders, statuses

Required fields:
- equipment_id on DI/WO
- status codes

Semantic key:
`EQUIP_OPEN_WORK`

Permission:
`eq.view`

Default roles:
—

Trend sense:
up-bad

Default target:
Prior count / 30d daily series for histogram

Displayed:
- Equipment registry KPI strip (histogram)

Status:
Partial

---

# KPI-051

Nom:
Stale Counter Readings

Module:
Assets / Equipment Registry

Description:
Count of filtered assets that have at least one active meter with no reading or last_read_at older than the stale threshold (30 days), aligned with health meter recency bands.

Formula:
COUNT DISTINCT equipment WHERE EXISTS active meter AND (last_read_at IS NULL OR last_read_at < now - 30d)

Inputs:
- equipment_meters.is_active, last_read_at

Required fields:
- equipment_meters

Semantic key:
`EQUIP_STALE_METERS`

Permission:
`eq.view`

Default roles:
—

Trend sense:
up-bad

Default target:
Prior count

Displayed:
- Equipment registry KPI strip (amber alert)

Status:
Partial

