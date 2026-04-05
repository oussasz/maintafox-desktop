# Modules 6.4 and 6.5 Research

## Intervention Requests and Work Orders as One Scientific Workflow

### 1. Why These Two Modules Must Be Researched Together

In a serious maintenance system, the request module and the work order module are not independent features. They are two stages of one operational chain:

1. a signal is raised
2. the signal is evaluated
3. authorized work is planned
4. work is executed
5. evidence is captured
6. the event enters maintenance history and analytics

If Maintafox treats intervention requests as a lightweight ticket form and work orders as a separate execution form, the system will miss the continuity needed for reliable metrics, root-cause analysis, and planning optimization.

The correct model is this:

- Module 6.4 captures demand, context, urgency, and triage evidence
- Module 6.5 captures planning, execution, resource consumption, technical findings, and closure evidence
- together they produce the data backbone for reliability, cost, backlog, SLA, and downtime calculations

That is why this research brief covers both modules together.

### 2. Research Base

This brief is based on:

- current Maintafox PRD sections 6.4 and 6.5
- IBM Maximo documentation on quick reporting, reporting actuals, and failure analysis
- MaintainX official help articles on work requests, request settings, work order creation, work order form fields, and completion
- UpKeep official help and learning content on work requests and work orders
- Fiix official help and product documentation on work request portals and work orders
- ISO 14224 official scope summary for reliability and maintenance data collection
- BS EN 13306 official scope summary for maintenance terminology

### 3. What The Sources Show

Across the strongest references, the pattern is consistent.

#### 3.1 Work Requests Are Intake And Triage Objects

MaintainX, UpKeep, and Fiix all separate the initial request from the actual work authorization.

Common competitor pattern:

- requesters and guests can submit issues without full maintenance access
- administrators or authorized reviewers evaluate the request
- the request can be approved, declined, or enriched before conversion
- when approved, the request becomes a work order and the original request is preserved as the origin record

This separation is important because it prevents every reported issue from immediately becoming an execution commitment.

#### 3.2 Work Orders Are Execution And Evidence Objects

IBM Maximo, MaintainX, UpKeep, and Fiix all treat work orders as records that capture much more than task title and assignee.

Common competitor pattern:

- planning information: due date, estimated duration, assigned resources, checklists, files, procedures
- execution information: status changes, start and stop timing, comments, attachments, progress updates
- resource information: labor, parts, tools, services, miscellaneous costs
- technical analysis information: failure codes, categories, maintenance type, condition, downtime, meter readings, inspection evidence

IBM Maximo is especially important here because it makes the reliability logic explicit: work orders can store structured failure codes, report actuals, report downtime, and then support failure analysis and MTBF review.

#### 3.3 Standards Emphasize Structured Reliability Language

ISO 14224 is formally targeted at petroleum, petrochemical, and natural gas industries, but its data philosophy is directly relevant to Maintafox. The official ISO summary emphasizes:

- standardized reliability and maintenance data collection
- minimum required categories of data
- structured equipment, failure, and maintenance data
- data quality control and assurance
- consistent terminology for exchanging and merging maintenance evidence

BS EN 13306 complements this by standardizing maintenance terminology across technical, administrative, and managerial maintenance contexts.

Practical meaning for Maintafox:

- the workflow should not rely on uncontrolled free text when a controlled classification is needed
- terms such as failure, maintenance action, downtime, corrective work, preventive work, and closure need consistent definitions
- the request-to-order lifecycle should produce structured fields that can support quantitative analysis later

### 4. The Correct Maintafox Operating Model

The right way to model these modules is as a controlled enrichment pipeline.

#### 4.1 Stage A: Intervention Request

Purpose:

- capture the existence of a problem, anomaly, need, or improvement demand
- preserve the original field signal
- support screening, prioritization, and approval
- avoid forcing technicians to invent execution details before planning exists

This module should answer:

- what was observed
- where and on what asset it was observed
- how severe or urgent it appears
- what the business, safety, and operational context is
- who reported it and when

#### 4.2 Stage B: Work Order

Purpose:

- authorize execution
- plan labor, materials, timing, and controls
- document what was actually done
- preserve actuals for cost, reliability, and performance analysis

This module should answer:

