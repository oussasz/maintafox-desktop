# Module 6.24 Budget and Cost Center Management Research Brief

## 1. Research Position

This module should not be treated as a generic finance screen that happens to show maintenance spending.

In a serious maintenance platform, budget control is a governed cost-evidence layer that sits on top of operational execution. It must answer four practical questions:

- What budget baseline are we controlling against?
- What maintenance cost has actually been incurred, from which source, and when?
- What future maintenance cost is already committed or strongly forecast?
- Why is the organization over or under plan?

If the module only shows monthly totals, it will look useful but it will not support trustworthy variance analysis, cost-of-failure review, or ERP reconciliation.

## 2. Source Signals

### 2.1 IBM Maximo: actuals and budget monitoring

IBM Maximo documents two points that matter directly here.

First, work order actuals are not a single total. They are reported through structured subtabs for:

- labor
- materials
- services
- tools

That matters because Maintafox should preserve cost provenance instead of collapsing all execution spend into one undifferentiated amount.

Second, IBM budget monitoring is built around budget records in financial periods so organizations can monitor maintenance-related transactions against agreed budgets and improve the estimation of future projects. That supports a budget model based on:

- explicit budget records by period
- monitoring of maintenance transactions against those budgets
- estimation and re-estimation of future work

### 2.2 UpKeep: KPI focus, reporting dimensions, and cost perspective

UpKeep's maintenance KPI guidance reinforces three useful ideas:

- dashboards should focus on a limited number of decision-grade KPIs tied to business goals
- maintenance reporting should break down results by technician, team, asset, and location rather than only showing one global number
- maintenance cost must be interpreted in context with planned maintenance percentage, schedule compliance, downtime, and recurring failures

UpKeep also highlights maintenance cost as a percent of replacement asset value as a meaningful metric when the required asset-value context exists.

### 2.3 Maintafox cross-module evidence already established

Earlier Maintafox research in this session already established several prerequisites that 6.24 must build on:

- 6.5 Work Orders now requires labor actuals, parts actuals, downtime, and close-out quality gates
- 6.9 and 6.16 now provide PM occurrences, readiness, commitments, and planned-work forecast context
- 6.10 now treats cost-of-failure as a reliability decision input
- 6.23 and 6.25 now allow permit and inspection workload to be traced as governed execution effort

That means 6.24 should become the financial roll-up layer for evidence already captured elsewhere, not a standalone budgeting island.

## 3. Operational Purpose

The operational purpose of this module is to let maintenance leaders, planners, and controllers manage maintenance spend without forcing every user into the ERP.

In practice, it must support:

- annual and periodic maintenance budgeting
- separation of baseline budget, committed spend, posted actuals, and forecast spend
- cost review by cost center, entity, asset family, asset, and work category
- faster understanding of variance drivers such as emergency work, scope growth, price increases, or estimate error
- stronger linkage between maintenance strategy and financial outcome

## 4. Data Capture Requirements

The module must capture four classes of financial data.

### 4.1 Budget baseline data

- fiscal year and period
- version and scenario type
- cost center and optional reporting hierarchy
- budget bucket such as labor, parts, services, shutdown, contract, or capex
- amount, currency, and planning basis

### 4.2 Actual cost evidence

- source type such as work order labor, work order parts, services, tools, purchase receipt, or contract call-off
- source record reference for auditability
- asset, work order, or project context where relevant
- posting status and posting date

### 4.3 Commitment and forecast data

- approved purchase orders or reserved contracts that represent committed spend
- PM and shutdown demand that represent expected future spend
- forecast method and confidence so forecasts are explainable rather than magical

### 4.4 Variance review data

- variance amount and percent
- coded driver such as emergency break-in, vendor delay, labor overrun, estimate error, or scope change
- action owner and review status

## 5. Workflow Integrity

Budget control needs its own lifecycle. A realistic minimum model is:

Draft -> Submitted -> Approved -> Frozen
Frozen -> Reforecasted through a new approved version
Frozen or Approved -> Closed at fiscal year or period end

Key workflow rules:

- only one frozen control baseline should drive alerts for a given fiscal year and scenario
- manual adjustments must retain reason and approver
- actual cost posting should distinguish provisional values from posted values suitable for financial reporting or ERP handoff
- commitments must remain separate from posted actuals to avoid hiding future overruns

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- budget buckets
- fiscal period definitions
- threshold rules
- variance driver lists
- reporting hierarchies
- exchange-rate handling and display rules

The tenant administrator should not be able to break:

- source provenance of actual cost events
- link between actuals and the originating WO, PO, or contract event
- the distinction between baseline, committed, actual, and forecast values
- the minimum financial fields required for variance analysis and ERP reconciliation

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.2 and 6.26 for admin-defined cost-center attachment and reporting scope rules
- 6.5 Work Orders for labor, parts, services, tools, downtime, and close-out quality
- 6.8 Inventory and Purchasing for issue, receipt, and replenishment cost visibility
- 6.9 and 6.16 for PM-occurrence forecasts, shutdown packages, ready backlog demand, and committed work
- 6.10 Reliability for cost-of-failure, chronic-asset review, and action prioritization
- 6.11 Analytics for budget, variance, and spend-mix dashboards
- 6.22 ERP Connector for cost-center master data import and official posting export
- 6.23 and 6.25 for permit and inspection workload cost traceability where that effort rolls into maintenance spend

## 8. Bottom-Line Position For Maintafox

The design mistake would be to make 6.24 a thin reporting page with monthly totals and threshold alerts.

Maintafox should position this module as:

- a governed maintenance budget baseline system
- a provenance-preserving roll-up of actual maintenance cost events
- a commitment and forecast view for future spend
- a variance-management workspace tied to operational causes

That is what makes the module useful to supervisors, planners, reliability engineers, and controllers at the same time.

## 9. Recommended PRD Upgrade Summary

- add versioned budget baselines and reforecast capability
- separate budget lines, actual events, commitments, and forecast lines
- preserve cost provenance by source type instead of storing only aggregate totals
- track provisional versus posted actuals
- add variance-review workflow with coded drivers and accountable owner
- connect planned-work forecasts from PM, shutdown, and backlog layers
- strengthen ERP alignment while keeping local maintenance users out of unnecessary accounting complexity

## 10. Source Set

- IBM Maximo Reporting Actuals for Work Orders: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-reporting-actuals-work
- IBM Maximo Monitoring Maintenance Budgets: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=managing-monitoring-maintenance-budgets
- UpKeep Maintenance Metrics: https://upkeep.com/learning/maintenance-metrics/
- UpKeep Analytics and Reporting: https://upkeep.com/product/analytics-reporting/