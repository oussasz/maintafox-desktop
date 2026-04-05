# Modules 6.9 and 6.16 Research

## Preventive Maintenance and Planning/Scheduling as One Planned-Work Pipeline

### 1. Why These Two Modules Must Be Researched Together

Preventive maintenance planning and planning/scheduling are not separate administrative conveniences. They are two linked layers of the same planned-work system.

Module 6.9 decides:

- which assets need planned work
- why that work exists
- what the interval or trigger should be
- which standard tasks, skills, tools, parts, and safety controls are expected

Module 6.16 decides:

- which work is ready to execute now
- when it should be done
- who should do it
- which work must wait because of missing parts, missing permits, production constraints, or skill gaps

If Maintafox treats PM as a recurring calendar generator and scheduling as a drag-and-drop calendar, the system will miss the real operational logic that determines whether planned maintenance is actually effective.

The correct model is this:

- 6.9 defines and continuously improves the planned maintenance strategy
- 6.16 turns ready work into an executable commitment against real capacity and real constraints
- together they produce the data needed for PM compliance, schedule adherence, backlog control, labor utilization, downtime prevention, and maintenance optimization

That is why these modules must be researched together.

### 2. Research Base

This brief is based on:

- current Maintafox PRD sections 6.9 and 6.16
- MaintainX official preventive maintenance product page, work order product page, and preventive maintenance scheduling guidance
- UpKeep official work order product page and meter-trigger documentation
- Fiix official work order product page, preventive maintenance audit article, and maintenance planning and scheduling best-practice article
- ISO 14224 official summary for reliability and maintenance data structure
- BS EN 13306 official summary for maintenance terminology discipline

### 3. What The Sources Show

Across the strongest sources, four patterns are consistent.

#### 3.1 Planning and Scheduling Are Different Functions

MaintainX explicitly distinguishes planning from scheduling in its PM scheduling guidance.

Its official guidance says planning determines what work should be done and how, while scheduling determines who does it and when.

Fiix says nearly the same thing in more operational language:

- planning identifies critical assets, frequencies, and required resources
- scheduling places the work into real time against actual operational conditions

This distinction matters because many weak systems merge these ideas into one calendar and then lose the ability to explain why work was created, why it was delayed, or whether the schedule itself was realistic.

#### 3.2 Serious PM Uses More Than One Trigger Type

MaintainX emphasizes three major schedule patterns:

- fixed schedules
- floating schedules
- meter-based schedules

MaintainX and UpKeep both explicitly support meter-based triggering.

UpKeep's official meter-trigger documentation shows that meter readings can create work orders directly when a threshold or cadence is reached, and those readings can be recorded again at work order close.

Fiix also emphasizes date-, time-, meter-, and event-based scheduling in its work order product material.

This means Maintafox should not reduce PM to a simple date-frequency table.

#### 3.3 Planned Work Must Be Resource-Aware

MaintainX highlights labor insights, smart time estimates, recurring work orders, and resource planning.

Fiix highlights:

- skill specialization
- estimated versus actual time
- seasonality and production shifts
- shutdown coordination
- criticality-based prioritization

UpKeep emphasizes calendar-based visibility, cost trends, time spent, preventive work assignment, and field execution tracking.

The common message is clear:

- planned work is not ready work merely because it is due
- readiness depends on people, skills, materials, windows, and operating constraints

#### 3.4 PM Strategy Must Be Closed-Loop and Evidence-Driven

MaintainX recommends tracking KPIs such as:

- MTBF
- planned maintenance percentage
- preventive maintenance compliance
- scheduled maintenance critical percent
- OEE

Fiix adds a more operational feedback model:

- use follow-up work after PM as a sign of PM value
- tune frequencies using PDCA rather than freezing them permanently
- audit PMs by criticality, specialization, backup asset availability, estimated vs actual duration, and impact of failure

This is essential for Maintafox.

The PM module should not only schedule work. It should learn from execution history and improve the maintenance strategy over time.

### 4. The Correct Maintafox Operating Model

Maintafox should model planned maintenance as a controlled pipeline with two linked layers.

#### 4.1 Layer A: PM Strategy and Work Definition

This layer defines:

- the asset or asset class covered
- the trigger logic
- the standard task package
- required parts, tools, skills, and safety notes
- business reason and criticality
- expected interval and acceptable tolerance

This layer should be versioned because PM strategies change over time.

#### 4.2 Layer B: Ready Backlog and Schedule Commitment

This layer determines:

