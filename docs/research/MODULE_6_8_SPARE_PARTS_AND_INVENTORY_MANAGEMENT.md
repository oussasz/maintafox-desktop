# Module 6.8 Spare Parts and Inventory Management Research Brief

## 1. Research Position

This module should not be treated as a stock list with reorder alerts.

In a serious maintenance platform, inventory is the governed material-evidence layer for planned work, emergency response, repairable-part loops, procurement, and cost provenance.

If Maintafox only tracks quantity on hand and purchase orders, it will still fail the operational questions that matter:

- was a part reserved before work was committed
- which issue and return transactions belong to which work record
- which stock movement has already been posted externally
- whether a repairable spare was repaired, scrapped, or reinstalled

## 2. Source Signals

### 2.1 Work execution and planning already require stronger material governance

Maintafox now depends on 6.8 for:

- 6.5 planned parts, actual parts, and close-out evidence
- 6.9 and 6.16 readiness blocking when critical materials are unavailable
- 6.24 cost provenance from material issue, receipt, and reservation events
- 6.22 procurement and stock-transaction handoff to ERP

### 2.2 Reference governance matters here too

The 6.13 research already established that item families, storage locations, units, VAT codes, and supplier semantics are governed domains. Inventory quality depends on that backbone.

## 3. Operational Purpose

The operational purpose of this module is to:

- govern spare-part identity, stock state, and location traceability
- reserve and issue material against real work demand
- manage procurement, receipt, and repairable-part loops
- preserve cost and posting provenance for budgeting and ERP reconciliation

## 4. Data Capture Requirements

The module should capture six classes of stock-governance data.

### 4.1 Item-master data

- stocking type, criticality, reorder policy, and preferred warehouse

### 4.2 Location and balance data

- on-hand, reserved, on-order, quarantine, and last-counted state

### 4.3 Reservation and issue data

- demand source, reserved quantity, issued quantity, and release state

### 4.4 Procurement and receipt data

- requisition, purchase order, goods receipt, supplier, and external posting status

### 4.5 Count and adjustment data

- count session, variance, approval, and reason trail

### 4.6 Repairable-part loop data

- removed, shipped for repair, returned, scrapped, or reinstalled status

## 5. Workflow Integrity

Recommended stock flow:

Forecast or demand -> Reserve -> Pick or Issue -> Return or Consume -> Post and Reconcile

Recommended procurement flow:

Recommendation -> Requisition -> Order -> Receipt -> Inspect -> Release to Stock

Key workflow rules:

- committed work should not silently consume parts that were never reserved or issued
- returns, reversals, and count adjustments must remain explicit stock events, not silent balance edits
- repairable spares should preserve their own cycle history instead of disappearing into generic stock adjustments
- ERP posting status should be visible so local stock evidence and official external postings are not confused

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- reorder policy and critical-spare logic
- warehouse and location structure
- supplier preferences and procurement routes
- count frequency and safety-control rules

The tenant administrator should not be able to:

- bypass transaction provenance for issue, receipt, transfer, and adjustment
- hide shortages on committed work by editing balances directly
- erase historical stock and procurement movements already used for costing or audit

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.5 for planned and actual part consumption
- 6.9 and 6.16 for material-readiness blocking and forecast demand
- 6.13 for item and location reference governance
- 6.22 for procurement, supplier, and stock-posting exchange
- 6.24 for material cost provenance and commitment visibility
- 6.25 where inspection results create material follow-up demand

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.8 as shelves, quantities, and a PO list.

Maintafox should position this module as:

- a governed spare-parts and stock-state system
- a material-readiness control layer for execution planning
- a procurement and repairable-parts evidence service

## 9. Recommended PRD Upgrade Summary

- add reservation, issue, receipt, count, and repair-cycle governance
- distinguish local operational movements from external posted transactions
- strengthen replenishment logic with planned-work demand and lead-time risk
- preserve material provenance for costing, scheduling, and audit

## 10. Source Set

- Maintafox research brief: MODULES_6_4_6_5_REQUEST_TO_WORK_ORDER_LIFECYCLE.md
- Maintafox research brief: MODULES_6_9_6_16_PREVENTIVE_MAINTENANCE_AND_PLANNING_SCHEDULING.md
- Maintafox research brief: MODULE_6_13_LOOKUP_REFERENCE_DATA_MANAGER.md
- Maintafox research brief: MODULE_6_24_BUDGET_AND_COST_CENTER_MANAGEMENT.md
- Maintafox research brief: MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md
