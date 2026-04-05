# Modules 6.23 and 6.25 Research

## Work Permits and Inspection Rounds as Control and Verification Workflows

### 1. Why These Two Modules Must Be Researched Together

Inspection rounds and work permits are different workflows, but they play a closely related role in a serious maintenance system.

Module 6.25 exists to:

- detect abnormal conditions early
- gather structured condition evidence
- confirm compliance with inspection routines
- trigger corrective follow-up when anomalies appear

Module 6.23 exists to:

- control hazardous maintenance work before it starts
- verify that required isolations, PPE, approvals, tests, and handovers are complete
- prevent unsafe execution on energized, pressurized, confined, hot-work, or otherwise dangerous jobs

Together they provide the control layer around maintenance execution:

- inspections discover risk, degradation, and non-conformance
- permits gate hazardous execution and safe return to service

That is why these modules should be researched together.

### 2. Research Base

This brief is based on:

- current Maintafox PRD sections 6.23 and 6.25
- ISO 45001 official summary for OH&S system principles
- UK HSE technical guidance on permit-to-work systems
- MaintainX procedure hub and preventive maintenance product guidance
- Fiix work order product page and Fiix guidance on using work orders for health, safety, and compliance
- UpKeep work order product guidance from the broader workflow research stream
- BS EN 13306 official summary for maintenance terminology discipline

### 3. What The Sources Show

Three conclusions stand out very clearly.

#### 3.1 Competitor CMMS Platforms Standardize Work Through Procedures, Templates, and Follow-Up Logic

MaintainX emphasizes:

- procedures for maintenance, safety, and operations
- inspection data capture with pre-filled templates, signatures, time tracking, and required fields
- failed inspections triggering corrective follow-up

Fiix emphasizes:

- inspection-type tasks inside work orders
- automated follow-up activities when inspections fail
- standard procedures, manuals, diagrams, and task libraries
- configurable work-order fields and mandatory information for compliant execution

UpKeep, through its broader work-order workflow model, also emphasizes configurable forms, controlled close-out, and mobile execution discipline.

Practical conclusion:

- serious systems standardize how inspections and safety-critical execution information are collected
- they do not rely on free-text memory alone

#### 3.2 True Permit-to-Work Control Is Stricter Than Generic Checklists

The HSE guidance is important because it describes permit-to-work as a formal control system for hazardous maintenance work, not merely a checklist attachment.

The HSE highlights that a real permit system must address:

- the type of work being done
- hazards before and during maintenance
- correct PPE and equipment
- authorization by responsible people
- isolation, draining, flushing, and environmental monitoring where needed
- training, supervision, and human factors
- communication during the work
- formal hand-back of the plant or equipment to operations
- filing, inspection, and review of permits

Practical conclusion:

- a serious PTW/LOTO module must model authorization, activation, suspension, revalidation, and hand-back explicitly
- it cannot be reduced to one approval and one close button

#### 3.3 Health and Safety Governance Must Be Systemic, Not Cosmetic

ISO 45001 emphasizes:

- hazard identification and risk assessment
- legal and regulatory compliance
- worker participation
- emergency planning
- auditing and review
- continual improvement using PDCA

Fiix reinforces the same operationally by stressing:

- safety steps at the start and end of task lists
- PPE in the work package
- photos, diagrams, and repair histories for safer execution
- structured completion notes and failure codes for safety and compliance evidence

Practical conclusion:

- both permits and inspections should feed a broader safety and compliance loop
- they are not isolated forms

### 4. The Correct Maintafox Operating Model

Maintafox should position these two modules as complementary control workflows.

#### 4.1 Inspection Rounds

Inspection rounds are the routine detection mechanism.

Their role is to answer:

- what was checked
- what condition was observed
- what value was measured
- whether the result was normal, abnormal, or borderline
- whether a follow-up DI or WO is required

#### 4.2 Work Permits

Work permits are the hazardous-work authorization mechanism.

Their role is to answer:

- what dangerous work is being authorized
- what hazards and energy sources are present
- what controls were applied before work starts
- who approved and who verified those controls
- whether conditions changed during work
- whether the plant was safely handed back after completion

### 5. Module 6.25 Research: Inspection Rounds & Checklists

### 5.1 Module Role In The Maintenance Operating Model

This module should not be treated as a digital replacement for paper rounds only.

It is the frontline anomaly-detection and condition-evidence system.

It should feed:

- intervention requests
- follow-up work orders
- PM review and frequency tuning
- condition history and trend analysis
- audit and compliance reporting

### 5.2 The Most Important Design Correction

The current PRD is already stronger than many lightweight checklist designs, but it still needs one important correction:

- an inspection round is not only a sequence of checkpoints
- it is also a structured evidence record with review consequences

That means the module must distinguish between:

- results collected
- anomalies detected
- corrective follow-up required
- review status of the round and of each anomaly

### 5.3 Recommended Maintafox Model

At minimum, this module should include:

- inspection templates and template versions
- route or round definitions
- scheduled inspection round instances
- checkpoint results with typed evidence
- anomaly records separate from raw results
- follow-up routing logic to DI, WO, or permit review where needed
- missed-round and late-round governance

### 5.4 Required Data Model Direction

Recommended data direction:

- `inspection_templates`: id, code, name, entity_id, route_scope, estimated_duration_minutes, is_active, current_version_id
- `inspection_template_versions`: id, template_id, version_no, effective_from, checkpoint_package_json, tolerance_rules_json, escalation_rules_json, requires_review
- `inspection_checkpoints`: id, template_version_id, sequence_order, asset_id, component_id, checkpoint_code, description, check_type, measurement_unit, normal_min, normal_max, warning_min, warning_max, requires_photo, requires_comment_on_exception
- `inspection_rounds`: id, template_id, template_version_id, scheduled_at, assigned_to_id, started_at, completed_at, reviewed_at, reviewed_by_id, status (scheduled/released/in_progress/completed/completed_with_findings/reviewed/missed/cancelled)
- `inspection_results`: id, round_id, checkpoint_id, result_status (pass/warning/fail/not_accessible/not_done), numeric_value, text_value, boolean_value, comment, recorded_at, recorded_by_id
- `inspection_evidence`: id, result_id, evidence_type (photo/file/reading_snapshot/signature), file_path_or_value, captured_at
- `inspection_anomalies`: id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, requires_permit_review, resolution_status

### 5.5 Required State Logic

Recommended round lifecycle:

1. Scheduled
2. Released
3. In Progress
4. Completed
5. Completed With Findings
6. Reviewed
7. Missed
8. Cancelled

Why this matters:

- a round with critical findings should not disappear into the same end state as a clean round
- reviewed status matters for audit and escalation follow-through

### 5.6 Required Evidence Rules

Maintafox should support:

- typed results, not only free text
- warning and failure thresholds, not only pass/fail
- required photo/comment rules for exceptions
- auto-generated anomaly records for abnormal readings
- anomaly review and follow-up routing rules
- duplicate suppression where the same persistent issue is repeatedly detected

### 5.7 Corrections Recommended For The Current PRD 6.25

1. Add template versioning so checkpoint rules can evolve without overwriting history.
2. Separate anomaly records from raw checkpoint results.
3. Add review status after completion, especially for rounds with findings.
4. Add warning thresholds in addition to fail thresholds where numeric trend monitoring matters.
5. Add explicit follow-up routing rules to DI, WO, or permit review.
6. Measure missed, late, reviewed, and exception-heavy rounds separately.

### 6. Module 6.23 Research: Work Permit System

### 6.1 Module Role In The Maintenance Operating Model

This module should be the formal hazard-control gate for dangerous work.

It should be used when work involves conditions such as:

- lockout or tagout
- hot work
- confined space entry
- energized electrical work
- chemical exposure
- work at height
- other tenant-defined high-risk tasks

Its purpose is not only to document risk but to block execution until required controls are active.

### 6.2 The Most Important Design Correction

The current PRD is stronger than a generic approval form, but it still needs a stricter lifecycle model.

The permit should distinguish clearly between:

- approved for issue
- issued to the field
- active under controlled conditions
- suspended because conditions changed
- revalidated after suspension or expiry
- closed technically
- handed back to operations safely

Without explicit hand-back and revalidation logic, the system will understate real permit risk.

### 6.3 Recommended Maintafox Model

At minimum, this module should include:

- permit type definitions and type-specific rules
- permit records with hazard and control context
- isolation points and control steps
- atmospheric or environmental test results where applicable
- approval, issue, activation, suspension, revalidation, close, and hand-back events
- witness and sign-off records
- expiry and reissue rules

### 6.4 Required Data Model Direction

Recommended data direction:

- `permit_types`: id, name, code, description, requires_hse_approval, requires_operations_approval, requires_atmospheric_test, max_duration_hours, mandatory_ppe_ids, mandatory_control_rules_json
- `work_permits`: id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, description, work_scope, status (draft/pending_review/approved/issued/active/suspended/revalidation_required/closed/handed_back/cancelled/expired), requested_by_id, issued_by_id, activated_by_id, suspended_by_id, handed_back_by_id, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at
- `permit_hazard_assessments`: id, permit_id, hazard_type, hazard_description, risk_level, control_measure, verification_required
- `permit_isolations`: id, permit_id, isolation_point, energy_type, isolation_method, applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at
- `permit_tests`: id, permit_id, test_type, result_value, unit, acceptable_min, acceptable_max, tested_by_id, tested_at, is_pass
- `permit_checkpoints`: id, permit_id, checkpoint_type, sequence_order, description, is_mandatory, is_completed, completed_by_id, completed_at, evidence_note
- `permit_suspensions`: id, permit_id, reason, suspended_by_id, suspended_at, reinstated_by_id, reinstated_at, reactivation_conditions
- `permit_handover_logs`: id, permit_id, handed_from_role, handed_to_role, confirmation_note, signed_at