- which due or candidate work items are truly ready
- which are blocked and why
- what enters the weekly or daily executable schedule
- how schedule changes are justified
- whether the organization actually follows the schedule it committed to

This layer should produce measurable schedule discipline, not just a moving calendar.

### 5. Module 6.9 Research: Preventive Maintenance Planning

### 5.1 Module Role In The Maintenance Operating Model

Module 6.9 should be the strategy and generation layer for planned maintenance.

It should answer:

- which assets need planned work
- which trigger logic applies
- which task package should be executed
- what constitutes compliance
- when the PM plan should be revised based on evidence

It should not be limited to storing a frequency and generating work orders blindly.

### 5.2 The Most Important Design Correction

The current PRD models PM mainly as plans, tasks, executions, and counters.

That is directionally useful, but it misses a critical distinction:

- a PM plan is a maintained strategy record
- a due PM occurrence is a generated obligation
- a work order is the execution record

Without separating those layers, Maintafox will struggle to explain:

- what was due but not generated
- what was generated but not scheduled
- what was scheduled but missed
- what was completed late or deferred
- whether the PM strategy itself is still valid

### 5.3 Recommended Maintafox Model

At minimum, Module 6.9 should include:

- PM master plans
- PM plan revisions and version history
- PM occurrence records for each due event
- trigger history for time-, meter-, event-, and condition-based creation
- PM task libraries and reusable job plans
- PM-required parts, tools, and skills
- PM deferral and miss logic
- PM findings and follow-up work linkage

### 5.4 Required Data Model Direction

Recommended data direction:

- `pm_plans`: id, code, title, asset_scope, strategy_type (time/floating/meter/event/condition), criticality_class, target_interval, tolerance_window, assigned_group_id, requires_shutdown, requires_permit, is_active, current_version_id
- `pm_plan_versions`: id, pm_plan_id, version_no, effective_from, trigger_definition_json, task_package_json, required_parts_json, required_skills_json, required_tools_json, estimated_duration_hours, change_reason
- `pm_occurrences`: id, pm_plan_id, plan_version_id, due_basis (calendar/meter/event/condition), due_at, due_meter_value, generated_at, status (forecasted/generated/ready/scheduled/in_progress/completed/deferred/missed/cancelled), linked_work_order_id, deferral_reason_id, missed_reason_id
- `pm_trigger_events`: id, pm_plan_id, trigger_type, source_reference, triggered_at, measured_value, threshold_value, was_generated
- `pm_executions`: id, pm_occurrence_id, work_order_id, execution_result (completed_no_findings/completed_with_findings/deferred/missed/cancelled), executed_at, completed_by_id, notes
- `pm_findings`: id, pm_execution_id, finding_type, severity, description, follow_up_di_id, follow_up_work_order_id
- `pm_counters`: id, equipment_id, counter_type, current_value, last_reset_at, unit

The important point is not the exact table names. The important point is that planned strategy, due occurrence, and execution evidence are distinct objects.

### 5.5 Required Trigger Logic

Maintafox should support at least:

- fixed calendar schedules
- floating schedules based on actual completion date
- meter-based schedules based on runtime, cycles, mileage, or other counters
- event-based triggers linked to inspections or other workflow events
- condition-based triggers linked to IoT thresholds where available

Each trigger type should produce an auditable generation event.

### 5.6 Required PM State Logic

PM plan lifecycle should be distinct from PM occurrence lifecycle.

Recommended PM plan lifecycle:

1. Draft
2. Proposed
3. Approved
4. Active
5. Suspended
6. Retired

Recommended PM occurrence lifecycle:

1. Forecasted
2. Generated
3. Ready For Scheduling
4. Scheduled
5. In Progress
6. Completed
7. Deferred
8. Missed
9. Cancelled

Why this matters:

- compliance depends on occurrences, not only on master plans
- strategy governance depends on plan revision history, not on the last work order alone

### 5.7 Required Evidence and Analytics

The PM module should capture enough structured information to support:

- PM compliance
- PMP
- overdue PM exposure
- missed PM rate
- follow-up corrective work rate after PM
- PM finding rate by asset and checkpoint type
- plan-to-actual duration variance for recurring work
- interval optimization decisions

Fiix's audit guidance is especially useful here. Maintafox should treat the following as optimization signals:

- how often PMs generate corrective follow-up work
- whether PMs on critical assets still precede failures
- whether PMs consistently exceed estimated duration
- whether specialized skills or contractor dependence make a PM hard to schedule
- whether the work can be done while equipment remains operational