- what work was authorized
- who did it
- when it started and ended
- what resources were consumed
- what technical findings were confirmed
- whether the problem was actually resolved

### 5. Module 6.4 Research: Intervention Requests

### 5.1 Module Role In The Maintenance Operating Model

Intervention Requests are the formal intake gate for reactive and semi-reactive maintenance demand.

They should cover:

- operator reports
- supervisor escalation
- inspection findings
- PM-discovered anomalies
- safety or quality findings
- IoT-triggered events
- improvement requests that may later become planned work

This module is not the place to capture full execution history. Its job is to preserve the initial signal with enough structure for triage.

### 5.2 Research-Backed Operating Pattern

The competitor model is very clear:

- MaintainX supports requester users and public request portals
- UpKeep supports in-app requesters and public request portals, with admin review before approval
- Fiix supports guest request portals at tenant or site level

All three systems support a controlled conversion model where the request is reviewed, enriched, and then converted into a work order.

Maintafox should therefore model DI as a triage and authorization object, not as a miniature work order.

### 5.3 Required State Model

The current PRD has a good 11-state workflow, but the state logic should be tightened around data quality.

Recommended DI state logic:

1. Submitted
2. Pending Review
3. Returned for Clarification
4. Rejected
5. Screened
6. Awaiting Approval
7. Approved for Planning
8. Deferred
9. Converted to Work Order
10. Closed as Non-Executable
11. Archived

Why this is better:

- it distinguishes screening from approval
- it allows non-executable closure without pretending work occurred
- it makes conversion an explicit terminal outcome for the request record

The request should stop being an editable field-intake object after conversion, except for immutable linkage and commentary.

### 5.4 Required Data Model

The current PRD is directionally strong, but for scientific use the request entity needs stricter field design.

#### 5.4.1 Transactional Control Data

- request_id
- request_number
- reported_by
- reported_at
- current_state
- reviewer_id
- screened_at
- approved_at
- declined_at
- converted_to_work_order_id
- converted_at
- deferred_until
- assigned_review_team_id

#### 5.4.2 Operational Evidence Data

- asset_id
- sub-asset or component reference if known
- organization node where observed
- detection source: operator, supervisor, inspection, PM, IoT, quality, HSE, production
- observed symptom code
- narrative symptom description
- occurrence timestamp
- immediate impact on production, safety, quality, environment
- observed downtime started flag and optional downtime start timestamp
- photos, files, sensor snapshot, meter reading snapshot if relevant
- initial urgency and perceived criticality
- repeat issue flag if reporter recognizes recurrence

#### 5.4.3 Analytical Derivation Data

- request-to-review elapsed time
- review-to-approval elapsed time
- approval-to-conversion elapsed time
- SLA target and breach timestamps
- origin type distribution
- request quality score: complete vs incomplete intake
- request recurrence rate by asset and symptom

### 5.5 Required Fields By Stage

To support data quality without overloading the reporter, fields should become mandatory progressively.

#### At Submission

Mandatory:

- title or short issue statement
- detection source
- organization node or location
- asset if known, or location if asset unknown
- occurrence time or observed time
- priority estimate

Optional at submission but encouraged:

- attachment
- symptom code
- safety and production impact
- free-text observation

#### At Review

Mandatory before approval or rejection:

- reviewer decision
- reviewer note
- validated priority
- responsible planning group or review queue
- classification of request type

#### At Conversion

Mandatory before converting to work order:

- confirmed asset or location context
- maintenance type candidate
- approved execution path
- linkage to work order class or template if applicable

This is how competitor tools behave in practice: initial request data can be light, but the approver enriches it before execution.

### 5.6 UX Requirements

The DI experience should be intentionally simple for requesters and intentionally structured for reviewers.

#### Requester UX

- short, mobile-friendly intake form
- optional photo or file upload
- ability to scan QR on equipment to prefill asset and location
- clear status tracking after submission
- notifications on approval, rejection, status movement after conversion

#### Reviewer UX

- request queue by assigned review team
- split-pane detail with request history, attachments, asset context, recent similar requests, and open work on the same asset
- quick actions: reject, request clarification, defer, approve for planning, convert
- duplicate detection suggestions based on asset, symptom, and recent request window

