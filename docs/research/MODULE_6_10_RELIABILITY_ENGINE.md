# Module 6.10 Research

## Reliability Engineering Engine as a Data-Governed Decision Layer

### 1. Why This Module Must Be Defined After Planned-Work Logic

The reliability engine cannot be designed credibly in isolation.

It sits on top of the execution pipeline already researched:

- DI and WO provide failure, downtime, labor, parts, delay, and close-out evidence
- PM and Planning provide planned-work logic, PM findings, interval changes, readiness constraints, and compliance history
- Inspection Rounds provide anomaly and condition evidence
- Work Permits provide safety and hazardous-work context where failure consequences are operationally important

If those upstream workflows are weak, the reliability engine becomes a charting layer with fragile numbers.

The correct approach is the reverse:

- first define the evidence chain
- then define the reliability layer that consumes, qualifies, and interprets that evidence

That is why this module belongs immediately after the planned-work logic.

### 2. Research Base

This brief is based on:

- current Maintafox PRD section 6.10
- IBM Maximo documentation on failure analysis and reporting actuals for work orders
- MaintainX preventive maintenance and work order product guidance
- Fiix guidance on reliability-centered maintenance and work-order-based analytics
- UpKeep work-order and asset-operations guidance from the broader workflow research stream
- ISO 14224 official summary for reliability and maintenance data collection
- BS EN 13306 official summary for maintenance terminology discipline

### 3. What The Sources Show

Four conclusions matter most.

#### 3.1 Reliability Starts With Structured Failure and Actuals Data

IBM Maximo is especially important here.

Its documentation shows that reliability analysis depends on:

- structured failure codes and failure hierarchies
- work-order-based failure recording
- reporting actual labor, materials, services, and tools
- downtime reporting
- correlation of failure histories with PM schedules to reduce future failures

Practical conclusion:

- a reliability module is only as good as the structure of the operational evidence feeding it

#### 3.2 Competitor Platforms Expose Reliability Outcomes, Not Only Raw Math

MaintainX emphasizes:

- PM compliance
- MTBF
- MTTR-related improvement outcomes
- failure causes and asset work history as inputs to better maintenance decisions

Fiix emphasizes:

- RCM as a strategy selection framework
- criticality-based prioritization
- failure mode and consequence analysis
- maintenance tactic selection based on failure behavior and feasibility

Practical conclusion:

- users do not need a disconnected lab tool first
- they need an operational reliability layer that converts work history into decisions

#### 3.3 Reliability-Centered Maintenance Is a Decision Discipline, Not a Dashboard

Fiix's RCM guidance is useful because it frames reliability properly.

It emphasizes:

- preserving system function
- identifying failure modes and their causes
- prioritizing failure modes by consequence
- selecting the most appropriate tactic for each failure mode
- reviewing and renewing those decisions with new information

Practical conclusion:

- the reliability engine should not stop at computing indicators
- it must also support action selection and PM optimization

#### 3.4 The Current PRD Is Directionally Ambitious but Too Broad for a Realistic First Reliability Layer

The current PRD includes:

- MTBF, MTTR, MTTF, availability, Weibull
- FMECA
- FTA
- RBD
- RCM
- ETA
- Bow-Tie/LOPA with SIL determination
- Markov analysis

This is intellectually interesting, but it creates a risk:

- the product may try to emulate specialist reliability suites before it has fully reliable input data and a phased rollout model

Practical conclusion:

- Maintafox needs a staged reliability architecture
- core reliability should come first, advanced probabilistic modeling later

### 4. The Correct Maintafox Position

Maintafox should treat 6.10 as a data-governed decision layer with phased maturity.

#### Layer A: Reliability-Ready Data Foundation

This layer standardizes:

- failure coding
- failure event eligibility rules
- downtime definitions
- runtime and exposure data
- actual labor and material capture
- planned vs unplanned event distinction

#### Layer B: Core Reliability Analytics

This layer provides:

- MTBF
- MTTR
- availability
- bad-actor identification
- failure-mode frequency
- repeat failure analysis
- Pareto and trend reporting

#### Layer C: Decision Support

This layer provides:

- FMECA
- RCM logic
- PM interval review
- tactic recommendation support
- action tracking back into PM and WO modules

#### Layer D: Advanced Modeling

This layer may later provide:

- Weibull and survival analysis
- FTA
- RBD
- optional Bow-Tie / LOPA or other advanced risk models for high-maturity or enterprise contexts

The key point is that the product should mature upward from reliable evidence, not downward from impressive formulas.

### 5. The Most Important Design Correction

The current PRD starts from techniques.

The corrected design should start from governed reliability data.

In practice, that means the module must first answer:

- which work orders count as failure events
- which downtime counts for MTTR and availability
- how runtime or exposure is measured
- how failure class, mode, cause, and effect are coded
- how planned work, condition findings, and actual repair evidence feed back into reliability decisions

Without this layer, later analytics are hard to defend.

### 6. Recommended Maintafox Model

At minimum, Module 6.10 should include:

- controlled failure hierarchies
- governed failure-event generation from work orders and inspections
- runtime and meter history
- KPI calculation rules with inclusion and exclusion logic
- reliability dashboards and bad-actor analysis
- FMECA and RCM workspaces linked to live asset history
- PM feedback loop to update strategies based on findings and failures
- optional advanced analysis only where data sufficiency exists