### 5.8 Corrections Recommended For The Current PRD 6.9

1. Separate PM master plans from due PM occurrences.
2. Add versioned PM plan revisions instead of editing live strategy in place.
3. Add fixed, floating, meter-, event-, and condition-based trigger logic explicitly.
4. Add deferral, miss, and cancellation handling for PM occurrences.
5. Add findings and follow-up work linkage to evaluate PM effectiveness.
6. Treat PM optimization as a feedback loop driven by evidence, not only a rule-based calendar optimizer.

### 6. Module 6.16 Research: Planning & Scheduling Engine

### 6.1 Module Role In The Maintenance Operating Model

Module 6.16 should be the readiness, commitment, and schedule-discipline layer.

It should answer:

- which work is ready to enter the schedule
- which work is blocked and why
- which work was committed for the upcoming period
- what changed after commitment
- whether the organization executed what it planned

This module is not just a visual calendar. It is the operational control room for planned work.

### 6.2 The Most Important Design Correction

The current PRD emphasizes timeline visualization, drag-and-drop scheduling, and conflict detection.

Those are useful, but the section needs stronger planning discipline.

The scheduler must know the difference between:

- work that exists
- work that is ready
- work that is committed
- work that broke into the frozen schedule unexpectedly

Without that distinction, the calendar becomes descriptive instead of operationally controlling.

### 6.3 Recommended Maintafox Model

The planning engine should manage a readiness pipeline before calendar placement.

That means every candidate work item should carry structured readiness signals such as:

- parts ready or not ready
- skill coverage available or not available
- permit required and permit status
- shutdown or access window required
- prerequisite inspection or diagnostic step complete
- work package complete or incomplete

Only schedule-ready work should be committed into the short-term frozen schedule unless an authorized override is used.

### 6.4 Required Data Model Direction

Recommended data direction:

- `planning_windows`: id, entity_id, window_type (production_stop/maintenance_window/planned_shutdown/turnaround/public_holiday/access_window), start_datetime, end_datetime, description, is_locked
- `capacity_rules`: id, entity_id, team_id, effective_date, available_hours_per_day, max_overtime_hours_per_day, notes
- `schedule_candidates`: id, source_type (work_order/pm_occurrence/inspection_follow_up/project), source_id, readiness_status (not_ready/ready/committed/dispatched/completed), readiness_score, priority_id, required_skill_set_json, required_parts_ready (boolean), permit_status, shutdown_requirement, estimated_duration_hours
- `schedule_commitments`: id, schedule_period_start, schedule_period_end, source_type, source_id, committed_start, committed_end, assigned_team_id, assigned_personnel_id, frozen_at, committed_by_id
- `scheduling_conflicts`: id, reference_type, reference_id, conflict_type (no_qualified_technician/missing_critical_part/locked_window/skill_gap/double_booking/permit_missing/prerequisite_missing), detected_at, resolved_at, resolution_notes
- `schedule_change_log`: id, reference_type, reference_id, field_changed, old_value, new_value, changed_by_id, changed_at, reason_code, reason_note
- `schedule_break_ins`: id, schedule_commitment_id, break_in_reason (emergency/safety/production_loss/regulatory/other), approved_by_id, created_at

### 6.5 Required Scheduling Process Logic

Recommended scheduling logic:

1. collect candidate work from WO backlog, PM occurrences, inspection follow-ups, and other eligible modules
2. evaluate readiness and blocking conditions
3. commit schedule-ready work into the planning horizon
4. freeze the near-term schedule for adherence measurement
5. dispatch work and track deviations
6. record break-ins, moves, and reasons
7. measure schedule adherence and planning accuracy

This is closer to real maintenance scheduling practice than a continuously shifting calendar with no commitment point.

### 6.6 Required UX Requirements

#### Planner UX

- ready backlog board separate from blocked backlog
- workload and skill coverage view by team and person
- planning windows and shutdown overlay
- drag-and-drop only for schedule-ready work unless override is used
- visible readiness blockers with one-click drilldown
- weekly freeze action that snapshots the committed schedule

#### Supervisor UX

- monitor schedule adherence for the current period
- compare committed vs actual start times
- review break-in work and reschedule reasons
- see delayed work by blocker category

#### Technician UX

- clear daily dispatch list from the committed schedule
- visibility of prerequisite failures before field execution
- simple reason capture when work cannot start as planned

### 6.7 Required Metrics