#### Scientific Upgrade For Maintafox

The reviewer screen should also surface:

- recent failures on same asset
- current PM status of asset
- active permits or shutdown constraints if relevant
- last three related work orders and whether the issue recurred

This prevents low-quality triage decisions.

### 5.7 Configurability Rules

The administrator should be able to configure:

- request form required fields
- terminology for requests
- routing teams and queues
- origin lists, symptom lists, and impact lists
- SLA rules by priority, source, and asset class
- visible request fields by role
- public request portal behavior and QR-linked intake

The administrator should not be able to disable core timestamps or traceability fields required for analytics.

### 5.8 Corrections Recommended For The Current PRD

The current PRD should be strengthened in these areas:

1. Separate screening from approval.
2. Add explicit reviewer timestamps and queue ownership.
3. Add structured impact capture for production, safety, quality, and environment.
4. Add repeat-issue detection and duplicate request logic.
5. Make conversion lock the original request except for commentary.
6. Add QR-driven asset prefilling and recent-history context for reviewers.

### 6. Module 6.5 Research: Work Orders

### 6.1 Module Role In The Maintenance Operating Model

The work order is the formal authorization and evidence record for maintenance execution.

It is the single most important operational record in the system because it connects:

- asset history
- labor and craft effort
- spare-part usage
- downtime and availability impact
- root cause evidence
- permit and safety controls
- budget and cost accounting
- reliability analysis

If this record is incomplete or poorly structured, every downstream KPI becomes weak.

### 6.2 Research-Backed Operating Pattern

IBM Maximo provides the most mature model for Maintafox's intended direction.

IBM explicitly supports:

- reporting actual labor, materials, services, and tools
- recording downtime
- specifying meter readings
- classifying work orders
- recording failure codes from a failure hierarchy
- using work orders for failure analysis and MTBF review

MaintainX adds:

- structured form fields for asset, location, procedures, priority, parts, categories, vendors, and estimated time
- execution-time labor and incidental cost tracking
- mobile timer usage and mobile completion
- multi-asset work orders with sub-work-order progress

UpKeep adds:

- explicit connection between complete work orders and metrics like schedule compliance, MTBF, MTTR, OEE, and MRO spend
- emphasis on documenting time, parts, prerequisites, and checklist-driven work

Fiix adds:

- configurable work-order fields
- failure-code usage
- analytics on work-order impact, delay, compliance, and cost
- work request portal and scheduling integration

The design message is unambiguous: the work order must be both a planning object and a structured evidence object.

### 6.3 Required State Model

The current PRD 8-state workflow is a good base, but it should be refined to separate real execution states from administrative closure.

Recommended WO state model:

1. Draft
2. Awaiting Approval
3. Planned
4. Ready To Schedule
5. Assigned
6. Waiting For Prerequisite
7. In Progress
8. Paused
9. Mechanically Complete
10. Technically Verified
11. Closed
12. Cancelled

Why this matters:

- waiting for parts, permits, shutdown windows, or vendor attendance should not be hidden inside a generic hold state
- execution completion and technical verification are not always the same thing
- closure should occur only after final evidence and accounting checks are complete

### 6.4 Required Data Model

The current PRD captures many useful fields, but it needs a stronger evidence model.

#### 6.4.1 Transactional Control Data

- work_order_id
- source_request_id
- work_type
- current_state
- planner_id
- approver_id if required
- assigned_team_id
- primary_responsible_id
- planned_start
- planned_finish
- scheduled_at
- actual_start
- actual_finish
- mechanically_completed_at
- technically_verified_at
- closed_at

#### 6.4.2 Operational Evidence Data

- asset_id and functional position
- failure class, failure mode, failure cause, failure effect
- symptom confirmed yes or no
- work performed classification: corrective, preventive, inspection, improvement, emergency, condition-based
- temporary repair flag
- permanent repair flag
- downtime start and end
- production loss estimate or impact class
- labor entries by person, skill, start-stop or manual duration
- parts planned versus parts used
- services used
- tools required or issued
- permit links
- inspection and checklist execution results
- meter readings before and after if relevant
- attachments: photos, measurements, reports, receipts
- root cause summary
- corrective action summary
- verification method and return-to-service confirmation