### 7. Required Data Model Direction

Recommended data direction:

- `failure_hierarchies`: id, name, asset_scope, version_no, is_active
- `failure_codes`: id, hierarchy_id, parent_id, code, label, code_type (class/mode/cause/effect/remedy), is_active
- `failure_events`: id, source_type (work_order/inspection/condition_alert/manual), source_id, equipment_id, component_id, detected_at, failed_at, restored_at, downtime_duration_hours, active_repair_hours, waiting_hours, is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, cause_not_determined, production_impact_level, safety_impact_level, recorded_by_id, verification_status
- `runtime_exposure_logs`: id, equipment_id, exposure_type (hours/cycles/output_distance), value, recorded_at, source_type
- `reliability_kpi_snapshots`: id, equipment_id, asset_group_id, period_start, period_end, mtbf, mttr, availability, failure_rate, repeat_failure_rate, event_count, data_quality_score
- `fmeca_analyses`: id, equipment_id, title, boundary_definition, created_at, created_by_id, status
- `fmeca_items`: id, analysis_id, component_id, functional_failure, failure_mode_id, failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control, linked_pm_plan_id, linked_work_order_id, revised_rpn
- `rcm_studies`: id, equipment_id, title, created_at, created_by_id, status
- `rcm_decisions`: id, study_id, function_description, functional_failure, failure_mode_id, consequence_category, selected_tactic (condition_based/time_based/failure_finding/run_to_failure/redesign), justification, review_due_at

### 8. Required Calculation Governance

Maintafox should not compute reliability metrics blindly.

It should govern:

- planned vs unplanned event inclusion
- partial vs full downtime handling
- cause-not-determined treatment
- minimum event count before certain analytics are displayed confidently
- confidence or data-quality warnings where sample size is weak
- asset-boundary definitions for system vs component analysis

This is what makes reliability results professionally credible.

### 9. Required Feature Direction

For a realistic and high-value first release, Maintafox should prioritize:

- failure code administration and governed mapping from WO close-out
- MTBF, MTTR, availability, failure-rate, and repeat-failure reporting
- bad-actor ranking by failure count, downtime, cost, and recurrence
- trend views linking failures to PM history and inspection findings
- FMECA with action tracking into PM plans and work orders
- RCM decision support linked to actual history and tactic outcomes
- Weibull analysis only where the event set is sufficient and well-qualified

Advanced models such as full FTA, RBD, Bow-Tie/LOPA, and Markov analysis should be treated as a later maturity tier unless the product can guarantee the required modeling discipline and user need.

### 10. Corrections Recommended For The Current PRD 6.10

1. Reframe the module from a bundle of techniques to a staged reliability architecture.
2. Strengthen failure-event governance and failure-code hierarchy design.
3. Add explicit data-quality and event-eligibility rules for KPI calculations.
4. Link FMECA and RCM outputs directly to PM plans and WOs as actionable outcomes.
5. Treat advanced probabilistic methods as maturity-tier capabilities, not all mandatory first-wave scope.
6. Keep the reliability engine tightly coupled to workflow evidence instead of allowing manual analytical drift.

### 11. Cross-Module Data Value

If Module 6.10 is implemented this way, it becomes the main source for:

- bad-actor analysis
- MTBF and MTTR tracking
- failure-mode prioritization
- PM interval tuning
- repair-vs-redesign decisions
- reliability trend reporting by asset class or site
- evidence-based capital planning and lifecycle discussions

### 12. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.3 Equipment Asset Registry for asset hierarchy, criticality, and technical boundaries
- 6.4 and 6.5 for structured failure, downtime, action, and actuals evidence
- 6.9 and 6.16 for PM performance, interval tuning, and planned-work feedback
- 6.21 IoT Integration Gateway for condition and exposure inputs
- 6.24 Budget & Cost Center for cost-of-failure and lifecycle views
- 6.25 Inspection Rounds for anomaly trend and early-warning evidence
- 6.26 Configuration Engine for failure hierarchies, metric rules, and analytical views

### 13. Bottom-Line Position For Maintafox

The biggest design mistake would be to present a mathematically impressive reliability engine built on weak operational evidence.

Maintafox should instead position 6.10 as:

- a reliability-ready data governance layer
- a core reliability analytics engine
- a decision-support workspace for FMECA and RCM
- a phased path toward more advanced modeling where the data and maturity justify it

That position is more technically credible and more useful to real maintenance teams.

### 14. Source Set

- IBM Maximo Failure Analysis: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=overview-failure-analysis
- IBM Maximo Reporting Actuals for Work Orders: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-reporting-actuals-work
- MaintainX Preventive Maintenance Software: https://www.getmaintainx.com/preventive-maintenance-software/
- MaintainX Work Order Software: https://www.getmaintainx.com/work-order-software/
- Fiix Reliability Centered Maintenance: https://fiixsoftware.com/maintenance-strategies/reliability-centered-maintenance/
- Fiix Work Order Management Software: https://fiixsoftware.com/cmms/work-orders/
- UpKeep Work Order Software: https://upkeep.com/product/work-order-software/
- ISO 14224:2016 official summary: https://www.iso.org/standard/64076.html
- BS EN 13306:2017 official summary: https://knowledge.bsigroup.com/products/maintenance-maintenance-terminology