This module should support at least:

- schedule adherence
- ready backlog size
- blocked backlog size by blocker type
- emergency break-in ratio
- committed versus completed work hours
- planning accuracy for duration estimates
- wrench time versus waiting time impact on planned work
- PM work completed on committed date

### 6.8 Corrections Recommended For The Current PRD 6.16

1. Add schedule readiness as a first-class concept, not only conflict detection after placement.
2. Add schedule commitment and freeze logic for short-term adherence measurement.
3. Add explicit blocker types for parts, permits, skills, shutdown windows, and missing prerequisites.
4. Add break-in work tracking so emergency work does not disappear inside rescheduling noise.
5. Separate ready backlog from blocked backlog in the planner experience.
6. Measure schedule discipline and planning accuracy, not only calendar occupancy.

### 7. Cross-Module Data Value

If Modules 6.9 and 6.16 are implemented this way, they become the main source for:

- PM compliance and overdue-risk reporting
- planned maintenance percentage
- schedule adherence and break-in work tracking
- backlog age and backlog readiness analysis
- critical-asset protection logic
- manpower and specialization bottleneck analysis
- shutdown preparation quality
- interval optimization based on findings and failures
- cost and labor forecasting for planned work

### 8. Integration Expectations With The Rest Of Maintafox

These modules must integrate tightly with:

- 6.3 Equipment Asset Registry for criticality, runtime, and history context
- 6.4 Intervention Requests for planned follow-up of screened demand where appropriate
- 6.5 Work Orders for execution evidence and actuals
- 6.8 Spare Parts & Inventory Management for part readiness and reservation
- 6.10 Reliability Engineering for interval tuning and failure feedback
- 6.20 Training, Certification & Habilitation for skill and authorization matching
- 6.21 IoT Integration Gateway for condition-based triggers and counters
- 6.23 Work Permit System for hazardous-work readiness gating
- 6.24 Budget & Cost Center for planned-work cost forecasting and variance
- 6.25 Inspection Rounds & Checklists for inspection-triggered PM review or follow-up work
- 6.26 Configuration Engine for trigger rules, field rules, workflow semantics, and planner layouts

### 9. Bottom-Line Position For Maintafox

The biggest design mistake would be to build PM as a recurring work-order generator and scheduling as a visual calendar.

Competitor systems already support more than that, and the best-practice materials are clear.

Maintafox should position these modules as:

- a versioned planned-maintenance strategy system
- a readiness-aware scheduling engine
- a closed-loop optimizer for PM intervals and work content
- a schedule-discipline layer that measures commitment, deviation, and break-in work

That is how planned maintenance becomes operationally useful and analytically defensible.

### 10. Recommended PRD Upgrade Summary

For Module 6.9:

- separate PM plans from PM occurrences
- add richer trigger types and trigger history
- add findings and follow-up work linkage
- add evidence-driven interval optimization

For Module 6.16:

- add readiness and blocked-work logic
- add schedule commitment and freeze concepts
- add break-in work tracking and reason capture
- add schedule-discipline metrics beyond visualization

For both together:

- treat planned work as a pipeline from strategy to due occurrence to ready backlog to committed schedule to executed work
- protect the data needed for compliance, backlog, labor, downtime-prevention, and reliability calculations from being configured away

### 11. Source Set

- MaintainX Preventive Maintenance Software: https://www.getmaintainx.com/preventive-maintenance-software/
- MaintainX Work Order Software: https://www.getmaintainx.com/work-order-software/
- MaintainX How to Make a Preventive Maintenance Schedule: https://www.getmaintainx.com/blog/preventive-maintenance-schedule
- UpKeep Work Order Software: https://upkeep.com/product/work-order-software/
- UpKeep How to Create Meter Work Order Triggers: https://help.onupkeep.com/en/articles/4730220-how-to-create-meter-work-order-triggers
- Fiix Work Order Management Software: https://fiixsoftware.com/cmms/work-orders/
- Fiix How to audit your preventive maintenance schedule and make the most of your team's time: https://fiixsoftware.com/blog/preventive-maintenance-audit-to-optimize-equipment-maintenance-program/
- Fiix How to plan and schedule work orders like the best maintenance teams: https://fiixsoftware.com/blog/maintenance-planning-and-scheduling-best-practices/
- ISO 14224:2016 official summary: https://www.iso.org/standard/64076.html
- BS EN 13306:2017 official summary: https://knowledge.bsigroup.com/products/maintenance-maintenance-terminology