### 6.5 Required State Logic

Recommended permit lifecycle:

1. Draft
2. Pending Review
3. Approved
4. Issued
5. Active
6. Suspended
7. Revalidation Required
8. Closed
9. Handed Back
10. Cancelled
11. Expired

Why this matters:

- approved does not mean safe to start
- closed does not necessarily mean the plant has been safely returned to operation
- suspension and revalidation are not optional details in hazardous work control

### 6.6 Required Control Rules

Maintafox should enforce:

- type-specific mandatory approvals
- mandatory hazard and control information
- mandatory isolation verification before activation where relevant
- mandatory test results before activation for applicable permit types
- automatic expiry handling with revalidation path
- work-order state gating so hazardous work cannot start without an active permit
- hand-back confirmation before final technical close of the permit chain

### 6.7 Corrections Recommended For The Current PRD 6.23

1. Add issued vs active distinction.
2. Add explicit revalidation-required state after suspension or expiry.
3. Add formal hand-back to operations after close.
4. Add separate hazard assessments, isolation records, and test records instead of storing all permit logic in one permit row.
5. Add stronger expiry, suspension, and witness controls.
6. Treat PTW as a work-order gate, not just a linked safety document.

### 7. Cross-Module Data Value

If Modules 6.23 and 6.25 are implemented this way, they become a major source for:

- safety and compliance evidence
- anomaly detection rate by asset and area
- repeated unsafe-condition trends
- permit usage by work type and hazard type
- permit expiry and suspension statistics
- inspection-to-DI conversion rate
- inspection-to-WO conversion rate
- hazardous-work planning bottlenecks
- audit readiness and traceability

### 8. Integration Expectations With The Rest Of Maintafox

These modules must integrate tightly with:

- 6.3 Equipment Asset Registry for asset and location context
- 6.4 Intervention Requests for anomaly escalation and unsafe-condition reporting
- 6.5 Work Orders for follow-up execution and hazardous-work gating
- 6.9 Preventive Maintenance Planning for recurring inspection strategy and PM findings
- 6.16 Planning & Scheduling for readiness blocking by permit status
- 6.20 Training, Certification & Habilitation for authorization and competency checks
- 6.21 IoT Integration Gateway where condition-based anomalies support inspection prioritization
- 6.24 Budget & Cost Center for inspection and permit workload cost traceability
- 6.26 Configuration Engine for permit types, inspection forms, threshold rules, and workflow controls

### 9. Bottom-Line Position For Maintafox

The biggest design mistake would be to build inspections as simple digital checklists and permits as ordinary approval forms.

Competitor systems already show the value of standardized procedures, inspection-triggered follow-up, mandatory fields, and mobile evidence capture. The safety standards and HSE guidance show that hazardous work control requires even stricter lifecycle discipline.

Maintafox should position these modules as:

- a structured anomaly-detection and field-verification system
- a formal hazardous-work authorization and hand-back system
- a safety and compliance evidence layer tightly connected to execution workflows

That is what makes them professionally credible.

### 10. Recommended PRD Upgrade Summary

For Module 6.25:

- add template versioning and reviewed status
- separate anomalies from raw results
- add stronger exception evidence rules and routing logic

For Module 6.23:

- strengthen the lifecycle with issued, active, revalidation, and handed-back states
- separate hazard, isolation, and test records
- treat permit activation as a hard gate on hazardous work execution

For both together:

- treat inspections and permits as control workflows around maintenance execution
- protect the data needed for compliance, safety, anomaly analysis, and audit from being turned into optional narrative only

### 11. Source Set

- ISO 45001:2018 official summary: https://www.iso.org/standard/63787.html
- UK HSE Permit to Work Systems: https://www.hse.gov.uk/comah/sragtech/techmeaspermit.htm
- MaintainX Procedure Hub: https://www.getmaintainx.com/procedures
- MaintainX Preventive Maintenance Software: https://www.getmaintainx.com/preventive-maintenance-software/
- Fiix Work Order Management Software: https://fiixsoftware.com/cmms/work-orders/
- Fiix A blueprint for improving health, safety, and compliance using work orders: https://fiixsoftware.com/blog/improving-health-safety-and-compliance-using-work-orders/
- UpKeep Work Order Software: https://upkeep.com/product/work-order-software/
- BS EN 13306:2017 official summary: https://knowledge.bsigroup.com/products/maintenance-maintenance-terminology