#### 6.4.3 Analytical Derivation Data

- schedule compliance
- plan-to-actual duration variance
- wrench time ratio
- waiting time by cause
- downtime duration
- total maintenance cost
- labor productivity by craft or team
- repeat failure interval
- MTTR contribution
- cost by failure mode
- closure completeness score

### 6.5 Required Evidence Staging

Maintafox should not require all fields on day one of the work order. It should require the right fields at the right lifecycle stage.

#### At Draft or Planning

Mandatory:

- title
- asset or location context
- work type
- priority
- planning owner
- preliminary scope

Strongly recommended:

- estimated labor
- planned parts
- planned start and due dates
- checklist or procedure
- prerequisite flags: permit, shutdown, special skill, vendor, spare part

#### Before Assigning

Mandatory:

- responsible team or assignee
- planned execution window
- required parts reviewed
- required permits or safety constraints identified
- required competence or certification identified

#### Before Moving To In Progress

Mandatory:

- assignee confirmed
- prerequisite checks passed or waived by authorized role
- permit active if required
- lockout or safety gate confirmed where applicable
- baseline downtime state captured if the asset is already down

#### Before Mechanical Completion

Mandatory:

- labor actuals entered
- parts usage entered or explicitly marked none
- performed task steps updated
- technical findings entered
- failure coding entered for corrective work

#### Before Technical Verification and Closure

Mandatory:

- corrective action completed
- verification result entered
- downtime end entered if applicable
- closure comment entered
- root cause or cause-not-determined code entered for eligible work types
- cost actuals complete

This stage-gated approach is how Maintafox can stay usable while still generating high-quality data.

### 6.6 Work Order UX Requirements

#### Planner UX

- board and Gantt views for backlog and capacity
- split between unplanned backlog and scheduled work
- quick visibility of missing prerequisites
- drag-and-drop rescheduling with variance warnings
- template and job-plan support

#### Technician UX

- mobile-first execution screen
- start, pause, resume, finish controls
- timer with pause reasons
- checklist and procedure execution inline
- one-tap part consumption and attachment upload
- simple coding helpers for failure and cause selection

#### Supervisor UX

- monitor open work by status and blocking reason
- review closure completeness before final close
- verify repeat failures and reopen logic
- compare plan versus actual for labor, duration, and part usage

#### Scientific Upgrade For Maintafox

Maintafox should add a dedicated close-out panel with four structured sections:

1. confirmed symptom and observed condition
2. diagnosed failure mode and likely cause
3. action performed and whether it was temporary or permanent
4. verification of restoration and recurrence risk

This is the highest-value improvement for reliability and RCA quality.

### 6.7 Configurability Rules

The administrator should be able to configure:

- state names and workflow transitions within safe constraints
- required fields by work type and status
- failure taxonomies and maintenance categories
- priority and urgency scales
- close-out templates by work type
- checklist templates, job plans, and recurring-task logic
- views, queues, and role-specific dashboards

The administrator should not be able to remove the minimum core evidence fields required for reliability and cost metrics.

### 6.8 The Most Important Scientific Design Rule

Maintafox work orders must distinguish between:

- elapsed calendar duration
- active wrench time
- waiting time
- downtime

These are not interchangeable.

If the system only stores planned and actual end timestamps, then it cannot reliably calculate:

- wrench time ratio
- logistics delay
- permit delay
- schedule accuracy
- crew efficiency
- actual MTTR versus administrative elapsed time

Therefore the product should capture pause segments and pause reasons as structured records.

### 6.9 Corrections Recommended For The Current PRD

The current PRD should be strengthened in these areas:

1. Distinguish waiting-for-prerequisite from generic On Hold.
2. Add mechanical completion versus technical verification separation.
3. Add pause segments and structured delay reasons.
4. Expand failure capture from generic diagnosis and root cause text to coded symptom, mode, cause, and effect structure.
5. Add mandatory closure-quality rules.
6. Add downtime segment capture instead of only one aggregate downtime field.
7. Add verification and recurrence-risk fields at close-out.

### 7. Cross-Module Data Value

If Modules 6.4 and 6.5 are implemented this way, they become the primary source for:

- reliability metrics: MTBF, MTTR, repeat failure rate, failure-mode frequency
- execution metrics: wrench time, schedule compliance, backlog age, response time, completion time
- cost metrics: labor cost, parts cost, vendor cost, total event cost, cost by asset and failure mode
- planning metrics: delay causes, capacity mismatch, overdue work mix, emergency ratio
- HSE metrics: safety-related demand volume, permit-linked work count, unresolved safety requests
- quality metrics: closure completeness, uncoded failure rate, uncategorized demand rate

This is the exact reason these modules deserve top priority.

### 8. Integration Expectations With The Rest Of Maintafox

These modules must feed:

- 6.3 Equipment Asset Registry for maintenance history and health scoring
- 6.8 Spare Parts for planned versus actual consumption
- 6.9 Preventive Maintenance for PM-triggered work and feedback loop improvement
- 6.10 Reliability Engineering for failure-event analytics and Weibull or MTBF studies
- 6.16 Planning & Scheduling for capacity, queue, and conflict management
- 6.23 Work Permit for hazardous-work gating
- 6.24 Budget & Cost Center for financial roll-up
- 6.25 Inspection Rounds for anomaly-to-request conversion
- 6.26 Configuration Engine for workflow, required-field, and coding-set configurability

### 9. Bottom-Line Position For Maintafox

The biggest design mistake would be to build DI and WO as visually rich CRUD modules with incomplete evidence logic. Competitor systems already do more than that, and the standards direction is clear.

Maintafox should position these modules as:

- a controlled demand-to-execution pipeline
- a structured maintenance evidence system
- a reliability-ready history engine
- a configurable workflow with non-negotiable analytical discipline

That is how the platform becomes scientifically useful rather than just operationally convenient.

### 10. Recommended PRD Upgrade Summary

For Module 6.4:

- strengthen review, triage, and conversion logic
- add structured impact and recurrence fields
- add duplicate detection and better review context

For Module 6.5:

- strengthen execution-state granularity
- add structured time segmentation and delay coding
- add coded close-out evidence for failure and action analysis
- enforce minimum closure-quality requirements

For both together:

- treat request-to-order conversion as one traceable lifecycle
- protect the data required for KPI and reliability calculations from being made optional by configuration

### 11. Source Set

- IBM Maximo Quick Reporting: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=reporting-quick-overview
- IBM Maximo Reporting Actuals: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-reporting-actuals-work
- IBM Maximo Failure Analysis: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=overview-failure-analysis
- MaintainX Work Requests: https://help.getmaintainx.com/about-work-requests
- MaintainX Create a Work Request: https://help.getmaintainx.com/create-a-work-request
- MaintainX Work Request Settings: https://help.getmaintainx.com/work-request-settings
- MaintainX Create a Work Order: https://help.getmaintainx.com/create-a-work-order
- MaintainX Work Order Form Fields: https://help.getmaintainx.com/work-order-form-fields
- MaintainX Complete a Work Order: https://help.getmaintainx.com/complete-a-work-order
- UpKeep Work Requests: https://help.onupkeep.com/en/articles/4730330-how-to-create-and-manage-work-requests
- UpKeep Create Work Orders: https://help.onupkeep.com/en/articles/1746936-how-to-create-new-work-orders
- UpKeep Work Order Software: https://upkeep.com/product/work-order-software/
- UpKeep What Is a Work Order: https://upkeep.com/learning/work-order/
- Fiix About the Work Request Portal: https://helpdesk.fiixsoftware.com/hc/en-us/articles/360038454452-About-the-work-request-portal
- Fiix Enable the Work Request Portal: https://helpdesk.fiixsoftware.com/hc/en-us/articles/360038455092-Enable-the-work-request-portal
- Fiix Submit a Work Request: https://helpdesk.fiixsoftware.com/hc/en-us/articles/360038841971-Submit-a-work-request
- Fiix Create a Work Order: https://helpdesk.fiixsoftware.com/hc/en-us/articles/14562217303060-Create-a-work-order
- Fiix Work Order Management Software: https://fiixsoftware.com/cmms/work-orders/
- ISO 14224:2016 official summary: https://www.iso.org/standard/64076.html
- BS EN 13306:2017 official summary: https://knowledge.bsigroup.com/products/maintenance-maintenance-terminology