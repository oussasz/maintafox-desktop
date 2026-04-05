# MAINTAFOX DESKTOP
## Product Requirements Document (PRD) - v3.0
### Classification: Internal - Engineering & Product Team

---

> **Document Owner:** Product & Architecture Division  
> **Version:** 3.0  
> **Date:** March 31, 2026  
> **Status:** APPROVED FOR DEVELOPMENT - v3.0 Technical Review  

---

## TABLE OF CONTENTS

1. [Executive Summary](#1-executive-summary)
2. [Product Vision & Strategic Objectives](#2-product-vision--strategic-objectives)
3. [System Architecture Overview](#3-system-architecture-overview)
4. [Technology Stack](#4-technology-stack)
5. [Responsibility Split: Local App vs. VPS](#5-responsibility-split-local-app-vs-vps)
6. [Core Modules](#6-core-modules)
  - 6.1 Authentication & Session Management
  - 6.2 Organization & Site Management
  - 6.3 Equipment Asset Registry
  - 6.4 Intervention Requests (DI)
  - 6.5 Work Orders (OT)
  - 6.6 Personnel Management
  - 6.7 Users, Roles & Permissions (RBAC)
  - 6.8 Spare Parts & Inventory
  - 6.9 Preventive Maintenance Planning
  - 6.10 Reliability Engineering Engine (RAMS/RCM)
  - 6.11 Analytics & Dashboard
  - 6.12 Archive Explorer
  - 6.13 Lookup / Reference Data Manager
  - 6.14 Notification System
  - 6.15 In-App Documentation & Support Center
  - 6.16 Planning & Scheduling Engine
  - 6.17 Activity Feed & Operational Audit Log
   - 6.18 Application Settings & Configuration Center
   - 6.19 User Profile & Self-Service
   - 6.20 Training, Certification & Habilitation Management
   - 6.21 IoT Integration Gateway
   - 6.22 ERP & External Systems Connector
   - 6.23 Work Permit System (LOTO / Permit-to-Work)
   - 6.24 Budget & Cost Center Management
   - 6.25 Inspection Rounds & Checklists
   - 6.26 Configuration Engine & Tenant Customization
7. [Database Architecture](#7-database-architecture)
8. [Sync Layer - Hybrid Cloud](#8-sync-layer--hybrid-cloud)
9. [Reliability Engineering Computation Engine](#9-reliability-engineering-computation-engine)
10. [Licensing & Subscription Control System](#10-licensing--subscription-control-system)
11. [Automatic Update System](#11-automatic-update-system)
12. [Security Architecture](#12-security-architecture)
13. [UI/UX Design Guidelines](#13-uiux-design-guidelines)
14. [Non-Functional Requirements](#14-non-functional-requirements)
15. [Delivery Phases & Milestones](#15-delivery-phases--milestones)
16. [VPS Infrastructure Specification](#16-vps-infrastructure-specification)
17. [Appendices](#17-appendices)

---

## 1. EXECUTIVE SUMMARY

Maintafox Desktop is a local-first maintenance operations platform for industrial teams that need governed execution, offline resilience, and evidence-grade maintenance history without turning day-to-day work into a browser-dependent SaaS workflow.

The product combines:

- local desktop execution for requests, work orders, planning, permits, inventory, training, inspections, analytics, and reliability workflows
- governed operational data needed for MTBF, MTTR, downtime, backlog, SLA, cost, and readiness analysis
- centralized vendor-operated control-plane services for entitlement, update rollout, synchronization coordination, and fleet administration
- an admin-defined but protected operating model so each tenant can adapt structure, workflows, terminology, and views without destroying analytical meaning

Strategically, Maintafox is positioned as an industrial maintenance operating system rather than a packaged web CMMS. The product's differentiator is not only feature breadth. It is the combination of local operational continuity, explicit trust boundaries, auditable workflow control, and progressive analytical maturity.

---

## 2. PRODUCT VISION & STRATEGIC OBJECTIVES

### 2.1 Vision Statement

> *"Give industrial maintenance teams a local-first operating system for execution, planning, compliance, and reliability so they can work safely and intelligently on their own hardware, with or without connectivity, without sacrificing auditability or analytical rigor."*

### 2.2 Strategic Objectives

1. **Operational continuity:** core maintenance execution remains usable on trusted devices during network outages.
2. **Governed execution:** work, approvals, permits, and readiness constraints are enforced through configurable but protected workflow rules.
3. **Evidence-grade history:** the product preserves the structured data needed for backlog, cost, compliance, and reliability analytics.
4. **Progressive intelligence:** analytics and advanced RAMS capabilities are introduced only when the underlying data quality and maturity justify them.
5. **Secure enterprise control:** centralized entitlement, update, and sync coordination are available without making the VPS the primary runtime dependency.
6. **Adaptability without drift:** tenants can configure language, structure, fields, states, and layouts within analytical and audit guardrails.
7. **Multilingual by design:** the application is architected for professional multi-language support from the first implementation phase, with French as the primary launch language and English supported through the same governed locale model rather than as an afterthought.

### 2.3 Target Customers And Deployment Pattern

Maintafox is designed for maintenance-heavy industrial organizations such as manufacturing plants, utilities, processing facilities, logistics sites, and multi-site industrial groups. The platform must serve:

- single-site operators that primarily need local resilience and strong workflow control
- professionalized maintenance teams that need planning, inventory, qualification, and compliance coordination
- enterprise customers that need multi-machine sync, controlled rollout, ERP or IoT interfaces, and central vendor support visibility

Default deployment posture:

- the desktop application is the operational runtime
- the VPS is the control plane and mirror, not the day-to-day execution dependency
- enterprise services extend the desktop product; they do not replace local operational authority

### 2.4 Commercial Model

Maintafox is sold as a subscription product with signed entitlements controlled from the VPS. Commercial packaging is expressed through feature flags, trusted-device limits, update channels, support posture, and enterprise integration scope rather than through separate codebases.

---

## 3. SYSTEM ARCHITECTURE OVERVIEW

Maintafox uses a layered local-first architecture with explicit authority and trust boundaries.

| Layer | Responsibilities | Key Constraints |
|---|---|---|
| **React Presentation Layer** | Screens, forms, dashboards, charts, and workspace composition | No implicit access to filesystem, shell, process, or raw secrets |
| **Rust Application Core** | Business orchestration, IPC commands, background jobs, policy enforcement, export logic | Trusted boundary for privileged actions and local system access |
| **Local Data Plane** | SQLite or SQLCipher data, staging tables, caches, analytical snapshots, audit evidence | Primary operational source of truth for day-to-day work |
| **Local Security Plane** | OS-managed secrets, trusted-device material, local session keying, installation master secret | Secrets do not rely solely on database storage |
| **Background Worker Plane** | Sync, updater checks, notification delivery, long-running analytics, backup helpers | Runs inside controlled Rust tasks with observable status |
| **VPS Control Plane** | Entitlements, update manifests, sync coordination, tenant mirror, vendor admin operations | Central authority for commercial controls and vendor-managed coordination |

Architectural rules:

1. The desktop application is the authoritative runtime for operational workflows.
2. The VPS coordinates, mirrors, and governs; it does not replace local execution.
3. WebView code and Rust core code are separate trust domains joined only through narrow, typed IPC.
4. No local web server is exposed as the primary application boundary.
5. Configuration, workflow, and audit history are version-aware and do not rely on silent reinterpretation of past records.
6. Heavy analytics and exports run in controlled background tasks so UI responsiveness and auditability are preserved.

---

## 4. TECHNOLOGY STACK

### 4.1 Frontend

| Technology | Version | Role |
|---|---|---|
| **React** | 18.x | Primary UI framework for desktop workspaces and shared component architecture |
| **TypeScript** | 5.x | Type-safe frontend and IPC contract definitions |
| **Tailwind CSS** | 3.x | Tokenized utility styling for dense, configurable operational surfaces |
| **Shadcn/ui + Radix UI** | Current | Accessible primitives for dialogs, sheets, forms, menus, and command surfaces |
| **TanStack Table** | 8.x | High-density industrial list and grid behavior |
| **D3.js** | 7.x | Reliability, planning, hierarchy, and analytical visualization layer |
| **React Hook Form + Zod** | Current | Typed form state, validation, and schema-driven input enforcement |

### 4.2 Desktop Shell And Core Runtime

| Technology | Role |
|---|---|
| **Tauri 2.x** | Cross-platform desktop shell with capabilities-based security model |
| **Rust** | Trusted application core, background task runtime, file operations, exports, and computation host |
| **Tokio** | Async runtime for sync, updater, notification, and background computation workloads |
| **Serde** | Serialization for typed IPC, sync envelopes, and cached computational outputs |
| **tracing** | Structured diagnostics and support logging |

### 4.3 Local Data Plane

| Technology | Role |
|---|---|
| **SQLite 3.x** | Embedded transactional operational database in WAL mode |
| **SQLCipher 4.x** | Encryption for local database contents where enabled by policy or packaging |
| **sea-orm** | Primary Rust ORM for entity definitions, migrations, and common query paths |
| **sqlx** | Lower-level query layer for complex SQL and verification-sensitive access patterns |
| **SQLite FTS5** | Local full-text search for archive, documentation, reference, and evidence-heavy modules |

### 4.4 VPS And Enterprise Services

| Technology | Role |
|---|---|
| **PostgreSQL 16** | Control-plane database and tenant mirror store |
| **Fastify / Node.js** | VPS API layer for sync, entitlements, admin, relay, and updater endpoints |
| **Redis** | Coordination cache, queue support, and short-lived policy or rollout state |
| **Nginx** | Reverse proxy, TLS termination, routing for API and admin console |
| **Docker Compose** | Service deployment and lifecycle control on the VPS |
| **S3-compatible storage / MinIO** | Update bundles, backups, and permitted mirror object storage |

---

## 5. RESPONSIBILITY SPLIT: LOCAL APP VS. VPS

| Domain | Local Desktop Authority | VPS Authority | Notes |
|---|---|---|---|
| **Operational transactions** | Create, edit, progress, validate, and close governed local records | Mirror and coordinate only | Local data remains authoritative until synchronized |
| **User interaction and workflow enforcement** | Immediate UI, validation, state gating, and offline policy enforcement | Policy refresh and administrative constraints | Day-to-day operations do not require round-trip connectivity |
| **Entitlement and machine trust** | Cache last valid entitlement and trusted-device state | Issue, revoke, suspend, and update entitlement policy | Commercial authority lives on the VPS |
| **Software update availability** | Check channel, present notes, install validated bundle | Publish manifests, signatures, rollout cohorts, and recall rules | Updater trust is centrally governed |
| **Cross-machine sync** | Produce outbox, apply inbound changes, resolve local review tasks | Coordinate checkpoints, mirror state, and accept idempotent batches | Sync is coordination, not primary runtime |
| **Enterprise relay functions** | Collect and package local outbound data | Receive, relay, and expose external-system contracts | ERP, IoT, report relay, and email relay are VPS-managed services |
| **Vendor operations** | Expose diagnostics and support bundles when allowed | Operate `console.maintafox.systems`, rollout control, and fleet monitoring | Vendor console is not bundled into the desktop app |

External systems such as ERP, document repositories, or telemetry platforms remain authoritative for their own master domains. Maintafox stores governed references, mappings, and synchronized snapshots where required for local execution and analytics.

---

## 6. CORE MODULES

The Maintafox core module set defines the operational product. Modules are configurable, but they are not structurally free-form; the product protects the minimum evidence required for traceability, analytics, and controlled execution.

### 6.1 Authentication & Session Management

**Objective:** Provide secure, offline-capable authentication for shared industrial workstations without collapsing first login, device trust, idle unlock, and sensitive-action reauthentication into one mechanism.

**Identity modes:**

- local Maintafox account with password
- enterprise SSO account via SAML 2.0 or OpenID Connect
- PIN or biometric fast unlock only after prior full authentication on the device

**Operating model:**

1. First login on a device must be online and creates a trusted-device record.
2. Refresh tokens and device-bound secrets are stored in OS-managed secure storage.
3. Offline sign-in is allowed only for previously trusted users on previously trusted devices inside the configured offline grace window.
4. Local lock and unlock are distinct from full sign-in and do not bypass policy enforcement.
5. Sensitive actions require step-up reauthentication based on tenant policy.

**Session layers:**

| Layer | Purpose |
|---|---|
| Authenticated account identity | Who the user is and which tenant or scope applies |
| Short-lived local access session | Active application session for normal use |
| Renewable refresh or trust grant | Silent renewal while policy permits |
| Idle unlock state | Quick return to a locked session without reloading the full desktop context |

**Shared workstation rules:**

- switch-user clears decrypted in-memory state before presenting the next identity prompt
- cached data is isolated by tenant and user context
- users who never completed a trusted online bootstrap cannot enter offline on a shared device
- unsynchronized changes are surfaced before logout or user switch

**Administrative controls:**

- idle timeout
- absolute session maximum
- offline grace duration
- password-only or password-plus-fast-unlock policy
- SSO enabled, local login enabled, or hybrid identity mode
- sensitive-action reauthentication rules

---

### 6.2 Organization & Site Management

**Objective:** Model the tenant's operating structure as governed data rather than a fixed hierarchy hardcoded into the product.

Maintafox treats this module as the operating backbone for routing, ownership, planning scope, KPI aggregation, and structural analytics.

**Required capabilities:**

- tenant-defined node types
- allowed parent-child relationship rules
- versioned structure models with effective dating
- node capability flags such as can host assets, can own work, can receive permits, can carry cost center, and can aggregate KPIs
- named responsibility bindings such as maintenance owner, production owner, HSE owner, planner, and approver
- impact preview before major structural change, deactivation, or reassignment

**Design rules:**

1. Physical location and responsibility structure must not be assumed to be identical.
2. Historical records keep their original structural meaning even after renames, moves, or deactivations.
3. The tenant can configure node types and relationships, but cannot publish invalid structures that strand governed records.
4. Structural changes are versioned and auditable.

**Representative data model direction:**

- `org_structure_models`
- `org_node_types`
- `org_type_relationship_rules`
- `org_nodes`
- `org_node_responsibilities`
- `org_entity_bindings`

---

### 6.3 Equipment Asset Registry

**Objective:** Serve as the governed asset-identity and lifecycle-history backbone for work execution, planning, reliability, cost, telemetry, and ERP handoff.

This module is not a flat equipment list. It preserves the maintainable boundary and the historical trace required for correct analytics.

**Required asset domains:**

- master identity: code, class, family, manufacturer, serial, commissioning state
- hierarchy and maintainable boundary: parent-child relationships, installed components, functional position
- technical and commercial context: warranty, supplier, replacement value, external IDs, dossier links
- meter and condition context: counters, primary meters, IoT or imported condition bindings
- lifecycle history: move, install, replace, reclassify, preserve, and decommission events
- cross-system linkage: ERP, document repository, telemetry, and inspection context

**Governance rules:**

- assets referenced by historical work, cost, permit, or reliability records are never hard-deleted
- replacement and movement retain before-and-after provenance
- reclassification is version-safe so historical analysis does not silently change meaning
- governed reference domains control classes, statuses, criticality, and family semantics

**Permissions:** `eq.view` / `eq.manage` / `eq.import`

---

### 6.4 Intervention Requests (DI - Demandes d'Intervention)

**Objective:** Capture maintenance demand as a triage object that preserves the original field signal, supports review and approval, and converts governed demand into executable work without losing traceability.

**Scope of intake:**

- operator and technician reports
- inspection findings
- PM-detected anomalies
- HSE, quality, or production escalations
- IoT or external-system triggered alerts that require human review

**Required state model:**

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

**Stage-gated data capture:**

- submission requires the minimum valid intake context
- review requires validated priority, queue ownership, and triage decision
- conversion requires confirmed asset or location context, request classification, and approved execution path

**Data-quality rules:**

- the request remains the immutable origin record once converted
- request-to-review, review-to-approval, and approval-to-conversion timings are preserved for SLA and backlog analysis
- photos, sensor snapshots, and free text support triage, but controlled classifications are used where analytics require structured evidence

---

### 6.5 Work Orders (OT - Ordres de Travail)

**Objective:** Manage authorized maintenance work as a planning, execution, and close-out record that produces reliable evidence for cost, downtime, performance, and reliability analytics.

**Data Entities:**
- `work_order_types`: id, code, label (Corrective / Preventive / Improvement / Inspection / Emergency / Overhaul / Condition-Based)
- `work_order_statuses`: id, code, label, color, macro_state, is_terminal, is_system
- `urgency_levels`: id, level (1-5), label (Faible -> Critique), hex_color
- `delay_reason_codes`: id, code, label, category (parts/permit/shutdown/vendor/labor/access/diagnosis/other), is_active
- `work_orders`: id, code (WOR-XXXX), type_id, status_id, equipment_id, component_id (nullable), location_id (nullable), requester_id, source_di_id, entity_id, planner_id, approver_id, assigned_group_id, primary_responsible_id, urgency_id, title, description, planned_start, planned_end, scheduled_at, actual_start, actual_end, mechanically_completed_at, technically_verified_at, closed_at, expected_duration_hours, actual_duration_hours, active_labor_hours, total_waiting_hours, downtime_hours, labor_cost, parts_cost, service_cost, total_cost, recurrence_risk_level, production_impact_id, root_cause_summary, corrective_action_summary, verification_method, notes
- `work_order_interveners`: id, work_order_id, intervener_id, skill_id, started_at, ended_at, hours_worked, hourly_rate, notes
- `work_order_parts`: id, work_order_id, article_id, quantity_planned, quantity_used, unit_cost, stock_location_id
- `work_order_tasks`: id, work_order_id, task_description, sequence_order, estimated_minutes, is_mandatory, is_completed, completed_by_id, completed_at, result_code, notes
- `work_order_delay_segments`: id, work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id
- `work_order_downtime_segments`: id, work_order_id, started_at, ended_at, downtime_type (full/partial/standby/quality_loss), comment
- `work_order_failure_details`: id, work_order_id, symptom_id, failure_mode_id, failure_cause_id, failure_effect_id, is_temporary_repair, is_permanent_repair, cause_not_determined, notes
- `work_order_attachments`: id, work_order_id, file_path, file_name, uploaded_by_id, uploaded_at
- `work_order_verifications`: id, work_order_id, verified_by_id, verified_at, result (pass/fail/monitor), return_to_service_confirmed, recurrence_risk_level, notes

**12-State Default Workflow:**
```
Draft -> Awaiting Approval -> Planned -> Ready To Schedule -> Assigned -> Waiting For Prerequisite -> In Progress -> Paused -> Mechanically Complete -> Technically Verified -> Closed
Any pre-close state -> Cancelled (authorized reason required)
```

**Features:**
- **Three view modes:** Table / Kanban board / Calendar (D3-powered timeline)
- **Gantt-style planning view:** shows all WOs on a timeline with drag-to-reschedule, backlog segmentation, and capacity conflict warnings
- **Planning panel:** assign technicians and groups, reserve parts, attach procedures or job plans, define checklist tasks, and expose missing prerequisites such as permits, shutdown windows, vendor attendance, required skills, or unavailable spares before assignment
- **Stage-gated data quality rules:** planning, assignment, start, mechanical completion, technical verification, and closure each enforce the minimum required fields for that transition
- **Time segmentation:** distinguish active labor time, waiting time, and downtime; pausing work requires a structured delay reason so MTTR, wrench time, and logistics-delay analytics remain reliable
- Automatic duration tracking: timer starts on `In Progress`, pause and resume events create delay segments, and downtime segments are recorded separately from elapsed calendar time
- Cost accumulation: labor (hours x rate) + parts consumed + service cost = total maintenance cost; plan-versus-actual variance shown for duration, labor hours, parts, and cost
- **Technician execution workspace:** mobile-friendly start, pause, resume, and finish controls; inline checklist execution; one-tap part consumption; photo and measurement attachments; simple failure-coding helpers for symptom, mode, cause, and effect
- **Structured close-out panel:** captures confirmed symptom and observed condition, diagnosed failure mode and likely cause, action performed (temporary or permanent), and verification of restoration plus recurrence risk
- **Mandatory closure quality gate:** a work order cannot close until labor actuals are entered, parts actuals are entered or explicitly marked none, corrective work has coded failure details or cause-not-determined recorded, downtime segments are closed if applicable, and verification result is captured
- **Backlog heatmap:** D3 weekly workload chart showing overloaded days, blocked work, and delayed work by reason category
- PDF work order print sheet: full technical sheet ready for field technician, including prerequisites, checklist, and QR reference
- **Closed WO analytics feed:** all close-out evidence feeds reliability engineering, repeat-failure detection, schedule-compliance reporting, and cost-by-failure-mode analysis
- **Reopen logic:** supervisors can reopen recently closed work within a configurable recurrence window while preserving the original close-out evidence and verification history

**Permissions:** `ot.view` / `ot.create` / `ot.edit` / `ot.delete`

---

### 6.6 Personnel Management

**Objective:** Govern the maintenance workforce as a work-readiness and labor-capacity registry, not just an HR roster. The module must show who is available, qualified, authorized, costed, and assignable for real work at any moment.

**Data Entities:**
- `positions`: id, code, name, category (technician/supervisor/engineer/operator/contractor/planner/storekeeper/hse), requirement_profile_id (nullable)
- `schedule_classes`: id, name, shift_pattern_code, is_continuous, nominal_hours_per_day
- `schedule_details`: id, schedule_class_id, day_of_week, shift_start, shift_end, is_rest_day
- `personnel`: id, employee_code, full_name, employment_type (employee/contractor/temp/vendor), position_id, primary_entity_id, primary_team_id, supervisor_id, home_schedule_id, availability_status (available/assigned/in_training/on_leave/blocked/inactive), hire_date, termination_date, email, phone, photo_path, hr_external_id, notes
- `personnel_skills`: id, personnel_id, skill_id, proficiency_level (1-5), verification_status (self_declared/validated/expired), last_validated_at
- `personnel_team_assignments`: id, personnel_id, team_id, entity_id, assignment_role (technician/planner/supervisor/specialist), valid_from, valid_to
- `personnel_availability_blocks`: id, personnel_id, block_type (leave/training/medical/restriction/borrowed/manual_hold), starts_at, ends_at, source_reference, approved_by_id
- `personnel_rate_cards`: id, personnel_id, effective_from, labor_rate, overtime_rate, cost_center_id, source_type (hr/manual/vendor_contract)
- `personnel_authorizations`: id, personnel_id, authorization_type (permit_issuer/isolation_authority/inspector/warehouse_signoff), valid_from, valid_to, source_certification_type_id
- `external_companies`: id, name, service_domain, contract_start, contract_end, onboarding_status, insurance_status, notes
- `external_company_contacts`: id, company_id, contact_name, contact_role, phone, email

**Features:**

**Workforce Identity & Capacity:**
- Every person is linked to a position, primary entity, team, schedule, and supervisor so planning and reporting use explicit workforce structure instead of informal labels
- Internal employees and contractor personnel can both be modeled, but contractor onboarding, contract validity, and vendor context remain visible at assignment time
- Personnel cards expose workload, next available window, active work count, and labor-rate context where permitted

**Availability, Skills & Work Readiness:**
- Availability is calculated from schedule, active assignments, training sessions, leave, manual holds, and other `personnel_availability_blocks`; a person can be active in the organization yet still not be assignment-ready
- Skills matrix and readiness views combine skill proficiency, 6.20 qualification status, and 6.23 permit authorizations so planners see real coverage gaps instead of only headcount
- Certification and authorization overlays distinguish valid, expiring, expired, suspended, and missing readiness states on the personnel card and team board

**Contractor & Vendor Labor Governance:**
- External companies hold onboarding, insurance, contract-expiry, and service-domain context so vendor labor can be planned and audited without collapsing it into employee data
- Contractor resources can appear in crews, work history, and labor-cost rollups while still preserving company context for procurement and compliance review

**Planning, Cost & Succession Views:**
- 6.16 consumes team assignments, availability blocks, and skills as scheduling constraints instead of relying on static availability flags
- 6.24 can price labor using governed rate cards, while succession and single-point-of-failure views highlight teams where one specialist holds critical knowledge or authorization
- HRMS imports may update controlled identity fields, but maintenance-specific fields such as skills, notes, local photo, and readiness context remain protected from blind overwrite

**Permissions:** `per.view` / `per.manage` / `per.report`

---

### 6.7 Users, Roles & Permissions (RBAC)

**Objective:** Govern scoped authorization, delegated administration, and dangerous-action control across the full product. This module must answer not only what a user can do, but where they can do it, under which role, and whether extra authentication is required.

**Data Entities:**
- `roles`: id, name, description, is_system (system roles cannot be deleted), role_type (system/custom), status (draft/active/retired)
- `permissions`: id, name (dot-notation: `domain.action.scope`), description, category, is_dangerous, requires_step_up (boolean)
- `role_permissions`: role_id, permission_id
- `user_accounts`: id, username, identity_mode (local/sso/hybrid), personnel_id, is_active, force_password_change, last_seen_at
- `user_scope_assignments`: id, user_id, role_id, scope_type (tenant/entity/site/team/org_node), scope_reference, valid_from, valid_to
- `permission_dependencies`: id, permission_name, required_permission_name, dependency_type (hard/warn)
- `role_templates`: id, name, description, module_set_json, is_system
- `delegated_admin_policies`: id, admin_role_id, managed_scope_type, managed_scope_reference, allowed_domains_json, requires_step_up_for_publish

**Permission Domains:**
| Domain | Covers |
|---|---|
| `eq.*` | Equipment module |
| `di.*` | Intervention Requests |
| `ot.*` | Work Orders |
| `org.*` | Organization/entities |
| `per.*` | Personnel |
| `ref.*` | Reference data / lookups |
| `inv.*` | Inventory / spare parts |
| `pm.*` | Preventive maintenance |
| `ram.*` | RAMS / Reliability module |
| `rep.*` | Reports & analytics |
| `arc.*` | Archive Explorer |
| `doc.*` | Documentation & support center |
| `adm.*` | Administration (users, roles, settings, audit log) |
| `plan.*` | Planning & Scheduling Engine (6.16) |
| `log.*` | Activity Feed & Audit Log (6.17) |
| `trn.*` | Training, Certification & Habilitation (6.20) |
| `iot.*` | IoT Integration Gateway (6.21) |
| `erp.*` | ERP & External Systems Connector (6.22) |
| `ptw.*` | Work Permit System (6.23) |
| `fin.*` | Budget & Cost Center Management (6.24) |
| `ins.*` | Inspection Rounds & Checklists (6.25) |
| `cfg.*` | Configuration Engine & Tenant Customization (6.26) |

**Features:**

**Scoped Role Assignment:**
- Users can hold one or more role assignments scoped to tenant, entity, site, team, or org node so a supervisor may manage one site without inheriting global rights everywhere
- Effective dates on `user_scope_assignments` support temporary coverage, acting assignments, and planned role changes without rewriting history
- SSO or local user identities bind to personnel records, but authorization is still governed here rather than hidden inside the authentication layer

**Permission Governance & Dangerous Actions:**
- Built-in domains remain stable, while tenant-created custom permissions can extend access control for 6.26 workflow guards, custom fields, and tenant-specific module behavior
- Dangerous permissions are clearly marked and can require step-up reauthentication before execution even when already granted to the role
- `permission_dependencies` warn or block invalid combinations such as granting close, publish, or export powers without the required base visibility or edit permissions

**Delegated Administration & Emergency Access:**
- `delegated_admin_policies` allow limited administrators to manage only approved scopes and permission domains instead of forcing one all-powerful global administrator model
- Time-boxed emergency elevation is allowed only where policy permits; reason, expiry, approver, and all granted rights must be captured in 6.17 audit history
- Default system roles remain non-deletable, but custom role templates help tenants create repeatable operator, planner, storekeeper, HSE, and contractor access models quickly

**Access Simulation & Auditability:**
- Effective-access simulator shows what a selected user can view, edit, approve, export, or configure in a given scope before a role assignment is saved
- Role and permission changes are previewed before activation and recorded as dangerous admin events in 6.17 with actor, scope, diff summary, and whether step-up reauthentication was used
- Export and import of role models support multi-site parity without bypassing validation of scope rules, dangerous permissions, or dependencies

**Permissions:** `adm.users` / `adm.roles` / `adm.permissions`

---

### 6.8 Spare Parts & Inventory Management

**Objective:** Govern spare-part identity, stock state, reservation, movement, procurement, and repairable-part loops so material availability and material cost remain trustworthy inputs to execution, planning, budgeting, and ERP reconciliation.

**Data Entities:**
- `article_families`: id, code, name, parent_id, reference_domain_version_id
- `articles`: id, code, description, family_id, stocking_type (stocked/non_stock/repairable/consumable), unit_of_measure_id, criticality_class, preferred_warehouse_id, reorder_policy (min_max/reorder_point/periodic/manual), min_stock, max_stock, reorder_point, safety_stock, standard_cost, lead_time_days, erp_item_id, is_active
- `stock_balances`: id, article_id, warehouse_id, storage_location_id, quantity_on_hand, quantity_reserved, quantity_on_order, quantity_quarantined, last_counted_at
- `stock_reservations`: id, article_id, source_type (work_order/pm_occurrence/inspection_round/permit), source_id, reserved_qty, issued_qty, status (requested/reserved/partially_issued/issued/released/cancelled), required_by_date
- `inventory_transactions`: id, article_id, warehouse_id, storage_location_id, transaction_type (receipt/issue/return/transfer/adjustment/count_variance/repair_send/repair_return/scrap), quantity, unit_cost, source_type, source_id, performed_by_id, posting_status (local/erp_posted/reversed), occurred_at
- `purchase_requisitions`: id, code, supplier_id (nullable), article_id, requested_qty, status (draft/approved/sent_to_erp/ordered/cancelled/closed), needed_by_date, source_type, source_id
- `purchase_orders`: id, code, supplier_id, external_po_id, status (draft/sent/confirmed/partial_received/received/cancelled), order_date, expected_date, closed_at
- `goods_receipts`: id, purchase_order_id, supplier_delivery_ref, received_at, received_by_id, qc_status (pending/pass/hold/rejected), posted_to_erp_at
- `stock_counts`: id, warehouse_id, count_type (cycle/full/spot), started_at, completed_at, status (draft/in_progress/reviewed/posted), approved_by_id
- `repairable_parts_cycles`: id, article_id, linked_equipment_id, removed_at, sent_to_workshop_at, returned_at, repair_status (installed/at_workshop/returned/scrapped), workshop_name, repair_cost, replaced_with_article_id
- `replenishment_recommendations`: id, article_id, recommended_qty, recommendation_basis (min_max/pm_forecast/critical_shortage/lead_time_risk/manual), generated_at

**Features:**

**Item Master, Stock State & Location Traceability:**
- Every stocked item carries governed family, stocking type, criticality, reorder policy, and external ERP identifier so procurement and reporting stay consistent across sites and systems
- Stock state distinguishes on-hand, reserved, on-order, and quarantined quantities rather than one undifferentiated balance
- Warehouse and bin structure remains governed through 6.13 reference domains, with scan-friendly lookup for field issue and receipt workflows

**Reservation, Issue & Return Discipline:**
- Planned work in 6.5, 6.9, 6.16, and 6.25 can create `stock_reservations` before commitment so material-readiness blockers are explicit instead of discovered on the job
- Issue, return, transfer, and adjustment actions are always recorded as `inventory_transactions`; balance edits without a transaction trail are not allowed
- Work-order closure can consume issued parts automatically only if reservation and issue evidence are coherent; shortages remain visible rather than being hidden by negative stock assumptions

**Procurement, Receipt & ERP Handoff:**
- Replenishment can generate requisitions, supplier follow-up, or ERP handoff depending on the active procurement contract in 6.22
- Goods receipt captures receipt evidence, quality hold state, and posting status separately so local operational availability does not get confused with official external posting completion
- Supplier scorecards, overdue-order risk, and lead-time exposure support both local buyers and ERP-integrated purchasing workflows

**Counts, Repairables & Forecasting:**
- Count sessions preserve variance, approver, and posted adjustment evidence for cycle counts and full counts
- Repairable-part cycles preserve remove-send-return-scrap history with cost and replacement context instead of burying rotable items inside generic stock adjustments
- Replenishment forecasts combine PM demand, corrective consumption history, critical-spare policy, and lead-time risk so planners see which shortages threaten committed or upcoming work

**Permissions:** `inv.view` / `inv.manage` / `inv.procure` / `inv.count`

---

### 6.9 Preventive Maintenance Planning

**Objective:** Define, version, generate, execute, and continuously optimize planned maintenance strategies using time-, floating-, meter-, event-, and condition-based logic. This module manages preventive maintenance as a strategy system plus due occurrences, not just a recurring work-order generator.

**Data Entities:**
- `pm_plans`: id, code (PM-XXXX), title, asset_scope_type (equipment/family/location/criticality_group), asset_scope_id, strategy_type (fixed/floating/meter/event/condition), criticality_class, assigned_group_id, advance_notice_days, requires_shutdown, requires_permit, is_active, current_version_id
- `pm_plan_versions`: id, pm_plan_id, version_no, effective_from, effective_to, target_interval_unit (day/week/month/hour/km/cycles/event), target_interval_value, tolerance_window_days, trigger_definition_json, task_package_json, required_parts_json, required_skills_json, required_tools_json, estimated_duration_hours, change_reason
- `pm_occurrences`: id, pm_plan_id, plan_version_id, due_basis (calendar/meter/event/condition), due_at, due_meter_value, generated_at, status (forecasted/generated/ready_for_scheduling/scheduled/in_progress/completed/deferred/missed/cancelled), linked_work_order_id, deferral_reason_id, missed_reason_id
- `pm_trigger_events`: id, pm_plan_id, trigger_type, source_reference, measured_value, threshold_value, triggered_at, was_generated
- `pm_executions`: id, pm_occurrence_id, work_order_id, execution_result (completed_no_findings/completed_with_findings/deferred/missed/cancelled), executed_at, completed_by_id, notes
- `pm_findings`: id, pm_execution_id, finding_type, severity, description, follow_up_di_id, follow_up_work_order_id
- `pm_counters`: id, equipment_id, counter_type (hours/km/cycles), current_value, last_reset_at, unit

**Features:**
- **Versioned PM strategies:** PM plans have governed revisions so interval logic, task packages, and required resources can evolve without overwriting historical strategy context
- **Distinct plan vs occurrence model:** PM master plans generate auditable PM occurrences; compliance, deferral, miss, and completion are measured on occurrences, not only on the master plan
- PM calendar and forecast board: D3.js monthly/weekly horizon showing forecasted, generated, deferred, and missed occurrences by asset, family, entity, and criticality class
- **Trigger types:** fixed calendar, floating from actual completion date, meter-based from runtime or cycles, event-based from inspection or workflow events, and condition-based from IoT thresholds
- **Auditable trigger history:** each meter, event, or condition trigger creates a trigger-event record showing threshold, measured value, and whether a PM occurrence or WO was generated
- **Auto Work Order generation:** when a PM occurrence becomes due or is triggered, the system can automatically create a Draft WO or place the occurrence into the ready-for-scheduling queue depending on tenant configuration
- **Compliance tracking:** PM compliance, overdue exposure, missed-PM rate, planned maintenance percentage, and follow-up corrective-work rate are tracked by occurrence, entity, asset family, and criticality class
- **Deferral and miss logic:** deferring or missing an occurrence requires a coded reason; repeated deferrals are highlighted as strategy risk, not hidden in schedule noise
- **Findings-driven feedback loop:** PM completion can produce structured findings and linked follow-up DIs or WOs so Maintafox can measure whether a PM is catching issues early or failing to prevent breakdowns
- **PM optimizer (evidence-driven):** suggest interval changes, task consolidation, shutdown alignment, and workload leveling using findings rate, failures between PMs, actual vs estimated duration, and follow-up work frequency; supervisors review and approve proposed changes before publishing a new PM plan version
- **Safety and qualification requirements:** PM task packages can include mandatory safety steps, required permits, and required habilitation; scheduling warns when skill or authorization gaps exist
- **Team capacity awareness:** PM occurrences integrate with Planning & Scheduling (6.16) so only schedule-ready work is committed; blocked occurrences remain visible with the reason they are not ready
- Maintenance budget projection: project next 12 months of planned labor and parts demand using active PM plan versions and generated occurrences

**Permissions:** `pm.view` / `pm.create` / `pm.edit` / `pm.delete`

---

### 6.10 Reliability Engineering Engine (RAMS / RCM)

**Objective:** Transform governed failure, downtime, runtime, and execution evidence into reliability indicators and maintenance-decision support. Delivery is phased: **Phase 1** establishes the reliability-ready data foundation and core reliability analytics, **Phase 2** adds FMECA and RCM decision support, and **Phase 3** introduces advanced modeling only for mature datasets and advanced users.

#### 6.10.1 Reliability-Ready Data Foundation (Phase 1)

**Data Entities:**
- `failure_hierarchies`: id, name, asset_scope, version_no, is_active
- `failure_codes`: id, hierarchy_id, parent_id, code, label, code_type (class/mode/cause/effect/remedy), is_active
- `failure_events`: id, source_type (work_order/inspection/condition_alert/manual), source_id, equipment_id, component_id, detected_at, failed_at, restored_at, downtime_duration_hours, active_repair_hours, waiting_hours, is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, cause_not_determined, production_impact_level, safety_impact_level, recorded_by_id, verification_status
- `runtime_exposure_logs`: id, equipment_id, exposure_type (hours/cycles/output_distance/production_output), value, recorded_at, source_type
- `reliability_kpi_snapshots`: id, equipment_id, asset_group_id, period_start, period_end, mtbf, mttr, availability, failure_rate, repeat_failure_rate, event_count, data_quality_score

**What is fed from other modules:**
- Closed corrective WOs feed governed failure events with coded failure details, downtime, actual labor, and actual parts context
- PM executions feed planned downtime and finding-driven strategy feedback
- Inspection anomalies feed early-condition and incipient-failure evidence
- Equipment, IoT, and counter logs feed runtime or exposure history for denominator calculations

**Features:**
- **Governed failure-event generation:** only eligible records create reliability events; planned work, partial downtime, and cause-not-determined scenarios follow explicit inclusion rules
- **Failure hierarchy administration:** failure classes, modes, causes, and effects are versioned and governed so reliability metrics use stable technical language
- **Data quality dashboard:** highlights missing coding, missing downtime, weak runtime history, and assets with insufficient evidence for certain analyses
- **Legacy backfill:** manually add qualified historical failure events from paper logs or prior systems with provenance marker so old data can be used without pretending it is native-quality
- **Asset boundary definition:** allow analysis at equipment, component, family, or system boundary so reliability results are not mixed across inconsistent scopes

#### 6.10.2 Core Reliability Analytics (Phase 1)

All Phase 1 calculations are computed in **Rust** as dedicated async tasks. Results are cached in SQLite, recalculated on evidence changes, and flagged with data-quality warnings when sample size or coding quality is weak.

| Metric | Formula | Standard / Rule |
|---|---|---|
| **MTBF** (Mean Time Between Failures) | Total governed runtime or exposure / Number of unplanned failure events | ISO 14224-aligned |
| **MTTR** (Mean Time To Repair) | Total active repair time / Number of repairable failure events | ISO 14224-aligned |
| **MTTF** (Mean Time To Failure) | Exposure until first failure for non-repairable items | ISO 14224-aligned |
| **Availability** | Uptime / (Uptime + governed downtime) or MTBF / (MTBF + MTTR) where appropriate | IEC 60050-191 aligned |
| **Operational Availability** | Uptime / (Uptime + downtime + waiting/logistics time) | MIL-HDBK-911 aligned |
| **Failure Rate** | Number of governed failures / governed exposure | Reliability rule set |
| **Repeat Failure Rate** | Repeat same asset + same failure mode events / total governed failure events | Maintafox governed metric |

**Features:**
- **Bad-actor ranking:** rank assets by failure count, downtime, recurrence, and cost impact
- **Pareto and trend views:** failure mode frequency, downtime Pareto, recurrence trend, and site or asset-family trend charts
- **PM and inspection correlation:** show whether repeated failures persist despite PM work or recurring inspection findings
- **Confidence warnings:** metrics display a warning when event counts, coding completeness, or exposure data are too weak for strong interpretation

#### 6.10.3 Reliability Review Workspace (Phase 1)

**Features:**
- **Failure history explorer:** browse governed failure histories by asset, component, family, location, or site
- **Root-cause and recurrence review:** compare recent failure events, close-out evidence, and unresolved repeat failures
- **Cost-of-failure view:** combine downtime, labor, parts, and service actuals to expose the operational cost of repeat issues
- **Reliability action watchlist:** show assets that require deeper analysis, PM review, redesign consideration, or repeated correction tracking

#### 6.10.4 FMECA Workspace (Phase 2)

**Data Entities:**
- `fmeca_analyses`: id, equipment_id, title, boundary_definition, created_at, created_by_id, status
- `fmeca_items`: id, analysis_id, component_id, functional_failure, failure_mode_id, failure_effect, severity (1-10), occurrence (1-10), detectability (1-10), rpn (auto = S x O x D), recommended_action, current_control, linked_pm_plan_id, linked_work_order_id, revised_rpn

**Features:**
- RPN heatmap: red > 200, orange 100-200, yellow 50-100, green < 50
- Criticality matrix: 5x5 severity x occurrence view for rapid prioritization
- **Action tracking:** recommended actions link directly to PM plan versions or WOs for closure and later risk re-evaluation
- Export to Excel/PDF in standard FMEA-style format for engineering review

#### 6.10.5 RCM Decision Support (Phase 2)

**Data Entities:**
- `rcm_studies`: id, equipment_id, title, created_at, created_by_id, status
- `rcm_decisions`: id, study_id, function_description, functional_failure, failure_mode_id, consequence_category, selected_tactic (condition_based/time_based/failure_finding/run_to_failure/redesign), justification, review_due_at

**Features:**
- **RCM decision logic:** guide users through function, failure mode, consequence, and tactic selection instead of treating RCM as a static report
- **PM feedback loop:** approved RCM decisions can generate new PM plan versions, modify intervals, or justify run-to-failure or redesign choices
- **Rationale log:** preserve the full decision path and review date for audit and later challenge

#### 6.10.6 Advanced Reliability Modeling (Phase 3 / Enterprise Maturity)

Advanced models are explicitly **staged later** and are not part of the mandatory first-wave delivery. They become available only when the tenant has sufficient governed data, clear asset boundaries, and appropriately trained reliability users.

**Phase 3 candidate methods:**
- **Weibull / survival analysis:** enabled only when comparable governed failure-event volume is sufficient and runtime exposure is credible
- **Fault Tree Analysis (FTA):** for critical system-level causal modeling where top-event logic is clearly defined
- **Reliability Block Diagrams (RBD):** for system configuration studies with well-defined series/parallel relationships
- **Optional future enterprise tier:** Bow-Tie / LOPA, Event Tree, or Markov-state modeling for selected high-maturity deployments after explicit design validation

**Staging rule:** Phase 1 and Phase 2 create the operationally valuable reliability foundation; advanced probabilistic modeling is added later where the data, use case, and engineering maturity justify it.

**Permissions:** `ram.view` / `ram.analyze` / `ram.export`

---

### 6.11 Analytics & Dashboard

**Objective:** Role-aware operational intelligence grounded in governed workflow evidence. Reliability widgets follow the staged reliability model: **Phase 1** exposes core reliability KPIs and data-quality views, **Phase 2** adds FMECA and RCM action visibility when enabled, and **Phase 3** advanced-model outputs appear only for tenants that explicitly activate them.

#### 6.11.1 Main Dashboard KPIs

| Indicator | Calculation |
|---|---|
| Work Orders: Open | Count of WOs with status not in {Closed, Cancelled} |
| Work Orders: Overdue | Open WOs where planned_end < today |
| Work Orders: Critical Pending | Open WOs with urgency = 5 (Critique) |
| DI: Pending Approval | DIs in status = "Pending Review" |
| DI: SLA Breach Rate | % of open DIs past their SLA deadline |
| Equipment: Under Maintenance | Equipment with status = under_maintenance |
| PM Compliance Rate | (PM executed on time / Total PM due) * 100 for last 30 days |
| Fleet MTBF (Qualified Assets Only) | Average MTBF across assets with governed failure events, qualified exposure history, and minimum sample threshold |
| Repeat Failures (30d) | Count of governed failure events where the same equipment + failure mode recurs within 30 days |
| Maintenance Cost MTD | Sum of posted maintenance cost events this calendar month for the current reporting scope |
| Certifications Expiring =30d | Count of personnel certifications expiring within 30 days |

Reliability KPI widgets must display a data-quality badge. If runtime exposure, failure coding, or sample-size thresholds are not met, the widget shows a warning state with drill-through to the reliability data-quality workspace instead of a false-precision number.

#### 6.11.2 Charts (D3.js)

- **Weekly Workload Bar Chart:** WOs per day for current week (grouped by status)
- **Trend Line (30-day):** new DIs vs. closed WOs over rolling 30-day period
- **Equipment Health Distribution:** donut chart: Operational / Under Maint. / Out of Service / Decommissioned
- **Maintenance Type Breakdown:** Corrective vs. Preventive vs. Improvement (pie/donut)
- **Overdue Aging Chart:** bar chart by days overdue (0-7, 7-30, 30-90, 90+)
- **Top 5 Bad Actors:** ranked bar chart by governed failure count, downtime, repeat failures, or cost-of-failure; low-quality evidence flagged visibly
- **Core Reliability Trend:** MTBF, MTTR, repeat-failure rate, and reliability data-quality coverage for qualified assets only
- **Monthly Cost Trend:** 12-month area chart of maintenance cost
- **Corrective vs. Preventive Cost Split:** stacked bar chart per month; target ratio line overlay (e.g., =40% corrective)
- **Energy Consumption Trend:** monthly energy cost vs. normalized production output; requires IoT Gateway energy meter integration
- **Regulatory Compliance Status:** ATEX inspection compliance, open safety findings count, habilitation coverage rate

#### 6.11.3 Reports Module

**Pre-built report library:**
- PM Compliance Report (by equipment / by period / by entity)
- Work Order History Report (filterable, exportable)
- Equipment Maintenance Cost Report
- Technician Workload Report (hours by WO type per period per technician)
- Inventory Movement Report (IN/OUT/adjustments with transaction trace)
- Core Reliability KPI Summary (Phase 1; MTBF, MTTR, failure rate, repeat-failure rate, and data-quality indicators)
- Reliability Data Quality Report (uncoded failures, missing downtime, missing exposure, weak-sample assets)
- FMECA / RCM Action Review Report (Phase 2 only, when enabled)
- Habilitation & Certification Expiry Report (for external auditors/labor inspectorate)
- Regulatory & Safety Compliance Report (ATEX inspections, open findings, PTW summary)
- Contractor Activity Report (external companies, hours billed, open WOs)
- Budget vs. Actual & Commitment Report (per cost center, per period)
- Shutdown/Turnaround Planning Summary (planned window, WOs within window, resource plan)

**Output formats:** PDF (locally generated, offline-capable) / Excel (.xlsx) / CSV / Power BI push dataset (.pbix compatible)

**Report scheduling:** each report can be scheduled (daily / weekly / monthly / quarterly) with a recipient list; delivery requires SMTP configuration in Settings module (6.18); saved copies always retained locally

Advanced reliability model exports remain owned by module 6.10 and are surfaced here only when the corresponding Phase 3 methods are enabled for the tenant.

All reports: Export to **PDF** and **Excel** (generated locally, no network required)

#### 6.11.4 Analytical Alert Engine

Background rule engine that evaluates defined thresholds after every data change cycle and surfaces anomalies proactively - so supervisors do not need to manually search for problems.

| Alert Type | Trigger Condition | Default Severity |
|---|---|---|
| Corrective cost spike | Corrective WO costs this month > 120% of monthly budget | High |
| MTTR regression | Qualified asset-group MTTR this period > previous period * 1.30 and minimum sample threshold met | Medium |
| PM compliance drop | PM rate < 85% for 2 consecutive weeks | High |
| Repetitive failure | Same equipment + same failure mode, = 3 governed failure events in 30 days | Critical |
| Reliability data quality drop | % of governed failure events with missing coding, downtime, or exposure linkage exceeds threshold | Medium |
| Stock consumption anomaly | Article consumption > 200% of 30-day rolling average | Medium |
| Overdue WO surge | Overdue WO count increases > 20% week-over-week | Medium |
| DI SLA breach spike | SLA breaches > 15% of open DIs | High |
| Certification gap detected | A planned WO requires a habilitation no available technician holds | Critical |
| Budget overrun risk | Cumulative monthly spend projected to exceed annual budget before year-end | High |

Analytical alerts are displayed as banners on the Analytics page and can optionally trigger in-app / email / SMS notifications to configured recipients through module 6.14.

When Phase 2 reliability decision support is enabled, overdue FMECA actions and overdue RCM review dates can also raise analytical alerts. Phase 3 probabilistic-model outputs do not generate tenant-wide alerts by default.

#### 6.11.5 Report Scheduling & Distribution

- **Auto-schedule engine:** each report template supports a configurable cron schedule; reports generated by a background Rust task at the scheduled time
- **Distribution lists:** each scheduled report has a recipient list (internal users by role); delivery via SMTP (requires Settings -> Notifications SMTP config)
- **Report library log:** every generated report is stored locally (compressed) with metadata (generated_by, generated_at, period, format); accessible from the Analytics page with a download link
- **Custom dashboard layouts:** users can add/remove chart widgets to their personal dashboard and save the layout; up to 12 widgets per layout; advanced reliability widgets appear only when the corresponding reliability phase is enabled for the tenant and role
- **Power BI integration (Enterprise):** Maintafox exposes a push-dataset endpoint compatible with Power BI Streaming Datasets; data refreshed at configurable intervals

**Permissions:** `rep.view` / `rep.export` / `rep.schedule`

---

### 6.12 Archive Explorer

**Objective:** Govern read-only access to archived operational, audit, and configuration history while separating true historical records from the smaller set of soft-deleted items that are actually eligible for recovery. This module is a historical evidence workspace, not a generic recycle bin.

**Data Entities:**
- `archive_items`: id, source_module, source_record_id, archive_class (operational_history/soft_delete/audit_retention/config_snapshot/report_copy), source_state, archive_reason_code, archived_at, archived_by_id, retention_policy_id, restore_policy (not_allowed/admin_only/until_date), restore_until_at, legal_hold, checksum_sha256, search_text
- `archive_payloads`: id, archive_item_id, payload_json_compressed, workflow_history_json, attachment_manifest_json, config_version_refs_json, payload_size_bytes
- `retention_policies`: id, module_code, archive_class, retention_years, purge_mode (manual_approval/scheduled/never), allow_restore, allow_purge, requires_legal_hold_check
- `archive_actions`: id, archive_item_id, action (archive/restore/export/purge/legal_hold_on/legal_hold_off), action_by_id, action_at, reason_note, result_status

**Archive Classes Covered:**
- Operational history: closed or cancelled DIs, WOs, permits, inspections, PM occurrences, POs, contracts, projects, and financial snapshots that remain as evidence
- Soft-delete recovery: selected administrative records where policy allows restore
- Audit retention: archived activity logs, report packages, and exported evidence bundles
- Configuration history: archived change sets and superseded configuration snapshots where historical interpretation depends on prior settings

**Archive Flow Rules:**
```
Active record -> Archived historical record
Eligible soft-deleted record -> Archive recovery bucket
Archived record -> Clone or follow-up only
Recovery-bucket item -> Restored when policy allows
Archived item -> Purged only after retention and approval checks
```

**Features:**
- **Explorer plus catalog view:** browse archives by module, year, entity, asset, archive class, or retention status with a folder-tree left panel and searchable detail panel
- **Immutable historical snapshot:** every archived record preserves its field payload, workflow history, linked attachments manifest, and relevant configuration-version context at archive time
- **Restore eligibility rules:** only approved archive classes support restore; completed work history, posted financial events, and append-only audit logs are view-only and cannot be reopened as live records
- **Clone and follow-up actions:** from a non-restorable archive item, users can create a new DI, WO, PM review, or export package without mutating historical evidence
- **Legal hold and retention panel:** show purge due date, legal-hold status, retention rule, and approval requirements before any destructive action is attempted
- **Purge workflow:** hard delete is restricted to eligible classes after policy checks and multi-step confirmation; purge always leaves an immutable journal entry in `archive_actions`
- **Audit trail:** show who archived, restored, exported, placed legal hold, or purged an item, with timestamps and reason notes
- **Cross-module context:** archived records keep navigation back to related asset, work history, failure history, cost records, and activity events where links still exist
- **Bulk operations:** export packages, legal-hold actions, or purge actions can be performed in bulk for eligible items; bulk restore is restricted to approved recovery classes only

**Permissions:** `arc.view` / `arc.restore` / `arc.export` / `arc.purge`

---

### 6.13 Lookup / Reference Data Manager

**Objective:** Govern the reference domains, hierarchies, and controlled code sets that drive search, workflow semantics, reliability analysis, inventory traceability, and ERP mapping. This module is the semantic backbone of Maintafox, not a generic dropdown editor.

**Data Entities:**
- `reference_domains`: id, code, name, structure_type (flat/hierarchical/versioned_code_set/unit_set/external_code_set), governance_level (protected_analytical/tenant_managed/system_seeded/erp_synced), is_extendable, validation_rules_json
- `reference_sets`: id, domain_id, version_no, status (draft/validated/published/superseded), effective_from, created_by_id, created_at, published_at
- `reference_values`: id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json
- `reference_aliases`: id, reference_value_id, alias_label, locale, alias_type (legacy/import/search), is_preferred
- `reference_change_events`: id, set_id, action (create/update/deactivate/merge/migrate/import/publish), action_by_id, action_at, summary
- `unit_conversions`: id, from_uom_id, to_uom_id, factor, precision, rounding_rule

**Managed Reference Domains:**
- Workforce and capability: positions, schedule classes, skills, tools
- Asset and reliability taxonomies: equipment families, failure classes, failure modes, failure causes, failure effects, impact levels
- Inventory and supplier taxonomies: article families, supplier families, warehouses, storage locations, units of measure, VAT codes
- Work execution taxonomies: work order types, urgency levels, delay reasons, inspection anomaly types, variance-driver codes, and other module-owned code sets exposed through protected domain views

**Reference Lifecycle:**
```
Draft -> Validated -> Published -> Superseded
```

**Features:**
- **Reference domain catalog:** each domain shows governance badge, structure type, usage count, external mapping status, and whether it is protected analytical data or a simpler local list
- **Draft -> validate -> publish workflow:** changes are prepared in draft, validated for duplicates and structural errors, impact-analyzed, and then published as a new reference set version
- **Hierarchy editor:** tree-based editor for families, storage structures, and failure hierarchies with code uniqueness, parent-child validation, and effective version control
- **Protected analytical domains:** domains used by reliability, inventory valuation, planning, cost, or workflow analytics cannot be destructively edited; in-use values are deactivated or migrated rather than deleted
- **Usage explorer and impact preview:** before publish, show linked assets, WOs, PM plans, inventory items, reports, and integrations that depend on changed values
- **Alias and legacy mapping:** maintain synonyms, legacy labels, and import aliases so search, migration, and ERP synchronization remain stable during terminology changes
- **Merge and migration tools:** merge duplicates or retire obsolete values through an explicit mapping workflow that preserves historical traceability
- **Failure-code administration:** manage failure hierarchies as first-class governed reference domains with class/mode/cause/effect semantics aligned to the reliability module
- **Import/Export templates:** bulk import and export per domain with validation report, row-level error feedback, and optional ERP external-code matching
- **Unit and tax controls:** units of measure support governed conversion rules; VAT and similar financial code sets validate formatting and in-use constraints before publish

**Permissions:** `ref.view` / `ref.manage` / `ref.publish`

---

### 6.14 Notification System

**Objective:** Govern event routing, delivery, acknowledgement, and escalation for maintenance, safety, compliance, reliability, and integration signals. Notifications are the attention layer; the source record remains the system of record.

**Data Entities:**
- `notification_events`: id, source_module, source_record_id, event_code, category_code, severity (info/warning/error/critical), occurred_at, dedupe_key, payload_json
- `notification_categories`: id, code, label, default_severity, default_requires_ack, is_user_configurable
- `notification_rules`: id, category_code, routing_mode (assignee/reviewer/role/team/entity_manager/watcher/manual), requires_ack, dedupe_window_minutes, quiet_hours_policy_json, escalation_policy_id, is_active
- `notifications`: id, notification_event_id, recipient_user_id, recipient_role_id (nullable), delivery_state (pending/delivered/read/acknowledged/snoozed/escalated/closed/failed/expired), title, body, action_url, created_at, read_at, acknowledged_at, closed_at
- `notification_deliveries`: id, notification_id, channel (in_app/os/email/sms), attempt_no, delivery_status (queued/sent/delivered/failed/skipped), attempted_at, delivered_at, failure_reason
- `notification_acknowledgements`: id, notification_id, acknowledged_by_id, acknowledged_at, acknowledgement_note
- `notification_preferences`: id, user_id, category_code, in_app_enabled, os_enabled, email_enabled, sms_enabled, digest_mode (instant/daily_digest/off), muted_until
- `notification_escalation_steps`: id, escalation_policy_id, level_no, wait_minutes, route_to_type (user/role/team/entity_manager), route_to_reference, channel_set_json

**Notification Lifecycle:**
```
Source event detected -> Routed -> Delivered -> [Read]
Read -> [Acknowledged | Snoozed | Auto-Closed]
Unacknowledged critical alert -> Escalated
Resolved source record -> Related notifications auto-close where policy allows
```

**Default Notification Categories:**

| Event | Trigger Example | Routing Basis | Ack Required | Default Severity |
|---|---|---|---|---|
| DI Pending Review | DI enters pending review queue | reviewer / review queue | No | Info |
| DI SLA Breach | SLA deadline passed before review or action | reviewer -> manager escalation | Yes | High |
| WO Assigned / Overdue | WO assigned to user or missed planned end | assignee / supervisor | No | Warning |
| PM Due / PM Missed | PM occurrence enters due-soon or missed state | planner / assignee | No | Warning |
| Stock Critical | critical spare reaches zero stock | warehouse owner / buyer | Yes | Critical |
| Certification Expiry / Qualification Gap | certification near expiry, expired, or skill gate blocks work | person / supervisor / planner | Yes for expired | Critical |
| Inspection Escalation | missed round or anomaly routed to DI/WO review | assignee / supervisor | No | Warning |
| PTW Critical Event | permit expiring, suspended, or revalidation required | permit owner / operations / HSE | Yes | Critical |
| Analytical Alert | alert from 6.11 threshold engine | configured role recipients | Depends on category | Varies |
| Integration Failure | ERP sync, VPS sync, or IoT gateway failure | integration admin | Yes | High |
| Support Response Available | vendor replied to support ticket | ticket submitter | No | Info |

**Features:**
- **Event-driven core with scheduled reminders:** source modules emit notification events immediately on important transitions; a background Rust scheduler handles time-based reminders such as due-soon, overdue, and expiry thresholds
- **Noise control:** dedupe repeated events for the same unresolved condition, suppress obsolete alerts after source resolution, and support digest mode or quiet-hours policy for non-critical categories
- **Routing rules:** route alerts to assignees, reviewers, supervisors, entity managers, teams, or watchers instead of broadcasting indiscriminately
- **Actionable notification center:** bell icon opens a filterable inbox with unread, acknowledged, escalated, and snoozed views; each notification deep-links to the source record and can expose safe quick actions such as acknowledge, snooze, or open source item
- **Critical acknowledgement and escalation:** critical alerts require explicit acknowledgement with optional note; missing acknowledgement triggers the configured escalation chain from Settings 6.18.3
- **Channel fallback and retry:** in-app and OS notifications continue locally while offline; email and SMS deliveries queue and retry after connectivity returns; failed external sends are logged in `notification_deliveries`
- **Source-of-truth discipline:** notification actions never silently mutate the source record; operational closure remains in the owning module, while source resolution can auto-close stale notifications
- **History and archive:** notification history remains searchable locally for 180 days, then eligible records are archived through module 6.12 according to retention policy
- **Delivery and response metrics:** expose time-to-delivery, time-to-read, time-to-acknowledge, escalation counts, and noisy-category rates for notification-administration review

**Permissions:** No special permission required to view and act on a user's own notifications. System-wide category, channel, and escalation administration uses `adm.settings`.

---

### 6.15 In-App Documentation & Support Center

**Objective:** Deliver the right approved technical, safety, and training information at the point of work, online or offline, while preserving document lifecycle control, authorization gates, auditability, and a structured support channel for issue resolution.

**Data Entities:**
- `document_categories`: id, code (procedures/diagnostics/regulatory/training/safety/help), name, icon
- `documents`: id, reference (format: `CAT-FAM-NNN`, e.g., `SEC-LOTOTO-001`), title, category_id, status (draft/in_review/approved/effective/superseded/withdrawn), owner_id, review_due_at, confidentiality_level (public_internal/restricted_safety/restricted_admin), current_version_id, is_pinned
- `document_versions`: id, document_id, version_number, file_path, file_mime_type, file_size_bytes, checksum_sha256, effective_from, superseded_at, change_summary, created_by_id, created_at
- `document_bindings`: id, document_id, binding_type (equipment_family/equipment/work_type/permit_type/inspection_template/pm_plan_version/training_topic/hazard_type), binding_id, relevance_role (required/reference/emergency)
- `document_access_events`: id, document_version_id, user_id, access_type (view/download/print/acknowledge), occurred_at
- `document_acknowledgements`: id, document_version_id, user_id, acknowledgement_type (viewed/read_and_understood/required_before_work), acknowledged_at
- `support_tickets`: id, title, description, category (bug/feature/training/other), severity (low/medium/high/critical), status (draft/queued/submitted/acknowledged/in_progress/waiting_for_customer/resolved/closed), created_by_id, created_at, submitted_at, environment_snapshot_json, local_log_bundle_path
- `support_ticket_messages`: id, ticket_id, sender_type (customer/vendor/system), message_body, attachment_manifest_json, created_at
- `help_articles`: id, module_code, workflow_state_scope, role_scope, title, content_markdown, version_no, is_active, last_updated_at

**Features:**

**Controlled Document Library:**
- Full-text searchable knowledge base indexed locally with extracted-text search on title, reference, content, and aliases
- **Controlled lifecycle:** documents move through draft, review, approved, effective, superseded, and withdrawn states; only effective versions appear by default in operational workflows
- **Context bindings:** documents can be linked to equipment families, assets, work types, permit types, inspection templates, PM plan versions, training topics, or hazard classes so users see the right instruction in context
- **Authorization gate:** restricted documents are visible only to users whose role and certification or habilitation status permit access; unauthorized users see the title with a locked state when policy allows metadata exposure
- **Required acknowledgements:** critical safety or operational procedure revisions can require acknowledgement before work begins or before selected workflows proceed; acknowledgements are stored separately from downloads
- **Pinned and emergency documents:** administrators can pin emergency protocols, LOTO masters, ATEX procedures, and shutdown manuals for fast access from the home and work-detail views
- **LOTO and ATEX procedure packages:** structured templates support energy sources, isolation steps, verification tests, PPE, de-isolation sequence, and emergency references; packages can be attached to permits and exported with work packages
- **Historical version access:** superseded versions remain accessible for audit and investigation but are clearly labeled non-current; the system always shows which version was effective at a recorded work event when that context is available
- **Access audit trail:** views, downloads, prints, and acknowledgements are logged through `document_access_events`; logs remain exportable for audit and compliance review
- **Offline document packs:** users can pin packs for local offline use by role, site, or asset family; offline availability is policy-driven so critical procedures remain available even when disconnected
- **Bulk import and review:** upload multiple files, classify them, validate metadata, and route them through approval rather than publishing files directly into live use

**User Manual & Contextual Help:**
- Module-contextual help: the "?" action opens help articles filtered by module, workflow state, and role so users receive guidance relevant to what they are doing now
- Search across help, procedures, and diagnostic references from a single in-app search surface with filters by module, document class, asset family, and safety relevance
- Keyboard shortcut reference and release-note changelog remain bundled locally with the application version
- Context-aware cross-links: help articles can link directly to related documents, training topics, and support-ticket categories

**Support Center:**
- **Structured support ticket submission:** users can submit support issues with title, description, category, severity, screenshots, and attachments; ticket state is tracked locally and, when online, synchronized with the Maintafox vendor-support API
- **Offline queue with diagnostics:** if the VPS is unreachable, tickets remain in `queued` state locally and are submitted on the next successful sync; the app captures product version, OS info, active module, sync status, and recent diagnostic-log bundle to speed resolution
- **Threaded support conversation:** tickets maintain a message thread instead of a single vendor-response field; each reply is visible in-app and can trigger a notification through module 6.14
- **Customer-follow-up states:** vendor can request more information, and the local user sees the ticket move to `waiting_for_customer` until a reply is sent
- **Tutorial links and external community:** curated video tutorials remain available as external links when online; enterprise community/forum links are clearly labeled external and degraded gracefully offline

**Permissions:** `doc.view` / `doc.manage` / `doc.approve`. Any authenticated user can submit and track their own support tickets.

---

### 6.16 Planning & Scheduling Engine

**Objective:** Readiness-aware planning and commitment-based scheduling engine that turns candidate work into an executable short-term schedule against real capacity, real constraints, and measurable schedule discipline. This module replaces ad-hoc calendars with a controlled planning process.

**Data Entities:**
- `planning_windows`: id, entity_id, window_type (production_stop/maintenance_window/planned_shutdown/turnaround/public_holiday/access_window), start_datetime, end_datetime, description, is_locked (locked windows block maintenance scheduling)
- `capacity_rules`: id, entity_id, team_id, effective_date, available_hours_per_day, max_overtime_hours_per_day, notes
- `schedule_candidates`: id, source_type (work_order/pm_occurrence/inspection_follow_up/project), source_id, readiness_status (not_ready/ready/committed/dispatched/completed), readiness_score, priority_id, required_skill_set_json, required_parts_ready (boolean), permit_status, shutdown_requirement, estimated_duration_hours
- `schedule_commitments`: id, schedule_period_start, schedule_period_end, source_type, source_id, committed_start, committed_end, assigned_team_id, assigned_personnel_id, frozen_at, committed_by_id
- `scheduling_conflicts`: id, reference_type, reference_id, conflict_type (no_qualified_technician/missing_critical_part/locked_window/skill_gap/double_booking/permit_missing/prerequisite_missing), detected_at, resolved_at, resolution_notes
- `schedule_change_log`: id, reference_type, reference_id, field_changed (planned_start/planned_end/assigned_to), old_value, new_value, changed_by_id, changed_at, reason_code, reason_note
- `schedule_break_ins`: id, schedule_commitment_id, break_in_reason (emergency/safety/production_loss/regulatory/other), approved_by_id, created_at

**Features:**
- **Ready backlog and blocked backlog:** planners see candidate work separated into ready and blocked queues instead of one undifferentiated list; blocked work exposes the exact blocker type and required action
- **Consolidated multi-module planning view:** single hub for candidate WOs, PM occurrences, inspection follow-up work, and other eligible planned work across all entities; raw unapproved requests do not enter the committed schedule directly
- **View modes:** Weekly (default) / Daily / Monthly / Gantt (horizontal drag-and-drop timeline) plus backlog boards for readiness and blockage management
- **Readiness evaluation before scheduling:** candidate work is checked for parts availability, skill coverage, permit status, shutdown windows, prerequisite inspections, and work-package completeness before it can be committed into the schedule
- **Drag-and-drop reschedule (Gantt mode):** drag a committed WO or PM occurrence to a new slot; drag-and-drop of blocked work requires an explicit override with reason capture; all moves re-validate readiness and constraints immediately
- **Capacity indicator bar:** for each day column, shows planned hours vs available hours for the filtered scope; green (< 80%) / amber (80-100%) / red (> 100%); overtime displayed separately
- **Schedule freeze and commitment snapshot:** planners can freeze the short-term schedule (for example, the upcoming week) so schedule adherence, break-ins, and last-minute moves can be measured against a real commitment point
- **Real-time conflict and blocker detection:** side panel shows missing parts, missing permits, skill gaps, double booking, locked windows, and incomplete prerequisites with suggested resolution paths
- **Production window manager:** maintenance planners define locked production windows and shutdown windows; the scheduler respects these constraints and surfaces conflicts before commitment
- **Break-in work tracking:** emergency or safety work inserted into a frozen schedule is logged as a break-in with coded reason and approver so schedule discipline is measurable instead of hidden by silent reshuffling
- **Gantt PDF export:** print-ready Gantt chart exported as A3/A4 PDF with company logo header, showing the committed schedule for sharing with production and workshop teams
- **Team workload balancing panel:** view current load %, readiness blockers, and skill bottlenecks per team; one-click rebalance suggestions propose moves only where skills and prerequisites match
- **Notify teams action:** after confirming a schedule, trigger in-app + OS notifications to all assigned technicians; notification includes committed work for the upcoming period
- **Planning KPI layer:** schedule adherence, ready backlog size, blocked backlog size, emergency break-in ratio, committed vs completed hours, and duration-estimate accuracy are displayed alongside the planning board
- **ERP schedule export hook (Enterprise):** expose the committed weekly maintenance schedule as a JSON snapshot on the ERP connector endpoint so production planning systems can align with real maintenance commitments

**Permissions:** `plan.view` / `plan.edit` / `plan.confirm` / `plan.windows`

---

### 6.17 Activity Feed & Operational Audit Log

**Objective:** Provide two append-only visibility layers across the product: an operational activity feed for situational awareness and an immutable audit journal for security, compliance, configuration, and administrative traceability.

**Data Entities:**
- `activity_events`: id, event_class (operational/system/security/integration/compliance), event_code, source_module, source_record_type, source_record_id, entity_scope_id, actor_id, happened_at, severity, summary_json, correlation_id, visibility_scope (self/team/entity/global)
- `audit_events`: id, action_code, target_type, target_id, actor_id, auth_context (password/sso/pin/step_up/system), result (success/fail/blocked), before_hash, after_hash, happened_at, retention_class
- `event_links`: id, parent_event_id, child_event_id, link_type (caused/related/generated_from/acknowledged_by)
- `saved_activity_filters`: id, user_id, view_name, filter_json, is_default
- `event_export_runs`: id, requested_by_id, export_scope, started_at, completed_at, status

**Features:**

**Operational Feed:**
- Reverse-chronological activity feed surfaces important cross-module events such as DI transitions, WO completion, PM misses, stock shortages, permit state changes, condition anomalies, and integration failures without forcing supervisors to poll each module separately
- Filters, saved views, and domain summary widgets let users keep one lens for plant-floor awareness and another for a specific entity, team, or severity profile
- Activity entries deep-link to their source records and may expose safe quick actions such as open source item or acknowledge notification, but they never become the source of truth for operational state changes

**Immutable Audit Journal:**
- Security, configuration, role, export, connector, backup, and other high-risk actions generate `audit_events` with actor, target, auth context, result, and before or after hashes where policy requires tamper-evident comparison
- Rust persistence enforces append-only behavior for both activity and audit tables; no UI workflow can edit or delete events in place
- Step-up reauthentication from 6.1 and 6.18 is visible in audit history so reviewers can tell which dangerous actions were merely allowed and which were re-verified at execution time

**Event Correlation & Drill-Through:**
- `correlation_id` and `event_links` allow the system to display full chains such as IoT anomaly -> DI -> WO -> permit -> verification -> posted cost or role change -> reauth -> setting activation -> notification -> audit export
- Operators can pivot from an activity event to related notifications, downstream work, or upstream trigger evidence without losing the causality trail

**Retention, Archive & Export Governance:**
- Operational activity and audit evidence can use different retention classes, but both remain archiveable via 6.12 rather than silently purged
- Export runs are tracked in `event_export_runs`; broad audit export remains permission-controlled and itself creates a dangerous audit event
- Personally scoped users can view their own security and session activity, while broader audit review remains gated by administrative permissions

**Permissions:** `log.view` / `log.export` / `adm.audit`

---

### 6.18 Application Settings & Configuration Center

**Objective:** Govern tenant-wide system policies, secret-backed connection settings, notification and document-service administration, and backup/sync/recovery controls. This module is the operational control plane for the deployment. Tenant business configuration logic remains in module 6.26.

**Data Entities:**
- `app_settings`: id, setting_key (namespaced: `localization.primary_language`, `appearance.color_mode`, etc.), setting_scope (tenant/device/user_default), setting_value_json, category, validation_status, last_modified_by_id, last_modified_at
- `secure_secret_refs`: id, secret_scope (smtp/sms/erp/iot/dms/power_bi), backend_type (windows_credential_manager/mac_keychain/libsecret), secret_handle, last_rotated_at, last_validated_at
- `connection_profiles`: id, integration_type (smtp/sms/erp/iot/dms/power_bi), profile_name, config_json, secret_ref_id, status (draft/tested/active/error/retired), last_tested_at, last_test_result
- `policy_snapshots`: id, policy_domain (session/notification/document_access/sync/backup/recovery), version_no, snapshot_json, activated_at, activated_by_id
- `backup_policies`: id, target_path, encryption_mode, retention_json, schedule_cron, is_active
- `backup_runs`: id, policy_id, run_type (manual/scheduled/restore_test), started_at, completed_at, status, output_checksum_sha256, error_summary
- `settings_change_events`: id, setting_key_or_domain, change_summary, old_value_hash, new_value_hash, changed_by_id, changed_at, requires_step_up_auth, apply_result

**Settings Governance Rule:**
- Low-risk presentation settings may apply immediately but are still audited.
- High-risk policy and connection settings follow **Draft -> Test -> Activate -> Revert** workflow and require step-up reauthentication before activation.

**Settings Categories:**

#### 6.18.1 Localization & Language
- **Foundational principle:** multilingual capability is a day-one architectural requirement. UI labels, workflow states, validation messages, notifications, report labels, export labels, release notes, and help content must be sourced from governed locale resources rather than hardcoded strings.
- **Initial production baseline:** French is the primary operating language at launch, and English is supported from the outset through the same production-grade locale structure.
- **Expansion model:** additional languages and regional variants must be addable without refactoring business logic, database semantics, screen composition, or report templates; future RTL languages must use the same governed approach rather than a parallel implementation path.
- **Localization governance:** locale assets, domain glossaries, terminology overrides, and translation reviews are versioned, testable, and approval-controlled so multilingual delivery remains organized, professional, and maintainable.
- **Secondary languages:** comma-separated list of fallback languages for multi-language label support
- **Base currency:** DZD / MAD / EUR / USD / GBP; affects cost display and report formatting but does not retroactively revalue historical financial records
- **Date format:** DD/MM/YYYY (default) / YYYY-MM-DD / MM/DD/YYYY
- **Number format:** 1 234,56 (metric/French) / 1,234.56 (English)
- **Week start day:** Monday (default) / Sunday
- **Tenant timezone and report timezone:** control scheduled-report timestamps, due-soon calculations, and audit display conventions

#### 6.18.2 Appearance & Branding
- **Color mode:** Light / Dark / Follow System
- **Accent color:** configurable hex picker for navigation and primary-action surfaces
- **Custom logo:** upload PNG/SVG for sidebar, login screen, and export headers; white-label feature remains Enterprise-only
- **Interface density:** Comfort / Standard / Compact
- **Text scale and high-visibility mode:** accessibility-focused display controls for industrial workstations and kiosk screens
- **Brand preview:** preview mode shows branding changes on login, dashboard, and report headers before activation

#### 6.18.3 Notifications, Escalation & Document Alerts
- **Channel policy per notification category:** configure in-app / OS / email / SMS, retry rules, digest mode, and quiet-hours behavior for non-critical categories
- **SMTP and SMS profiles:** secret-backed configuration with test-before-activate workflow; secrets stored via OS-managed secure storage referenced by `secure_secret_refs`
- **Escalation matrices:** define multi-step escalation chains with wait windows, recipient routing, and channel sets for critical alerts
- **Critical document reminders:** configure reminder and acknowledgement-escalation behavior for safety or operational documents whose acknowledgements are required by module 6.15
- **Scheduled report recipients:** define report-distribution lists and failure-notification recipients separately from end-user notification preferences

#### 6.18.4 Integrations & Document Services
- **ERP connector profiles:** protocol, auth, base URL, and mapping-profile linkage with test and activation status; field mapping logic remains in module 6.22
- **IoT gateway profiles:** protocol, endpoint, polling policy, and connection-health rules
- **GED / DMS integration:** configure SharePoint / Alfresco / custom DMS with mode = link_only / metadata_mirror / selected_file_cache so document governance in 6.15 can support both external repositories and offline-critical local packs
- **Power BI workspace:** workspace, dataset, and refresh profile with validation before activation
- **Connection test and rollback:** every high-risk integration profile supports test run, error capture, and revert to last active version if validation fails

#### 6.18.5 Security, Session & Shared-Device Policy
- **Idle lock, absolute session maximum, and refresh-window policy:** configurable separately instead of one combined timeout
- **Offline grace and device trust policy:** define how long previously trusted users may enter offline, and display remaining grace visibly without allowing policy bypass
- **Shared-device switch behavior:** enforce neutral screen on user switch, clear decrypted in-memory state, and preserve tenant/user cache isolation
- **Step-up reauthentication policy:** required for sensitive actions such as changing connector credentials, restoring backups, changing notification escalation, or disabling protection controls
- **Secret-rotation reminders:** alert administrators when connector or messaging secrets are due for rotation or validation

#### 6.18.6 Backup, Sync & Recovery Operations
- **Encrypted database backup:** manual and scheduled backup with checksum and retention control; output remains encrypted
- **Backup retention:** configurable daily/weekly/monthly retention sets per `backup_policies`
- **Restore test mode:** validate a backup into an isolated workspace and verify integrity before a production restore is attempted
- **Sync preferences:** interval, bandwidth policy, offline-only mode, and connectivity preconditions for queued outbound actions
- **Configuration profile export/import:** export and import settings profiles with secret placeholders by default; optional secret rebind workflow after import
- **Full data export:** structured JSON export for migration or audit without weakening secret storage rules
- **Factory reset:** irreversible wipe gated by step-up reauthentication and explicit typed confirmation; action logged in `settings_change_events`

**Permissions:** `adm.settings`. High-risk activation, restore, and secret-changing actions require step-up reauthentication even inside an active admin session.

---

### 6.19 User Profile & Self-Service

**Objective:** Provide a bounded personal control surface where each authenticated user can manage their own profile, preferences, sessions, trusted devices, and readiness visibility without weakening administrative or security policy.

**Data Entities:**
- `user_profile_preferences`: id, user_id, theme_mode, language_override, home_module, density_mode
- `user_notification_preferences`: id, user_id, category_code, in_app_enabled, os_enabled, email_enabled, sms_enabled, digest_mode, muted_until
- `user_trusted_devices`: id, user_id, device_fingerprint_hash, device_label, enrolled_at, last_seen_at, can_offline_login, revoked_at
- `user_saved_views`: id, user_id, module_code, view_name, filter_json, is_default
- `self_service_events`: id, user_id, event_type (password_changed/pin_set/device_revoked/contact_updated/export_requested/preference_changed), occurred_at, result

**Features:**

**Personal Identity & Contact Context:**
- Profile hero shows name, role, entity scope, linked personnel record, completion status, and user-editable fields allowed by policy such as phone, email, and photo
- Contact and profile-photo updates are bounded by admin policy, synchronized back to the linked personnel record where allowed, and recorded in `self_service_events` and 6.17 audit history

**Security & Device Self-Service:**
- Users can change password, rotate local PIN, review active or trusted devices, and revoke offline trust from their own devices without needing full administrative access
- Device revocation, PIN setup, and other security-sensitive self-service actions can require current-password or step-up confirmation depending on tenant policy from 6.18
- Personal session history shows recent sign-in, lock, unlock, offline-entry, and revocation events from 6.1 so users can detect unexpected activity on their own account

**Notification & Workspace Preferences:**
- Users can adjust personal notification channels and mute windows only within the boundaries set by 6.14 and 6.18; admin-mandated critical categories remain non-optional
- Theme, language, home module, density, and saved default views are stored as personal preferences without overwriting tenant-wide defaults
- Saved filters and view presets help users reopen their preferred work queues quickly without requiring custom admin configuration for each user

**My Readiness Summary:**
- Self-service includes read-only visibility into current work assignments, expiring certifications, required acknowledgements, upcoming training sessions, and blocked-readiness signals sourced from 6.16 and 6.20
- The module is intentionally personal: users can see what affects their own readiness and workload, but they cannot change role, scope, or qualification state from here

**Permissions:** No special permission is required for a user to access and manage their own self-service workspace. Editing another user's profile or forcing device revocation for another account requires `adm.users`.

---

### 6.20 Training, Certification & Habilitation Management

**Objective:** Govern certifications, qualifications, habilitations, and training evidence so only properly qualified personnel can be planned, assigned, permitted, and released into hazardous or regulated work. This module is both a compliance register and an execution-readiness gate.

**Data Entities:**
- `certification_types`: id, code, name, domain (electrical/atex/height/confined_space/chemical/forklift/radiological/internal/regulatory), validity_months, is_renewable, requires_practical_exam, regulatory_reference, description, refresh_ack_document_category (nullable)
- `position_required_certifications`: id, position_id, certification_type_id, requirement_level (mandatory/recommended)
- `qualification_requirement_profiles`: id, source_type (position/job_plan/work_order/permit_type/pm_plan/task_template), source_id, certification_type_id, requirement_level (mandatory/conditional/recommended), exception_type (no_override/supervisor_override/manager_override), document_ack_required (boolean)
- `personnel_certifications`: id, personnel_id, certification_type_id, issued_date, expiry_date, issuing_authority, certificate_number, certificate_file_path, verification_status (pending/verified/rejected), status (valid/expiring_soon/expired/pending_renewal/suspended/awaiting_document_ack), linked_training_session_id, notes
- `training_sessions`: id, title, training_type (internal/external/e_learning/regulatory_exam), certification_type_id (nullable), trainer_name, planned_date, planned_end_date, duration_hours, location, max_participants, entity_id, status (planned/in_progress/completed/cancelled), notes
- `training_participants`: id, session_id, personnel_id, registration_status (registered/attended/passed/failed/no_show), score (0-100), passed_at, certificate_issued, notes
- `training_needs`: id, personnel_id, certification_type_id, identified_date, identified_by_id, target_date, priority (mandatory_regulatory/mandatory_role/recommended), needs_source (gap_analysis/job_plan_requirement/work_order_requirement/permit_requirement/document_revision/audit_finding/supervisor_request), status (open/in_training/completed/deferred)
- `competency_evaluations`: id, personnel_id, source_type, source_id, evaluated_at, clearance_status (qualified/missing_cert/expired/suspended/awaiting_document_ack/blocked_by_policy), blocking_reason
- `qualification_overrides`: id, competency_evaluation_id, approved_by_id, approved_at, reason_code, reason_note, valid_until

**Features:**

**Certification & Qualification Matrix:**
- Full grid: personnel (rows) x certification types (columns); cell status includes valid, expiring soon, expired, suspended, awaiting document acknowledgement, not required, and not held
- Filter by entity, team, domain, and requirement source; export as PDF/Excel for external audits
- Click any cell to open full evidence detail: issuer, issue date, expiry, verification status, linked certificate file, related training session, and blocking impacts on current or planned work

**Assignment, Planning & Permit Gates:**
- Qualification requirement profiles can originate from positions, PM tasks, job plans, work orders, and permit types; these requirements are evaluated during planning, assignment, dispatch, and permit activation
- Planning and scheduling (6.16) treats missing or expired qualifications as readiness blockers instead of passive warnings
- Work order and permit flows cannot assign or activate disallowed personnel when `exception_type = no_override`
- Emergency overrides are allowed only where policy permits; every override requires approver, reason, expiry, and audit trail in `qualification_overrides`

**Document Acknowledgement Link:**
- Critical procedure revisions from module 6.15 can set a certification or qualification state to `awaiting_document_ack` until the required acknowledgement is completed
- Required acknowledgements can create `training_needs` records automatically when a major procedural revision affects a hazardous or regulated task domain
- The module distinguishes expiry-driven non-compliance from acknowledgement-driven temporary clearance blocks

**Expiry & Competence Dashboard:**
- KPI counters: expiring in 7 / 15 / 30 / 60 days, expired, suspended, awaiting acknowledgement, and blocked active/planned work items
- Priority-sorted list shows technician, certification, expiry or block reason, and impact on planned WOs, PM tasks, or permits
- Auto-notifications use module 6.14 with configurable lead times and escalation for expired or blocked high-risk qualifications

**Training Session Management:**
- Create and manage training sessions with participant registration and capacity controls
- Training calendar sync: sessions appear on the planning board and block participants from work assignment during attendance windows
- Attendance, pass/fail, and exam scoring are captured per participant
- Completing a passed session can create or renew certification evidence automatically; verification status remains visible until reviewed where policy requires

**Training Needs Analysis:**
- Gap analysis compares position, work-package, permit, and document-driven requirements against actual personnel evidence
- Needs register prioritizes mandatory regulatory gaps, mandatory role gaps, and recommendations separately, with source traceability
- Supervisors can generate focused plans such as "all missing confined-space qualifications for next shutdown" or "all electricians awaiting revised LOTO acknowledgement"

**Regulatory & Audit Reporting:**
- Exportable compliance reports show certification status, issuing authority, evidence reference, override history, and blocked-work impacts
- Historical qualification and override events remain available for audit and incident investigation

**Permissions:** `trn.view` / `trn.manage` / `trn.report` / `trn.override`

---

### 6.21 IoT Integration Gateway

**Objective:** Govern trusted industrial telemetry ingestion, runtime accumulation, and derived condition events so Maintafox can use shop-floor signals for condition-based maintenance, reliability analysis, inspection prioritization, and work initiation without becoming a generic SCADA replacement.

**Data Entities:**
- `iot_gateway_profiles`: id, name, connection_profile_id, protocol_family (modbus_tcp/modbus_rtu/opcua_client/mqtt_client/http_poller), edge_mode (direct/edge_buffered/gateway_proxy), expected_heartbeat_seconds, status (draft/tested/active/error/suspended), last_seen_at, last_error_summary
- `iot_signal_definitions`: id, gateway_profile_id, equipment_id, point_code, source_path, semantic_type (condition/process/runtime/energy/state), data_type (float/int/bool/string), engineering_unit, expected_cadence_seconds, deadband_value, quality_policy (strict/permissive), is_counter, is_active
- `iot_signal_observations`: id, signal_definition_id, source_timestamp, ingested_at, numeric_value, text_value, quality_status (good/uncertain/bad/stale/offline/simulated), sequence_no, buffer_run_id, was_backfilled
- `iot_rule_profiles`: id, name, rule_type (threshold/persistence/rate_of_change/state_change/composite/offline), severity (info/warning/critical), logic_json, hysteresis_json, cooldown_seconds, min_duration_seconds, required_quality_min, status (draft/tested/active/retired), activated_at
- `iot_condition_events`: id, rule_profile_id, equipment_id, opened_at, last_evidence_at, closed_at, event_status (open/acknowledged/resolved/suppressed), trigger_summary, evidence_window_json, notification_event_id, follow_up_di_id, follow_up_work_order_id
- `iot_counter_updates`: id, signal_definition_id, pm_counter_id, source_window_start, source_window_end, delta_value, applied_at, reset_detected, rejected_reason
- `iot_buffer_runs`: id, gateway_profile_id, started_at, completed_at, records_received, records_replayed, records_rejected, max_lag_seconds, status

**Features:**

**Edge-Aware Acquisition & Buffering:**
- Protocol support includes Modbus polling, OPC UA subscriptions and events, MQTT topic subscriptions, and HTTP polling; endpoint secrets and credential testing are governed through 6.18 connection profiles
- Store-and-forward buffering preserves source order, source timestamp, and replay status during network loss or gateway outage; backlog size, replay lag, TTL risk, and disk-pressure state remain visible to operators
- MQTT deployments can optionally use Sparkplug-style session awareness so gateway and device birth/death state becomes a first-class health signal instead of a silent disconnect

**Signal Semantics & Asset Binding:**
- Each signal is bound to equipment, optional component position, semantic type, engineering unit, and expected cadence; normalization and code alignment reuse governed reference mappings from 6.13
- The module distinguishes stale, bad-quality, simulated, offline, and replayed data from trusted live measurements; low-quality or backfilled data may be displayed while remaining ineligible for automated work triggers where policy forbids it
- Read-oriented telemetry ingestion is the default operating model; sending control commands back to PLC or SCADA infrastructure is outside default scope and must not be assumed by this module

**Derived Condition Logic:**
- Rules support static thresholds, persistence windows, deadband and hysteresis, rate-of-change, state transitions, and composite conditions across multiple signals
- A trigger creates an `iot_condition_event` first; downstream actions can then notify, create a DI, prioritize inspection, or update planning readiness according to rule policy
- Automatic WO creation is limited to governed cases such as PM counter fulfillment or explicitly approved high-confidence rules; Maintafox should not open corrective WOs from one noisy reading

**Counters, Trends & Evidence Packs:**
- Runtime, energy, and cycle counters can update `pm_counters` with reset and rollover detection plus a full audit trail via `iot_counter_updates`
- Trend charts retain recent raw readings and older aggregates; evidence windows around triggered events are pinned so investigators keep pre- and post-trigger context even after compaction
- DIs, WOs, inspections, and permits can be overlaid on signal timelines to support causal review and condition-based planning decisions

**Health, Notifications & Auditability:**
- Gateway health view shows heartbeat state, buffer lag, bad-quality streaks, rejected records, and last successful replay window
- High-risk anomalies and gateway failures route through 6.14 notification rules and 6.17 audit capture with acknowledgement, suppression, and resolution history
- Reliability (6.10), PM (6.9), planning (6.16), and inspections (6.25) consume derived events and counter evidence rather than raw unqualified telemetry alone

**Permissions:** `iot.view` / `iot.configure` / `iot.respond`

---

### 6.22 ERP & External Systems Connector

**Objective:** Govern master-data synchronization, transactional handoff, and official posting between Maintafox and ERP or external business systems through versioned integration contracts, auditable mapping logic, and idempotent inbox/outbox processing. This module defines source-of-record boundaries rather than promising unrestricted bidirectional sync.

**Data Entities:**
- `external_system_profiles`: id, name, system_family (sap_s4hana/sap_ecc/oracle_erp_cloud/oracle_jde/d365_fo/d365_bc/ifs_applications/infor_csi/epicor_kinetic/sage_x3/odoo/custom_rest/custom_middleware), connection_profile_id, transport_type (odata_v4/rest_json/soap/rfc/graphql/file_sftp/webhook), auth_model, operating_mode (import_only/export_only/bidirectional), status (draft/tested/active/suspended/error)
- `integration_contracts`: id, profile_id, domain (equipment/personnel/supplier/material/cost_center/stock_transaction/purchase_requisition/purchase_order/work_order_cost/project_time), direction, source_of_record (external/maintafox/split), cursor_strategy (full/delta/high_watermark/webhook_event), schedule_policy_json, status (draft/tested/active/suspended), activated_at
- `mapping_profile_versions`: id, contract_id, version_no, maintafox_entity, external_entity, field_map_json, transform_library_json, validation_rules_json, status (draft/tested/active/retired), activated_at
- `external_record_links`: id, contract_id, maintafox_table, maintafox_record_id, external_record_key, external_version_token, last_synced_at, field_authority_json, sync_state (linked/pending_create/pending_update/pending_archive/error)
- `sync_batches`: id, contract_id, run_mode (manual/scheduled/webhook/dry_run/replay), direction, window_start, window_end, status (queued/running/completed/partial_failed/failed/suspended), records_seen, records_applied, records_rejected, idempotency_scope, error_summary
- `sync_batch_items`: id, batch_id, external_record_key, maintafox_record_id, operation (create/update/post/ack/archive), payload_hash, processing_status, retry_count, failure_reason
- `integration_exceptions`: id, batch_item_id, exception_type (validation/conflict/missing_reference/schema_drift/auth/rate_limit/posting_rejected), severity, maintafox_value_snapshot, external_value_snapshot, resolution_status (open/retried/ignored/merged/resolved), resolved_by_id, resolved_at
- `external_event_inbox`: id, profile_id, event_type, source_event_id, received_at, payload_hash, signature_status (verified/failed/not_applicable), processing_status, related_batch_id

**Representative System Families:**

| Family | Representative Systems | Typical Maintafox Domains |
|---|---|---|
| SAP ecosystem | SAP S/4HANA Cloud & On-Premise, SAP ECC 6.x | equipment masters, materials, cost centers, maintenance actuals |
| Oracle ecosystem | Oracle ERP Cloud, Oracle JD Edwards EnterpriseOne | fixed assets, purchasing, inventory, HR, project costs |
| Microsoft ecosystem | Microsoft Dynamics 365 Finance & Operations, Microsoft Dynamics 365 Business Central | items, dimensions, procurement, HCM, project accounting |
| Industrial ERP ecosystem | IFS Applications / IFS Cloud, Infor CloudSuite Industrial, EPICOR Kinetic | maintenance objects, inventory, requisitions, projects |
| Mid-market and custom | Sage X3, Odoo, custom REST or middleware-backed systems | suppliers, stock, personnel, costs, bespoke master data |

**Features:**

**Integration Contract Governance:**
- Connection secrets, base URLs, and credential tests live in 6.18 connection administration; 6.22 owns domain contracts, mapping versions, sync windows, and reconciliation policy
- Every contract explicitly defines source-of-record and field authority rules; the same field must not be edited bidirectionally without an explicit split-ownership policy
- Contract activation follows Discover -> Preview -> Test -> Activate -> Monitor workflow; mapping revisions are versioned and prospective rather than silently rewriting sync history

**Metadata-Aware Mapping & Import Preview:**
- Where supported, the connector inspects external metadata or schema endpoints such as OData service roots and `$metadata` documents to pre-populate entities, field lists, and validation hints
- Import preview surfaces create, update, deactivate, and reject outcomes before apply; governed alias and code mappings from 6.13 are used instead of ad hoc string matching
- External IDs, version tokens, and company or site scope values are preserved so repeat imports remain delta-aware, replay-safe, and cross-company capable where the external platform supports it
- Equipment import and history exchange can align to ISO 15926 / CFIHOS structures and ISO 14224 failure-taxonomy mappings where the external system exposes them

**Outbound Posting & Idempotent Sync:**
- Outbound flows use queued batches with idempotency keys, payload hashes, retry policy, and per-record acknowledgement capture for reservations, requisitions, inventory movements, work-order actuals, and project time
- Maintafox distinguishes locally recorded operational actuals from externally accepted official postings; 6.24 can therefore report provisional versus posted values without falsifying financial state
- Scheduled sync, manual dispatch, webhook ingestion, and replay use the same batch and item audit model so operators can trace exactly what was sent, received, skipped, or retried

**Exception Management & Reconciliation:**
- Conflict resolution shows external value, Maintafox value, field authority, last sync token, and the active contract version before an operator retries, merges, or accepts a winning side
- Exceptions separate business-rule failures from auth, rate-limit, and schema-drift problems so teams can fix the real root cause instead of repeatedly replaying broken batches
- Failed or delayed critical integrations route through 6.14 notifications and 6.17 audit history with direct drill-down to batch and item evidence

**API & Middleware Compatibility:**
- Maintafox exposes scoped OpenAPI 3.0 REST endpoints and secure webhook receivers for middleware-driven or event-driven integration patterns
- Supported deployment patterns include MuleSoft, Boomi, Azure Integration Services, WSO2, IBM App Connect, and SAP Integration Suite; these are transport options, not substitutes for contract governance inside Maintafox
- Enterprise-only capabilities include externally callable write APIs, inbound signed webhooks with HMAC-SHA256 verification, and scheduled financial-posting automation profiles

**Permissions:** `erp.view` / `erp.manage` / `erp.sync` / `erp.reconcile` - Enterprise tier required for outbound posting, inbound webhooks, and middleware-grade automation

---

### 6.23 Work Permit System - LOTO / Permit-to-Work

**Objective:** Formalized Lockout/Tagout and Permit-to-Work control system that governs hazardous maintenance work from request to safe hand-back. The module must hard-gate execution of dangerous work until required isolations, tests, approvals, PPE, and handover controls are complete. Compliant with ISO 45001:2018 principles and applicable local safety regulations.

**Data Entities:**
- `permit_types`: id, name, code (loto/hot_work/cold_work/confined_space/electrical_hv/electrical_lv/chemical/work_at_height/radiological), description, requires_hse_approval, requires_operations_approval, requires_atmospheric_test, max_duration_hours, mandatory_ppe_ids (JSON array), mandatory_control_rules_json
- `work_permits`: id, code (PTW-YYYY-NNNN), work_order_id (nullable), permit_type_id, equipment_id, entity_id, description, work_scope, status (draft/pending_review/approved/issued/active/suspended/revalidation_required/closed/handed_back/cancelled/expired), requested_by_id, issued_by_id, activated_by_id, suspended_by_id, handed_back_by_id, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at
- `permit_hazard_assessments`: id, permit_id, hazard_type, hazard_description, risk_level, control_measure, verification_required
- `permit_isolations`: id, permit_id, isolation_point, energy_type, isolation_method, applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at
- `permit_tests`: id, permit_id, test_type (atmospheric/gas/voltage/pressure/other), result_value, unit, acceptable_min, acceptable_max, tested_by_id, tested_at, is_pass
- `permit_checkpoints`: id, permit_id, checkpoint_type (isolation/de_energization/atmospheric_test/ppe_donning/post_work_clearance/de_isolation), sequence_order, description, is_mandatory, is_completed, completed_by_id, completed_at, evidence_note
- `permit_suspensions`: id, permit_id, reason, suspended_by_id, suspended_at, reinstated_by_id, reinstated_at, reactivation_conditions
- `permit_handover_logs`: id, permit_id, handed_from_role, handed_to_role, confirmation_note, signed_at
- `permit_witnesses`: id, permit_id, witness_role (hse_officer/supervisor/authorized_person), personnel_id, signed_at

**Default Permit Workflow:**
```
Draft -> Pending Review -> Approved -> Issued -> Active
Active -> [Suspended -> Revalidation Required -> Issued -> Active]
Active -> Closed -> Handed Back
Any pre-handback state -> Cancelled
Any issued/active state -> Expired (requires revalidation before reuse)
```

**Features:**
- **Multi-step permit form:** guided workflow covering equipment and work scope, hazard identification, energy sources, isolation points, PPE, tests, emergency references, expiry, and responsible roles
- **Type-specific control rules:** each permit type can require different approvals, isolation evidence, atmospheric tests, witness signatures, or hand-back conditions
- **Issued vs active distinction:** approval authorizes issue, but the permit does not become active until mandatory checkpoints, tests, and verification steps are complete
- **Checkpoint and isolation execution panel:** mobile-friendly view where each control step is signed off by the responsible person; completed steps are immutable and time-stamped
- **Active permit board:** live dashboard of all active PTWs with countdown to expiry, suspension indicators, and overdue hand-back warnings
- **Suspension and revalidation workflow:** changed conditions, alarms, or interrupted work force a suspension; restarting requires explicit revalidation rather than silent resume
- **Formal hand-back:** closing the job technically is not enough; the permit must be handed back to operations with confirmed de-isolation and site-clear status before the permit chain is complete
- **Work Order gate:** if a WO requires a permit type, the WO cannot transition to `In Progress` until the required PTW is `Active`; enforced at the Rust workflow layer
- **QR code per permit:** printed permit sheet links to live permit state and checkpoint history for field verification and audits
- **LOTO card printing:** generate a print-ready LOTO card per isolation point for physical tagging of energy sources
- **Compliance reporting:** monthly report of permits by type, active duration, expiry rate, suspension rate, checkpoint completion quality, and hand-back completeness for HSE review

**Permissions:** `ptw.view` / `ptw.request` / `ptw.approve.hse` / `ptw.approve.ops` / `ptw.close`

---

### 6.24 Budget & Cost Center Management

**Objective:** Govern maintenance budget baselines, actual cost events, commitments, and reforecasts so maintenance spend can be controlled by cost center, entity, asset group, and work category without losing the provenance of where costs came from. This module is a maintenance cost-control layer, not a generic accounting ledger.

**Data Entities:**
- `cost_centers`: id, name, code, entity_id, parent_cost_center_id, budget_owner_id, erp_external_id, is_active
- `budget_versions`: id, fiscal_year, scenario_type (original/approved/reforecast/what_if), version_no, status (draft/submitted/approved/frozen/closed/superseded), currency, created_at, created_by_id, approved_at, approved_by_id
- `budget_lines`: id, budget_version_id, cost_center_id, period_month (1-12; NULL = annual total), budget_bucket (labor/parts/services/contracts/shutdown/capex/other), planned_amount, source_basis (manual/prior_year_actual/pm_forecast/shutdown_plan/erp_import), justification_note
- `budget_actuals`: id, cost_center_id, period_start, period_end, budget_bucket, amount, source_type (work_order_labor/work_order_parts/work_order_services/work_order_tools/purchase_receipt/contract_call_off/manual_adjustment), source_id, work_order_id, equipment_id, posting_status (provisional/posted/reversed), posted_at
- `budget_commitments`: id, cost_center_id, period_month, budget_bucket, amount, source_type (approved_po/contract_reservation/shutdown_package), source_id, expected_posting_date, created_at
- `budget_forecasts`: id, budget_version_id, cost_center_id, period_month, forecast_amount, forecast_method (burn_rate/pm_occurrence/shutdown_loaded/manual), confidence_level, generated_at
- `budget_variance_reviews`: id, budget_version_id, cost_center_id, period_month, variance_amount, variance_pct, driver_code, action_owner_id, review_status, reviewed_at
- `budget_alert_configs`: id, cost_center_id, threshold_pct (80, 100, 120), alert_type (warning/critical), recipient_ids (JSON array), is_active

**Budget Lifecycle:**
```
Draft -> Submitted -> Approved -> Frozen
Frozen -> Reforecasted through a new approved version
Approved/Frozen -> Closed at fiscal-year or period end
```

**Features:**
- **Versioned budget baselines:** support original budget, approved control budget, and in-year reforecast versions; only one frozen control baseline per fiscal year and scenario drives alerts and dashboard variance status
- **Governed cost-event rollup:** labor, parts, services, tools, PO receipts, and contract call-offs flow into `budget_actuals` with source provenance; manual adjustments require coded reason and approver
- **Provisional vs. posted actuals:** WO-related costs remain provisional until required execution actuals and closure-quality fields are complete; only posted actuals feed official reporting and ERP export
- **Commitment plus actual view:** approved POs, contract reservations, and shutdown packages are tracked separately from posted actuals so future spend is visible before invoice or receipt
- **Variance dashboard and review workflow:** planned vs. committed vs. actual vs. forecast per cost center, month, and bucket; overspend or underspend beyond threshold opens a variance review with driver code and accountable owner
- **Planned-work forecast integration:** PM occurrences, shutdown packages, and ready-backlog labor/parts projections generate forecast lines with explicit method and confidence level
- **Spend mix analysis:** spend is sliced by corrective, preventive, inspection, compliance, improvement, shutdown, and capex context so management can see whether spend is reactive or strategic
- **Cost-of-failure view:** combine repeat failures, downtime, labor, parts, and services to expose the cumulative cost burden of chronic assets and recurring failure modes
- **Budget alerts:** in-app, email, or notification-center escalation at configurable thresholds (default: 80%, 100%, 120% of control budget); alert history and acknowledgement retained
- **Cost center report:** fully filterable and exportable PDF/Excel; includes baseline budget, commitments, posted actuals, forecast, variance drivers, corrective/preventive split, and top spending assets or WOs
- **ERP alignment:** import official cost-center master data and optional budget baselines from ERP; export posted actuals and approved reforecast snapshots without requiring all maintenance users to work in the ERP
- **Multi-currency support:** source currency and base-currency values are stored using governed exchange rates from Settings; reports can display both values where relevant

**Permissions:** `fin.view` / `fin.budget` / `fin.report`

---

### 6.25 Inspection Rounds & Checklists

**Objective:** Define, schedule, execute, review, and trend recurring inspection rounds as a structured anomaly-detection and condition-evidence workflow. This module replaces paper rounds with governed inspection evidence that can trigger DI, WO, PM review, or permit review when conditions warrant it.

**Data Entities:**
- `inspection_templates`: id, code, name, entity_id, route_scope, estimated_duration_minutes, is_active, current_version_id
- `inspection_template_versions`: id, template_id, version_no, effective_from, checkpoint_package_json, tolerance_rules_json, escalation_rules_json, requires_review
- `inspection_checkpoints`: id, template_version_id, equipment_id, component_id (nullable), sequence_order, checkpoint_code, description, check_type (pass_fail/measurement/visual_observation/photo_required/text_entry), measurement_unit, normal_min, normal_max, warning_min, warning_max, requires_photo, requires_comment_on_exception
- `inspection_rounds`: id, template_id, template_version_id, scheduled_at, assigned_to_id, started_at, completed_at, reviewed_at, reviewed_by_id, completion_percentage, status (scheduled/released/in_progress/completed/completed_with_findings/reviewed/missed/cancelled)
- `inspection_results`: id, round_id, checkpoint_id, result_status (pass/warning/fail/not_accessible/not_done), boolean_value, numeric_value, text_value, comment, recorded_at, recorded_by_id
- `inspection_evidence`: id, result_id, evidence_type (photo/file/reading_snapshot/signature), file_path_or_value, captured_at
- `inspection_anomalies`: id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, requires_permit_review, resolution_status

**Features:**
- **Versioned template builder:** define a named inspection route with an ordered list of checkpoints, threshold rules, and escalation behavior; updates create a new template version instead of overwriting history
- **Execution interface (tablet/touchscreen optimized):** full-screen single-checkpoint layout with large touch targets, swipe/next navigation, camera integration, numeric keypad for measurements, and immediate save for crash-safe operation
- **Offline execution:** rounds can be started and completed without internet connectivity; results sync when connectivity returns
- **Typed evidence capture:** store pass/warning/fail outcomes, numeric values, comments, and photo or file evidence separately instead of collapsing everything into one text field
- **Exception evidence rules:** warning or fail results can require photo, comment, signature, or all three based on checkpoint configuration
- **Automatic anomaly creation:** abnormal readings or failed checkpoints create anomaly records distinct from raw results; anomalies can route automatically to DI, WO, or permit review based on severity and rule set
- **Reviewed state:** rounds with findings enter a review step so supervisors or reliability users confirm follow-up actions and close anomaly routing gaps
- **Missed and late-round governance:** late start, missed round, and inaccessible checkpoint conditions are tracked explicitly for compliance analysis
- **Round history and trends:** full audit trail of rounds, anomalies, follow-up conversions, and technician completion patterns; filter by template, entity, date range, technician, or anomaly type
- **Compliance KPIs:** % of rounds completed on time, % reviewed after findings, average anomalies per round, repeat anomaly rate, and open follow-up count by route
- **Measurement trend tracking:** numeric checkpoints show history sparklines and warning-limit approach trends so the module can support early-condition detection rather than only binary failure capture
- **Template library:** pre-built templates for startup checks, handover inspections, lubrication checks, electrical panels, and other recurring field routines; customizable per tenant

**Permissions:** `ins.view` / `ins.execute` / `ins.manage`

---

### 6.26 Configuration Engine & Tenant Customization

**Objective:** Provide a governed, admin-managed runtime configuration layer that lets each tenant shape structure, workflows, forms, terminology, numbering, and UI behavior without code changes or redeployments, while preserving auditability, historical meaning, and the minimum analytical data required for serious maintenance management.

This module is the backbone of Maintafox's adaptability and is accessible exclusively to users holding `cfg.*` permissions. The core rule is not "everything can be changed freely" but rather: **every permitted variation is stored in the database, versioned, validated before publish, and prevented from breaking the product's analytical kernel.**

---

#### 6.26.1 Workflow State Machine Designer

**Principle:** Every stateful module (DI, OT, PTW, inspection rounds, PM executions) runs on a configurable workflow, but each tenant-defined state must map to a protected semantic macro-state so analytics, cross-module logic, and audit reporting keep stable meaning.

**Data Entities:**
- `workflow_definitions`: id, module (di/ot/ptw/ins_round/pm_plan), name, description, version_no, semantic_profile, is_draft, is_active, is_default, supersedes_workflow_id, created_by_id, created_at, published_at
- `workflow_states`: id, workflow_id, code (machine-readable, unique per workflow), label, semantic_macro_state (requested/under_review/approved/scheduled/waiting/executing/completed/verified/closed/cancelled/archived), color_hex, is_initial (exactly one per workflow), is_terminal (at least one per workflow), is_system, sequence_hint
- `workflow_transitions`: id, workflow_id, from_state_id, to_state_id, action_label, required_permission_id (nullable), requires_comment (boolean), guard_rule_json, requires_checklist_complete (boolean), requires_verification (boolean), is_system_guarded
- `workflow_state_history`: id, module, record_id, workflow_definition_id, from_state_id, to_state_id, transitioned_by_id, comment, transitioned_at *(immutable audit log)*

**Features:**
- **State designer UI:** drag canvas with labeled state nodes and directed transition arrows; add/rename/deactivate tenant states; system states remain protected
- **Semantic guardrails:** every configured state must map to a macro-state understood by the product; invalid mappings are rejected before publish
- **Transition rule builder:** for each transition, optionally assign required permission, mandatory comment, guard rules, checklist completion requirements, verification requirements, and prerequisite checks
- **Draft -> validate -> publish lifecycle:** workflow edits are made in draft, validated, simulated, and then published as a new version; existing records retain the workflow version they already use
- **Impact preview:** before publishing a workflow change, the system shows affected modules, states removed or renamed, record counts by state, and any required migration mappings
- **Safety constraints:** at least one initial state, at least one terminal state, no unreachable states, no orphaned transitions, and no removal of protected macro-state coverage
- **Test simulator:** input a sequence of transitions and the system validates them against rules; shows pass/fail per step with guard evaluation reason
- **Deactivate instead of delete:** states already used by historical records can be retired from future use but not physically deleted

---

#### 6.26.2 Dynamic Priority & Risk Level Configuration

**Principle:** Priority, urgency, criticality, severity, and risk scales are configurable, but their versions and semantic intent must remain traceable so historical dashboards and SLA logic stay coherent.

**Data Entities:**
- `level_configs`: id, config_set_id, config_type (priority/urgency/criticality/risk/severity/sil_target), level_value (integer 1-N), label, semantic_band (very_low/low/medium/high/critical), color_hex, text_color_hex, icon_name, description, is_active, sort_order
- `level_config_sets`: id, name, config_type, version_no, is_draft, is_default, module_associations (JSON array of module codes)

**Features:**
- **Level editor:** add/remove levels (minimum 2, maximum 10 per type); set label, color, semantic band, and sort order; drag-to-reorder
- **Module binding:** assign a configuration set to specific modules (e.g., DI uses one urgency scale while FMECA severity uses another)
- **Preview panel:** shows how the configured levels appear in the DI list, FMECA matrix, inspection anomaly badge, and planning board before publish
- **Migration tool:** if the level structure changes, a migration wizard maps old values to new values before publish; no live data is orphaned
- **Version-aware reporting:** historical records keep the level values and set version they were created with, while dashboards can optionally remap them to the current semantic band for comparison

---

#### 6.26.3 Form Rule, Template & Custom Fields Engine

**Principle:** Forms are rule-driven surfaces composed of system fields, tenant custom fields, and template presets. Field behavior may vary by role, workflow stage, record type, and work type, but protected analytical fields cannot be hidden or downgraded below the system minimum.

**Data Entities:**
- `custom_field_definitions`: id, entity_type (equipment/di/ot/personnel/article/inspection_template), field_code (slug, unique per entity type), label, field_type (text/integer/decimal/date/boolean/select/multi_select/url/email), data_classification (analytical_context/narrative_only), default_value_json, options_json, tooltip, is_active, requires_permission_id (nullable)
- `custom_field_values`: id, entity_type, entity_id, field_definition_id, value_text, value_integer, value_decimal, value_date, value_boolean, value_json *(one row per entity x field)*
- `form_rule_sets`: id, entity_type, scope_type (global/role/workflow_state/work_type/user_group/template), scope_reference, version_no, is_draft, is_active
- `form_field_rules`: id, rule_set_id, field_code, visibility (hidden/optional/required/read_only), default_value_json, display_order, show_in_list (boolean), show_in_export (boolean), show_in_filters (boolean)
- `template_presets`: id, entity_type, name, applies_to_work_type, preset_json, is_active

**Features:**
- **Field and form builder UI:** configure system fields and custom fields together instead of treating custom fields as a separate afterthought
- **Stage-specific governance:** configure Required / Optional / Hidden / Read Only behavior by workflow state, work type, role, or user group
- **Template presets:** pre-populate fields, freeze read-only values, or hide irrelevant fields for recurring jobs, inspections, permits, and standardized close-out scenarios
- **Protected analytical kernel:** core fields such as IDs, timestamps, asset/location context, state history, actual labor, parts actuals, delay segments, downtime, and verification cannot be removed from the minimum required model where the product depends on them
- **In-form integration:** custom and governed fields render inline in the configured order; field groups can be collapsed into logical sections without affecting validation behavior
- **Filter, search, and export control:** admins choose which fields appear in list filters, exports, and analytics, while narrative-only fields remain available for context without polluting KPI logic
- **Validation:** Rust-layer validation enforces field types, required rules, select membership, date constraints, and stage-specific guards; invalid values return typed errors to the frontend
- **Preview mode:** admin can preview a form as a requester, technician, planner, or supervisor before publishing changes

---

#### 6.26.4 Terminology Override Layer

**Principle:** Every system term visible in the UI is driven by an i18n key. A customer-specific override can replace display labels without rebuilding the app, but it cannot alter the underlying semantic codes, audit labels, or API field identities used for reporting and traceability.

**Data Entities:**
- `term_overrides`: id, locale (fr-DZ/en-US/ar-DZ), i18n_key (e.g., `module.di.title`), default_value (read-only reference), override_value, updated_by_id, updated_at

**Features:**
- **Terminology editor:** searchable table of all UI keys and default values; admin can rename business-facing terms (e.g., DI -> Work Request or Signalement)
- **Bulk import:** upload a CSV of `i18n_key, override_value` pairs for onboarding or terminology migration
- **Preview mode:** apply terminology overrides in a preview session before publish
- **Reset:** any override can be individually reset to the system default; full rollback supported by publishing the previous version
- **Per-locale support:** overrides can be set independently per language

---

#### 6.26.5 UI Layout & Module Visibility Engine

**Principle:** Role-level module visibility and per-user dashboard/widget layout are configurable at runtime, but configuration scope is explicit so tenant defaults, role defaults, and personal overrides do not conflict invisibly.

**Data Entities:**
- `ui_role_layouts`: id, role_id, module_code, is_visible (boolean), sidebar_order (integer)
- `ui_user_layouts`: id, user_id, widget_code, is_visible (boolean), column (1/2), row_order (integer), width (half/full)
- `ui_role_widget_templates`: id, role_id, widget_code, is_visible, column, row_order, width *(template applied to new users of this role)*

**Module visibility (Role Level):**
- Admin selects a role; the configuration panel shows all 26 modules as toggles with drag-to-reorder sidebar position
- Hiding a module removes it from the sidebar and blocks protected IPC commands for that module for users of that role
- Visibility changes take effect on next login or after explicit refresh
- System-required modules (Auth, Settings, Profile, RBAC) cannot be hidden

**Dashboard widget layout (User Level):**
- The Analytics dashboard (6.11) renders widgets from the user's `ui_user_layouts` configuration
- Available widgets: KPI counters (DI open, OT overdue, PM compliance, inventory alerts, PTW active, repeat failures, budget variance), charts (DI trend, WO closure rate, core reliability trend, cost per month), Gantt snapshot, notification feed, recent activity feed, quick-create shortcuts
- Stage-gated reliability widgets: advanced reliability widgets such as FMECA heatmaps, RCM action queues, or Weibull trend views appear only when the corresponding reliability phase is enabled for the tenant and allowed for the user's role
- Per-user configuration: show/hide individual widgets, set full-width or half-width, drag to reorder within column
- **Admin push layout:** an admin can push a role template to all users of that role, replacing their current personal layout; users may re-personalize afterward
- **Scoped defaults:** tenant default -> role template -> user override precedence is explicit and visible in the UI

---

#### 6.26.6 Module Enable / Disable

**Principle:** Entire modules can be disabled at the tenant level if not subscribed to, not deployed, or not needed, but dependency analysis must prevent disabling a module that would break active workflows or required analytics.

**Data Entities:**
- `module_states`: id, module_code, is_enabled (boolean), disabled_reason (optional note), disabled_by_id, disabled_at

**Features:**
- **Module toggle panel:** admin sees all 26 modules with on/off toggle and optional disable reason; built-in dependency warnings (e.g., disabling IoT Gateway also disables condition-based PM triggers)
- Disabled modules are hidden from all users' sidebars and their IPC commands return `MODULE_DISABLED`
- Module state is checked at app startup and after license heartbeat response; the license server can enforce module disablement via feature flags
- **Dependency graph preview:** before a module is disabled, the system shows impacted workflows, dashboards, rules, and integrations
- Useful for: disabling RAMS on a Starter/Professional license, disabling IoT if no OT hardware is present, disabling PTW in office-only environments

---

#### 6.26.7 Reference Number / Sequence Configuration

**Principle:** Auto-generated codes (DI-2026-000001, OT-XXXX, PTW-YYYY-NNNN, etc.) are configurable per module and per year, and can optionally vary by structure scope where operational practice requires site- or node-specific numbering.

**Data Entities:**
- `sequence_configs`: id, module_code (di/ot/pm/ptw/ins/per/art/po), scope_type (global/org_node/site), scope_reference, prefix, use_year_in_code (boolean), year_reset (boolean), padding_digits (3-8), current_value, last_reset_at, updated_by_id

**Features:**
- **Sequence editor:** admin sets prefix string, year inclusion, annual reset, optional scope, and zero-padding width; a live preview shows the next generated code
- **Manual reset:** admin can reset any sequence counter with confirmation; the operation is logged in the audit trail with old/new value and reason
- **Conflict prevention:** the Rust layer uses transactional locking semantics to guarantee uniqueness even under concurrent inserts
- **Scope-aware numbering:** enterprise tenants can choose global numbering or separate sequences by site or configured operating node where local practice requires it

---

#### 6.26.8 Configuration Governance, Versioning & Safe Publish

**Principle:** Configuration is edited in draft, validated, impact-analyzed, and then published as a versioned change set. Historical records always preserve the configuration version that governed them when they were created or transitioned.

**Data Entities:**
- `config_change_sets`: id, config_domain (workflow/levels/forms/terms/layout/modules/sequences/structure), name, description, created_by_id, created_at, published_by_id, published_at, status (draft/validated/published/rolled_back)
- `config_impact_reports`: id, change_set_id, affected_module, affected_record_count, risk_level, summary_json, generated_at
- `config_publish_events`: id, change_set_id, action (validate/publish/rollback/export/import), action_by_id, action_at, result, notes

**Features:**
- **Sandbox preview:** test configuration changes against representative records before publish
- **Validation engine:** checks referential integrity, protected-field rules, workflow semantic coverage, dependency graph consistency, and migration completeness
- **Impact analysis:** shows how many records, queues, reports, templates, and dashboards are affected before publishing
- **Safe rollback:** rollback is performed by publishing the previous valid version, never by deleting history
- **Config diff viewer:** compare draft vs active versions for workflows, forms, terminology, levels, layouts, and sequences
- **Deactivation over deletion:** configuration objects already referenced by historical data are retired, not hard-deleted

---

**Configuration Engine Permissions:**

| Permission | Access Granted |
|---|---|
| `cfg.workflow` | Edit workflow state machines |
| `cfg.levels` | Configure priority/risk/urgency level sets |
| `cfg.fields` | Create/edit custom field definitions, form rules, and templates |
| `cfg.terminology` | Edit terminology overrides |
| `cfg.layout` | Configure role/user UI layouts and push templates |
| `cfg.modules` | Enable/disable modules |
| `cfg.sequences` | Configure reference number sequences |
| `cfg.publish` | Validate, publish, and roll back configuration change sets |
| `cfg.export` | Export the full configuration profile as JSON |
| `cfg.import` | Import a configuration profile (configuration migration) |

**Configuration Profile Export/Import:**

The entire tenant configuration (structure models, workflows, level sets, form rules, custom fields, terminology overrides, role layouts, module states, sequences, and published configuration versions) can be exported as a single signed JSON archive - the "Configuration Profile." This enables:
- **Multi-site replication:** export from Site A, import on Site B to ensure consistent process definitions and operating-model rules
- **Onboarding acceleration:** Maintafox can ship pre-built configuration profiles for specific industry verticals (oil & gas, cement plant, food & beverage, pharmaceutical)
- **Backup & disaster recovery:** configuration profile is included in the full data export and can be restored independently from transactional data records
- **Controlled migration:** import validates all references, version dependencies, and conflicts before applying changes, and produces a migration report instead of partially applying invalid configuration

---

## 7. DATABASE ARCHITECTURE

### 7.1 Local SQLite Schema Principles

1. Synchronized business tables use a local `INTEGER PRIMARY KEY` for fast joins plus a stable `sync_id UUID` for cross-machine identity.
2. Local referential integrity is enabled for ordinary business data; sync flexibility is achieved through staging, tombstones, and review workflows rather than by disabling foreign keys globally.
3. Mutable records carry `created_at`, `updated_at`, and where relevant `deleted_at`, plus concurrency or provenance fields such as `row_version`, `origin_machine_id`, and `last_synced_checkpoint`.
4. Analytical snapshots, cached computations, and mirror-control metadata are separated from operational source tables.
5. Configuration and workflow semantics are version-aware so historical records remain interpretable after later changes.

### 7.2 Sync Control Tables

Representative sync-control structures include:

```sql
CREATE TABLE sync_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_sync_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    row_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    dispatched_at TEXT,
    acknowledged_at TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE TABLE sync_checkpoint (
    stream_name TEXT PRIMARY KEY,
    last_server_checkpoint TEXT,
    last_success_at TEXT,
    last_machine_id TEXT
);
```

These structures are coordination metadata, not substitutes for the operational audit trail.

### 7.3 Conflict Governance

| Data Class | Default Resolution Model |
|---|---|
| Append-only evidence and event logs | Preserve all valid events; never collapse by last-write-wins |
| Governed reference and configuration versions | Server-published version authority with explicit local adoption |
| Mutable operational records | Field-aware merge, record lock, or review queue depending on module criticality |
| Deletes and retirements | Tombstone or retirement model with review-aware application |

Conflict handling is class-aware because maintenance workflows do not treat all data as equally mergeable.

### 7.4 VPS PostgreSQL Mirror Model

The VPS stores:

- shared control-plane metadata for entitlements, rollout cohorts, admin users, and platform telemetry
- one tenant mirror schema per customer for synchronized business data
- sync checkpoints, idempotency records, replay protection, and mirror-side audit support

The VPS mirror coordinates multi-machine use and fleet administration, but the desktop database remains the operational source of truth for local runtime behavior.

---

## 8. SYNC LAYER - HYBRID CLOUD

### 8.1 Sync Modes

| Mode | Trigger | Purpose |
|---|---|---|
| **Opportunistic Background Sync** | Connectivity detected and policy allows | Silent outbox delivery and inbound refresh |
| **Manual Sync** | User action | Visible progress, diagnostics, and operator confidence |
| **Bootstrap Restore** | First trusted install or replacement machine | Full tenant restore onto a new trusted device |
| **Recovery Replay** | Support, repair, or conflict-resolution action | Controlled reapplication of inbound or outbound batches |
| **Heartbeat Refresh** | Scheduled policy interval | Entitlement, update-channel, and policy-state refresh |

### 8.2 Replay-Safe Delta Sync Contract

Representative payload shape:

```json
{
  "tenant_id": "<uuid>",
  "machine_id": "<trusted_machine_id>",
  "checkpoint_token": "<last_acked_server_checkpoint>",
  "idempotency_key": "<uuid>",
  "outbox_batch": [
    {
      "entity_type": "work_orders",
      "entity_sync_id": "<uuid>",
      "operation": "update",
      "row_version": 42,
      "payload": {}
    }
  ]
}
```

Server response includes:

- accepted items
- rejected items with reason
- `acknowledged_items`
- `inbound_batch`
- next `checkpoint_token`
- policy or update metadata where relevant

### 8.3 Heartbeat And Policy Refresh

Heartbeat is not only a yes-or-no license ping. It refreshes:

- entitlement status
- trusted-device policy snapshot
- offline grace and suspension behavior
- rollout channel and update availability
- support or security notices that affect local posture

### 8.4 Restore, Replay, And Recovery

The sync layer must support:

- trusted-device bootstrap from a full tenant restore
- replay-safe retry of failed batches
- controlled conflict review queues for records that cannot be merged automatically
- repair flows that do not require destructive database resets as the normal support answer

---

## 9. RELIABILITY ENGINEERING COMPUTATION ENGINE

### 9.1 Execution Model

The reliability engine is a pure Rust computation workspace isolated from the UI layer and from direct mutation of operational source records.

Core rules:

- operational source data is read, normalized, and hashed into a reproducible analysis input set
- computations run in bounded background tasks or synchronous micro-calculations depending on complexity
- results are stored as analysis snapshots and do not overwrite the original evidence base
- advanced methods are gated by data quality, entitlement, and maturity, not only by code availability

### 9.2 Typed Analysis Contracts

Each analysis run records at least:

- analysis type and algorithm version
- dataset hash and source filters
- execution timestamp and operator context when applicable
- summary metrics, warnings, and plot payloads
- interpretation notes or assumptions where the method requires them

This preserves reproducibility for Weibull, FMECA, FTA, RBD, ETA, Monte Carlo, Markov, and related staged methods.

### 9.3 Execution Modes And Stage Gates

- **Interactive calculations:** fast KPI and summary metrics inside normal UI latency budgets
- **Background studies:** heavier reliability or stochastic calculations with progress reporting and cancellation support
- **Governed availability:** advanced methods stay tier- and maturity-gated so Maintafox does not overstate certainty on weak evidence
- **Snapshot persistence:** outputs remain traceable to the input set used at execution time

---

## 10. LICENSING & SUBSCRIPTION CONTROL SYSTEM

### 10.1 Entitlement Envelope Model

Maintafox licenses are signed entitlement envelopes issued only by the VPS control plane. They define what the deployment is allowed to do; they do not replace local user authentication.

Representative claim set:

```json
{
  "sub": "<tenant_uuid>",
  "jti": "<entitlement_uuid>",
  "tier": "professional",
  "machines": 3,
  "features": ["eq", "di", "ot", "pm", "rep"],
  "offline_policy": { "grace_days": 7 },
  "update_channel": "stable",
  "not_before": 1711929600,
  "expires_at": 1727740800,
  "iss": "maintafox-vps"
}
```

### 10.2 Machine Binding

Machine binding uses a privacy-preserving fingerprint derived from an installation-local secret plus a resilient set of hardware or OS anchors.

| Anchor | Purpose |
|---|---|
| Install-local secret | Primary anti-copy binding factor stored in OS secure storage |
| OS platform anchor | Stable machine identity where the operating system exposes one |
| CPU model hash | Resilience factor, not standalone proof |
| Primary storage anchor | Hardware stability factor with graceful fallback |
| Network or baseboard anchor | Additional tolerance factor where available |

A policy-defined threshold determines whether the device is still considered the same trusted machine after ordinary hardware maintenance.

### 10.3 Feature Tiering Principles

| Capability Group | Starter | Professional | Enterprise |
|---|---|---|---|
| Core local operations | Enabled | Enabled | Enabled |
| Planning, compliance, and cost control | Limited | Enabled | Enabled |
| Advanced analytics and reporting | Basic | Extended | Full |
| Advanced RAMS methods | Disabled | Limited or staged | Full staged suite |
| ERP, IoT, and external coordination | Disabled | Optional | Enabled |
| Multi-machine coordination | Minimal | Moderate | Full |
| Advanced configuration and vendor-control features | Limited | Enabled | Full |

Exact entitlements are delivered as feature flags, but they follow these commercial bands.

### 10.4 Entitlement States And Enforcement

| State | Local Behavior |
|---|---|
| **active** | Operates normally within policy |
| **grace** | Existing trusted devices continue operating inside granted grace window |
| **expired** | New writes and new activations are blocked after grace; governed read access may remain |
| **suspended** | Administrative restriction applies; sync or writes may be selectively blocked |
| **revoked** | Entitlement is invalid and trust is withdrawn on next policy contact |

Security-driven emergency lock remains distinct from ordinary commercial revocation.

---

## 11. AUTOMATIC UPDATE SYSTEM

### 11.1 Release Channels And Update Flow

Maintafox uses the Tauri updater model with signed artifacts. Signature verification is mandatory and not disabled in production.

Supported rollout channels:

- `stable` for general production use
- `pilot` for controlled customer validation
- `internal` for vendor-operated pre-release validation

Typical flow:

1. Heartbeat or manual check detects an eligible update for the current channel.
2. The client requests the updater manifest from the VPS.
3. Version metadata and bundle signature are verified against the configured updater public key.
4. The user is shown release notes, compatibility notes, and timing options.
5. The bundle is downloaded, verified, and installed using platform-appropriate behavior.
6. On relaunch, migration checks run before normal UI loading.

### 11.2 Manifest And Artifact Contract

The updater service provides at least:

- `version`
- `pub_date`
- `notes`
- `url`
- `signature`

The updater-signing key is separate from licensing and session-related keys.

### 11.3 Rollout, Recall, And Rollback

- staged rollout can be controlled by tenant cohort, support cohort, or channel
- recalled builds stop being offered immediately
- controlled downgrade is allowed only through explicit server policy or support procedure
- bad releases must be diagnosable through rollback-safe support workflows

### 11.4 Migration Safety

- migrations are idempotent and tracked in `schema_versions`
- a pre-migration safety checkpoint is taken before destructive classes of change
- startup refuses normal UI entry until required migrations succeed or a safe recovery path is selected
- migration failures produce support-grade diagnostics instead of silently corrupting business data

---

## 12. SECURITY ARCHITECTURE

### 12.1 Data At Rest

- local SQLite data is protected with SQLCipher where enabled, using an installation master secret stored in OS-managed secure storage
- purpose-specific subkeys are derived for database encryption, local session signing, and other local cryptographic scopes
- passwords for local accounts are hashed with Argon2id using calibrated parameters
- refresh tokens, trusted-device secrets, and other small secrets live in OS keychain facilities rather than in plaintext database fields

### 12.2 Data In Transit

- all VPS traffic uses TLS 1.3 with certificate validation and modern cipher requirements
- certificate pinning uses SPKI continuity in the Rust HTTP client so ordinary certificate renewal does not break trust unexpectedly
- non-HTTPS updater transport is disabled in production
- request signing or provenance controls can be applied to sensitive sync or relay operations where policy requires it

### 12.3 Authentication And Session Security

- local session tokens use HS256 with a machine-local secret stored in OS secure storage
- entitlement verification uses a VPS-issued public-key trust chain distinct from local session signing
- offline access is restricted to previously trusted users on previously trusted devices inside policy-defined grace windows
- rate limiting, lockout, idle lock, and step-up reauthentication are enforced locally from the current policy snapshot

### 12.4 Tauri Trust Boundary And IPC Security

- the WebView and Rust core are separate trust domains
- only explicitly allowed IPC commands and plugin capabilities cross that boundary
- no local web server is exposed as the normal application command surface
- frontend code has no implicit access to shell, filesystem, or process APIs
- CSP remains strict and user-supplied rich content is sanitized before rendering

### 12.5 Update And Supply-Chain Security

- update artifacts are signed and verified before installation
- signing keys for updater artifacts are separated from entitlement or session key material
- dependency intake, build signing, and release promotion follow controlled vendor-side procedures
- VPS admin access to `console.maintafox.systems` requires strong session controls and MFA

---

## 13. UI/UX DESIGN GUIDELINES

### 13.1 Visual Identity And Product Feel

The desktop application preserves the Maintafox brand system, but it must feel like a native industrial operations workspace rather than a packaged website.

Core UI principles:

- state clarity before decoration
- blocker visibility before optimistic action
- evidence drill-through before summary-only dashboards
- offline and sync status always visible
- dangerous actions clearly differentiated from ordinary edits

**Primary Brand Colors:**
| Token | Value | Usage |
|---|---|---|
| `--color-primary` | `#003d8f` | Primary actions, active navigation, key headers |
| `--color-primary-dark` | `#002b6a` | Hover, pressed, and selected-emphasis states |
| `--color-primary-light` | `#4d7bc5` | Informational accents and chart support tones |
| `--color-primary-bg` | `#e8eef8` | Selected rows, soft highlight backgrounds |
| `--color-secondary` | `#f0a500` | Accent actions, warnings, and time-sensitive prompts |

**Semantic tokens for status colors:** success (green `#198754`), danger (red `#dc3545`), warning (yellow `#ffc107`), info (teal `#0dcaf0`), neutral (gray).

**Typography:** Inter as the primary UI family with system fallbacks. No web fonts are loaded from public CDNs.

**Logo Assets:**

The Maintafox logo is a flame-and-gear mark representing maintenance (gear) and proactive intelligence (flame). Two variants exist for all rendering contexts:

| Variant | File | Usage |
|---|---|---|
| Color (SVG) | `src/assets/logo/maintafox-logo-color.svg` | Light backgrounds: splash screens, about dialogs, login, headers, export watermarks |
| Color (PNG) | `src/assets/logo/maintafox-logo-color.png` | Fallback where SVG is unsupported (e.g. native OS dialogs) |
| White (SVG) | `src/assets/logo/maintafox-logo-white.svg` | Dark backgrounds: dark-mode sidebars, login overlay, PDF dark headers |
| White (PNG) | `src/assets/logo/maintafox-logo-white.png` | Fallback for dark-background contexts without SVG support |

Logo color values: flame gradient from `#F05A28` (base) to `#F7941D` (tip), gear `#1A4FA0` (dark blue). The white variant uses `#FFFFFF` for both elements. Desktop app icons (tray, taskbar, installer) are generated from the color PNG at build time via `tauri icon`. Components import logos from `@/assets/logo` — use `logoColor` / `logoWhite` for SVG, `logoColorPng` / `logoWhitePng` for PNG fallbacks.

### 13.2 Desktop Workspace Model

Default workspace rules:

- a persistent top bar shows search or command access, sync state, notification state, and user menu
- a role-scoped navigation sidebar remains stable across modules and reflects module visibility from 6.26
- the main content area uses a module header, action row, filter row, and one primary working surface at a time
- details, histories, and related records prefer side sheets or split panes over route thrashing
- a persistent status bar shows offline or online state, pending sync, database health, and application version
- multi-step workflows keep context visible so planners, reviewers, and supervisors can understand blockers without modal churn

### 13.3 Component And Workflow Conventions

- dense, filterable list views are the default operational surface
- cards and Kanban boards are supplementary views rather than the only interaction model
- forms enforce stage-specific requirements inline at the moment of transition
- dialogs confirm destructive or high-risk actions; sheets and split panes handle inspection and review detail
- state badges and severity markers use consistent semantic colors and iconography across modules
- publish, revoke, restore, and similar actions always require consequence visibility and reason capture where policy requires it

### 13.4 Analytical And D3 Surfaces

- chart logic lives in isolated hooks or service adapters, not inline inside large page components
- every analytical surface shows filters, time window, and scope
- charts use shared tokens and semantic colors instead of ad hoc palettes
- export is available where the underlying data is allowed to be exported
- enterprise-only analytical surfaces appear only when entitlement and product maturity allow them

### 13.5 Accessibility And Industrial Usability

- full keyboard navigation is required for standard workflows
- icon-only actions expose labels or tooltips
- contrast meets WCAG AA minimums
- compact and high-visibility modes remain usable on workshop PCs, kiosks, and large desktop monitors
- touch-friendly targets are required where the product is used in inspection or permit execution contexts

---

## 14. NON-FUNCTIONAL REQUIREMENTS

### 14.1 Performance

| Operation | Target |
|---|---|
| Cold start on reference hardware | < 4 seconds |
| Warm module navigation | P95 < 150ms |
| Standard list-view query and filter | P95 < 300ms |
| Large D3 operational chart redraw | < 500ms |
| Standard PDF or Excel export | < 10 seconds |
| Background sync of 1,000 ordinary changes | < 30 seconds on healthy network |
| Core reliability KPI calculation | < 1 second |
| Long-running advanced analysis | Background execution with responsive UI |

### 14.2 Reliability, Resilience, And Recoverability

- core execution workflows remain available offline on already trusted devices within allowed policy
- local durability uses transactional SQLite in WAL mode with startup integrity checks
- VPS outage must not invalidate ordinary local work
- update or migration failure must leave a recoverable safe state
- restore testing is required for both local and control-plane backups

### 14.3 Scalability

- a single tenant installation should handle high-volume industrial records with proper indexing, archival, and projection strategy
- enterprise deployments must support multiple trusted machines per tenant coordinating through the VPS mirror
- scale targets are validated on representative datasets, not only empty-schema benchmarks

### 14.4 Observability And Supportability

- structured logs, sync traces, and migration reports are collectable without exposing secrets in plaintext
- the product surfaces queue backlog, update state, sync state, and entitlement state visibly
- support workflows can inspect version, platform, and recent failure context without direct database intervention

### 14.5 Supported Platforms

| Platform | Minimum Version |
|---|---|
| Windows | Windows 10 (1903+) |
| Linux | Ubuntu 20.04+ / RHEL 8+ |
| macOS | macOS 12 (Monterey)+ |

### 14.6 Accessibility

- all interactive elements are keyboard-navigable
- form fields have associated labels and clear error feedback
- icon-only controls provide accessible names
- critical state changes are communicated visually and textually

### 14.7 Internationalization (i18n)

- Maintafox must be built multilingual from the outset; internationalization architecture is part of the foundation scope, not deferred localization work.
- **Launch baseline:** French is the primary language for the first production release, and English is supported from the same initial release through the same structured locale framework.
- **Planned expansion:** additional French variants, Arabic with full RTL support, and other future locales may be added through the same locale model once layout, chart, and workflow surfaces are validated.
- all user-facing strings, state labels, validation messages, notification templates, report labels, export labels, and release-note content must be externalizable
- locale resources must be namespaced, versioned, reviewable, professionally managed, and protected by fallback and missing-key detection controls
- formatting for dates, numbers, currencies, search behavior, sorting, and exports must be locale-aware from the beginning

---

## 15. DELIVERY PHASES & MILESTONES

Delivery is organized as gated release trains. A phase is complete only when its platform, data, migration, security, and support exit criteria are satisfied.

| Phase | Objective | Primary Scope | Required Exit Criteria |
|---|---|---|---|
| **Phase 1: Secure Foundation** | Establish the desktop runtime, local data plane, identity model, control-plane skeleton, and multilingual foundation | Tauri shell, local DB, auth, settings baseline, locale-resource architecture, updater skeleton, core architecture | Signed builds working, local DB migrations stable, trusted-device auth functional, French-and-English locale infrastructure operational on foundation surfaces, and backup or restore preflight validated |
| **Phase 2: Core Execution Backbone** | Deliver the operational maintenance workflow core | org model, asset backbone, DI, WO, archive, reference domains, notifications, basic audit | Stage-gated workflows working end-to-end, closure-quality enforced, audit and activity capture stable |
| **Phase 3: Planning, Compliance, And Material Control** | Deliver readiness-aware planning and operational control layers | PM, planning, inventory, permits, training, inspections, budget control, self-service and personnel readiness | Blocker logic, reservations, qualification gates, permit enforcement, and planned-work commitment model validated in pilot workflows |
| **Phase 4: Control Plane And Integrations** | Deliver cross-machine coordination and enterprise interfaces | sync, licensing, update rollout, ERP, IoT, vendor admin console, relay services | Idempotent sync proven, entitlement behavior verified, signed updates end-to-end, `console.maintafox.systems` operational |
| **Phase 5: Advanced Reliability And Launch Hardening** | Expand advanced analytics and prepare production launch | staged RAMS methods, performance tuning, localization completion, security review, pilot rollout | Reliability outputs validated on representative data, security review complete, pilot customers signed off, launch checklist closed |

Release-gating rules:

- advanced reliability methods are not general-availability scope until phase-1 and phase-2 data quality is sufficient
- no feature wave progresses without migration safety, backup or restore, and audit-visibility criteria for that wave
- enterprise integrations do not ship without operational monitoring and replay-safe recovery tooling
- pilot customers are introduced before public launch for workflow validation, not only UI feedback

---

## 16. VPS INFRASTRUCTURE SPECIFICATION

### 16.1 Deployment Topology And Baseline Sizing

The VPS is the control plane and tenant mirror service, not the primary runtime for daily work. Baseline sizing should therefore be planned around synchronization, entitlement checks, update delivery, admin operations, and mirror retention.

| Profile | CPU | RAM | Storage | Intended Use |
|---|---|---|---|---|
| **Pilot** | 4 vCPU | 8 GB | 100 GB SSD | Early pilot or low-tenant rollout |
| **Shared Production** | 8 vCPU | 16 GB | 250 GB SSD | Multi-tenant production baseline |
| **Growth Production** | 16+ vCPU | 32+ GB | 500+ GB SSD | Larger tenant count, heavier sync or update volume |

Ubuntu LTS remains the baseline operating system. All public traffic is limited to HTTPS, with SSH access restricted operationally.

### 16.2 Core Services Deployed

| Service | Role |
|---|---|
| `nginx` | Reverse proxy, TLS termination, routing for API and admin console |
| `api` | Fastify application for license, sync, update, and admin endpoints |
| `worker` | Background jobs for update rollout, email relay, report scheduling, and housekeeping |
| `postgres` | Shared control-plane metadata plus tenant mirror schemas |
| `redis` | Coordination cache, rate limiting, and short-lived queue state |
| `minio` or external S3 | Update bundles, backups, and permitted mirror object offload |
| `admin-ui` | Vendor-operated web console for `console.maintafox.systems` |
| `backup` | Scheduled PostgreSQL and object-storage backup routines |
| `metrics` / `logs` | Observability pipeline for health, alerts, and support diagnostics |

### 16.3 VPS API Families

| Family | Representative Endpoints | Purpose |
|---|---|---|
| **License & Activation** | `/api/license/heartbeat`, `/api/license/activate`, `/api/license/deactivate` | Entitlement refresh, machine-slot control, policy delivery |
| **Sync** | `/api/sync/push`, `/api/sync/pull`, `/api/sync/full` | Outbox ingestion, inbound batch delivery, bootstrap restore |
| **Updates** | `/api/updates/manifest`, `/api/updates/download/:bundle_id` | Channel-aware update discovery and artifact delivery |
| **Admin** | `/admin/customers`, `/admin/licenses`, `/admin/metrics` | Vendor-operated management and reporting |
| **Relay / Enterprise Support** | `/api/iot/readings/batch`, `/api/erp/sync/trigger`, `/api/notifications/email`, `/api/reports/schedule` | Cross-machine relay, enterprise integration coordination, optional delivery services |

### 16.4 Multi-Tenancy And Data Isolation

The VPS uses a shared control-plane schema plus one PostgreSQL schema per tenant for mirrored business data.

```sql
CREATE SCHEMA tenant_<customer_uuid>;

SET search_path TO tenant_<customer_uuid>;
CREATE TABLE equipment (...);
CREATE TABLE work_orders (...);
```

Isolation rules:

- tenant business data never shares tables with another tenant's business data
- shared control-plane tables store only vendor-side metadata such as entitlements, rollout state, admin users, and platform metrics
- backups, restore operations, and offboarding can be executed at the tenant boundary without cross-tenant spillover

### 16.5 Admin Dashboard - `console.maintafox.systems`

`console.maintafox.systems` is the vendor-operated operations console for entitlement control, sync visibility, update rollout, and platform administration. It is not bundled into the desktop app.

Technical characteristics:

- **Frontend:** React 18 + TypeScript + Tailwind CSS + Shadcn/ui
- **Backend:** Fastify admin routes separated from tenant runtime auth
- **Auth:** short-lived admin sessions, refresh cookie, mandatory TOTP, and optional IP allowlist or VPN-only access
- **Exposure:** dedicated Nginx virtual host with TLS and strict administrative access policy

Primary dashboard domains:

- **Customer management:** account lifecycle, tenant metadata, rollout cohort, and support posture
- **Entitlement management:** tier, feature flags, machine slots, expiry, suspension, revocation, and offline-policy control
- **Machine activation monitor:** active devices, heartbeat freshness, version skew, and remote slot release
- **Sync monitor:** queue lag, recent failures, replay or repair actions, and tenant-level synchronization health
- **Update distribution control:** staged rollout, recall, release notes, and channel management
- **Platform health:** service health, storage pressure, DB metrics, Redis pressure, and alert history
- **Admin audit trail:** append-only record of all vendor-side administrative actions

### 16.6 Backup, Observability, And Disaster Recovery

- PostgreSQL backups run on a schedule with retention policy and integrity verification
- object storage for bundles and allowed mirror artifacts is backed up or replicated according to environment tier
- restore tests are required periodically for both tenant mirror data and control-plane metadata
- platform observability includes structured logs, service metrics, queue depth, error-rate alarms, and tenant-specific sync-lag indicators
- secret rotation, certificate renewal, and signing-key management are treated as operational runbooks rather than ad hoc administrator knowledge

---


## 17. APPENDICES

### Appendix A - Web Prototype to Desktop Module Mapping

All pages implemented in the web prototype are accounted for and elevated into fully-specified desktop modules. Pages that were placeholders in the web app now have complete specifications.

| Web App Page / Module | Desktop Module | Conversion Notes |
|---|---|---|
| `Tableau-de-Bord` | 6.11 Analytics & Dashboard | D3 charts ported; added MTBF/PM compliance KPIs, SLA breach rate, certification expiry counter |
| `Équipements` | 6.3 Equipment Asset Registry | Same 4-level hierarchy; added IoT sensor link, ABC criticality, action plan panel, TCO, ERP import |
| `Demandes-d-intervention` | 6.4 Intervention Requests (DI) | Same 11-state workflow; added SLA tracking, security risk flag, 4th "Dashboard" view, file attachments, IoT-auto origin |
| `Ordres-de-travail` | 6.5 Work Orders (OT) | Same 8-state workflow; added Gantt timeline, task checklists, backlog heatmap, closed-WO analysis |
| `Gestion-du-personnel` | 6.6 Personnel Management | Same model; added certification overlay, succession planning, HRMS sync, staff gap alerts |
| `Gestion-utilisateurs-Roles` | 6.7 Users, Roles & Permissions | Same permission domain model; added online presence, session audit, password policy enforcement |
| `Stock-Pieces-de-rechange` | 6.8 Inventory & Spare Parts | Fully preserved; added repairable parts, HSE controls, FIFO/JIT analysis, 30-day forecast, open PO risk tracker |
| `Maintenance-preventive` | 6.9 Preventive Maintenance | Expanded with condition-based IoT triggers, AI/rule-based optimizer, team capacity integration |
| `RAMS-page` | 6.10 RAMS / Reliability Engine | Full Rust computation engine; added ETA (6.10.8), Bow-Tie/LOPA (6.10.9), Markov (6.10.10), DataBridge (6.10.11), CriticalFox (6.10.12) |
| `Rapports-Analyses` | 6.11 Analytics & Dashboard | Expanded with report scheduling, Analytical Alert Engine, energy/safety axes, Power BI integration |
| `Configuration-page` | 6.13 Lookup Manager + 6.18 Settings | Lookup types fully preserved; appearance/localization/notifications/integrations moved to dedicated Settings module (6.18) |
| `Archive-Explorer` (in Configuration) | 6.12 Archive Explorer | Same behavior; added bulk operations, extended coverage to archived DIs |
| `Organigramme` | 6.2 Organization & Site Management | D3 chart preserved; added ERP external ID field, entity labels/tags |
| `Documentation-Support` | 6.15 In-App Documentation & Support Center | Embedded and offline-capable; added habilitation gates, LOTO/ATEX templates, support tickets, download audit |
| `Activité-Récente` | 6.17 Activity Feed & Audit Log | *(was a standalone web page, now a full module)* Real-time feed, domain summaries, immutable audit, 2-year retention |
| `Planification-Calendrier` | 6.16 Planning & Scheduling Engine | *(was a standalone web page, now a full module)* Consolidated calendar, Gantt, capacity planning, conflict detection |
| `profil.html` | 6.19 User Profile & Self-Service | *(was a standalone web page, now a full module)* Profile card, password change, habilitation status, notification preferences |
| `login.html` | 6.1 Authentication & Session Mgmt | Same login screen; upgraded to Argon2id hashing, OS keychain storage, inactivity lock |
| *(not in web)* | 6.20 Training & Certification | New module; critical for ATEX/electrical compliance; referenced by Personnel page stats |
| *(not in web)* | 6.21 IoT Integration Gateway | New module; referenced from Equipment (sensor tiles) and PM (condition-based triggers) |
| *(not in web)* | 6.22 ERP & External Systems | New module; referenced from Equipment (ERP import), Configuration page (ERP connector placeholder) |
| *(not in web)* | 6.23 Work Permit (LOTO/PTW) | New module; referenced in Documentation LOTO procedures and Work Order status gating |
| *(not in web)* | 6.24 Budget & Cost Center | New module; cost data flows from Work Orders and Purchase Orders |
| *(not in web)* | 6.25 Inspection Rounds | New module; extends the recurring checklist concept shown in PM preventive page |
| *(not in web)* | 6.26 Configuration Engine & Tenant Customization | New module; no web prototype equivalent; provides the full runtime configurability layer (workflows, custom fields, terminology, UI layout, module enable/disable, sequence config) |

---

### Appendix B - Key Standards Referenced

| Standard | Domain | Application in Maintafox |
|---|---|---|
| ISO 55000 / 55001 / 55002 | Asset Management | Overall asset lifecycle philosophy, asset register |
| EN 13306:2017 | Maintenance Terminology | French-language maintenance term definitions throughout UI |
| ISO 14224:2016 | Reliability Data Collection | Failure event recording schema, MTBF/MTTR calculation formulae |
| IEC 60812:2018 | FMEA/FMECA | FMECA worksheet structure, RPN = Severity x Occurrence x Detectability |
| IEC 61025:2006 | Fault Tree Analysis | FTA gate definitions, Boolean probability propagation, MCS algorithm |
| IEC 62681:2022 | Event Tree Analysis | FoxChain ETA forward probability propagation methodology |
| IEC 61078:2016 | Reliability Block Diagrams | RBD series/parallel/mixed computation, system reliability |
| MIL-HDBK-338B | Electronic Reliability Design | Weibull analysis methodology, beta/eta MLE, bathtub curve |
| IEC 61882:2016 | HAZOP | Referenced in Bow-Tie LOPA threat identification guidance |
| IEC 61511-1:2016 | Safety Instrumented Systems | SIL determination in LOPA analysis (FoxRisk), PFD calculations |
| IEC 60300-3-11 | Reliability-Centered Maintenance | RCM decision logic tree structure, maintenance task selection |
| IEC 60300-3-5 | Reliability Engineering Review | RAMS methodology and documentation structure |
| ISO 45001:2018 | Occupational H&S Management | Work Permit System (PTW/LOTO) design and workflow |
| EN 60079-17 | ATEX Inspection & Maintenance | Training/habilitation for ATEX zones, Documentation ATEX template |
| ISO 13379-1:2012 | Machine Condition Monitoring | IoT Gateway sensor type classification, condition-based maintenance |
| ISO 50001:2018 | Energy Management | Energy consumption axis in Analytics, energy reporting |
| OWASP Top 10 (2024) | Application Security | All local and VPS security controls verified against this checklist |
| OData v4 (OASIS) | ERP Integration Protocol | Standard REST query protocol used by Microsoft Dynamics 365 F&O, SAP S/4HANA OData services, and Oracle ORDS for the ERP connector in module 6.22 |
| SAP BAPI / RFC | ERP Integration Protocol | SAP function module interface used for SAP ECC integration; accessed via SAP JCo REST gateway in module 6.22 |
| IFS REST API (IFS AB) | ERP Integration Protocol | IFS Applications 9/10 bidirectional REST interface used by the IFS connector in module 6.22 |
| ISO 15926-2 / CFIHOS | Asset Information Interchange | Equipment tag structure and property-set taxonomy for oil & gas / capital facilities handover packages; used by equipment-master import flows in module 6.22 |
| RFC 7517 / RFC 7519 | JSON Web Key / JWT | SPKI public key pinning in JWK format for license JWT verification; HS256 and RS256 token structures per RFC 7519 |
| NIST SP 800-132 | Password-Based Key Derivation | HKDF-SHA256 key derivation for SQLCipher database encryption key from machine fingerprint + license key segment |

---

### Appendix C - Glossary

| Term | Acronym | Definition |
|---|---|---|
| Demande d'Intervention | DI | Intervention Request - formal report of an equipment fault or maintenance need |
| Ordre de Travail | OT | Work Order - authorized maintenance task with assigned resources and schedule |
| Gestion de Maintenance Assistée par Ordinateur | GMAO | French acronym for CMMS (Computerized Maintenance Management System) |
| Mean Time Between Failures | MTBF | Average operating duration between consecutive failures of a repairable system |
| Mean Time To Repair | MTTR | Average time elapsed from failure detection to system restoration |
| Mean Time To Failure | MTTF | Expected operating time before first failure of a non-repairable component |
| Availability | A | Proportion of time an asset is in an operable state: MTBF / (MTBF + MTTR) |
| Operational Availability | Ao | Availability accounting for logistics and administrative delay: Uptime / (Uptime + All Downtime) |
| Failure Mode Effects Criticality Analysis | FMECA | Systematic method to identify failure modes, their effects, and criticality using RPN scoring |
| Risk Priority Number | RPN | FMECA risk score: Severity x Occurrence x Detectability (range 1-1000) |
| Fault Tree Analysis | FTA | Top-down deductive failure analysis using Boolean gates to trace top-level events to root causes |
| Event Tree Analysis | ETA | Inductive failure analysis modelling the spectrum of consequences from an initiating event through safety barriers |
| Bow-Tie Analysis | - | Risk analysis diagram combining FTA (prevention side) and ETA (mitigation side) around a top event |
| Layer of Protection Analysis | LOPA | Semi-quantitative method to assess the risk reduction provided by independent protection layers |
| Safety Integrity Level | SIL | 4-level classification (SIL 1-4) of required safety function performance per IEC 61511 |
| Probability of Failure on Demand | PFD | Probability that a safety system fails to perform its function when demanded |
| Reliability Block Diagram | RBD | Block diagram showing functional interdependencies of components for system reliability calculation |
| Reliability Centered Maintenance | RCM | Structured methodology to determine the most appropriate maintenance strategy per failure mode |
| Reliability, Availability, Maintainability, Safety | RAMS | The four quantitative properties of system dependability |
| Maximum Likelihood Estimation | MLE | Statistical algorithm for estimating distribution parameters (beta, eta) from failure data |
| Minimal Cut Set | MCS | Smallest set of basic events whose simultaneous failure causes the FTA top event |
| Markov Analysis | - | State-based reliability modeling using transition rate matrices; used for complex multi-state systems |
| Habilitation | - | French regulatory certification authorizing an individual to perform specific work types (electrical, ATEX, height, etc.) |
| Lockout/Tagout | LOTO | Energy isolation procedure to prevent the release of hazardous energy during maintenance |
| Permit-to-Work | PTW | Formal written authorization system for high-risk maintenance activities requiring controlled conditions |
| Gestion Électronique de Documents | GED | Electronic Document Management System (SharePoint, Alfresco, etc.) |
| Système d'Information Ressources Humaines | HRMS | Human Resource Management System (SAP HR, Workday, etc.) |
| Inspection Round | - | A structured recurring walk-through of a set of equipment items with defined checkpoints |
| DataBridge | - | Maintafox module bridging operational GMAO data (WOs, PMs) to RAMS computation inputs |
| CriticalFox | - | Maintafox RAMS reporting dashboard for fleet-level reliability and criticality KPIs |
| Cost Center | - | Organizational accounting unit to which maintenance expenditures are allocated |
| Total Cost of Ownership | TCO | Complete lifetime cost of an asset: purchase price + all maintenance costs from commissioning to decommission |
| Service Level Agreement | SLA | Defined maximum response/resolution time for a DI based on priority; breach triggers escalation |
| ABC Analysis | - | Classification of assets or spare parts into A (critical/high-value), B (essential/medium), C (secondary/low) categories |
| OPC-UA | OPCUA | Open Platform Communications Unified Architecture - industrial IoT communication standard (ISO/IEC 62541) |
| FoxChain | - | Maintafox module name for Event Tree Analysis |
| FoxRisk | - | Maintafox module name for Bow-Tie / LOPA analysis |
| FoxFlow | - | Maintafox module name for Markov Chain analysis |
| FoxSim | - | Maintafox module name for standalone Monte Carlo simulation |
| FoxRBD | MaintaRBD | Maintafox module name for Reliability Block Diagram analysis |
| sea-orm | - | Async ORM crate for Rust built on top of sqlx; provides entity definitions, compile-time query builder, and schema migration management for the local SQLite database |
| sqlx | - | Async SQL toolkit for Rust; used directly for complex queries and internally by sea-orm; compile-time SQL verification |
| SPKI Pin | SPKI | Subject Public Key Info hash - a TLS certificate pinning method that pins the public key itself rather than the certificate; survives certificate renewal as long as the key pair is unchanged |
| HKDF | - | HMAC-based Key Derivation Function (RFC 5869); used to derive the SQLCipher encryption key from the machine fingerprint and license key segment |
| Workflow State Machine | - | A configuration model defining the valid states of a record (DI, OT, PTW) and the permitted transitions between them; stored in `workflow_definitions`, `workflow_states`, `workflow_transitions` tables |
| Custom Field | - | Administrator-defined data field added to an entity type (Equipment, DI, OT, etc.) at runtime without code change; up to 20 custom fields per entity type |
| UI Layout Engine | - | The sub-system of module 6.26 that manages role-level module visibility and per-user dashboard widget layout, stored in `ui_role_layouts` and `ui_user_layouts` tables |
| Terminology Override | - | A customer-specific substitution for a UI label string, resolved at i18n layer without modifying compiled code; stored in `term_overrides` table |
| Tenant Configuration | - | The complete set of runtime configuration for a customer deployment: workflows, level sets, custom fields, terminology overrides, role layouts, module states, and sequence configurations; exportable as a signed JSON archive |
| Configuration Profile | - | Signed JSON archive of the full Tenant Configuration; importable on another installation for multi-site replication or vertical-specific onboarding packs |
| OData v4 | - | OASIS-standard REST query protocol used by Microsoft Dynamics 365 F&O, SAP S/4HANA, Oracle ORDS; natively supported by the ERP connector protocol adapters in module 6.22 |
| CFIHOS | - | Capital Facilities Information HandOver Specification (based on ISO 15926); defines equipment tag format and property-set structure for handover packages in oil & gas and capital projects |

---

### Appendix D - External Tool Data Format (DataBridge Export)

The DataBridge module (6.10.11) exports failure datasets in the following formats for compatibility with external reliability tools:

| Tool | Export Format | Notes |
|---|---|---|
| GRIF (Total SA / Bureau Veritas) | CSV - TTF column, suspended flag | GRIF Weibull module CSV import format |
| Isograph Reliability Workbench | XML (Isograph schema v8) | Full failure dataset with suspended times |
| PTC Windchill Quality / RELEX | CSV - RELEX TTF format | Compatible with RELEX Weibull Analyzer |
| Generic | Standardized CSV: equipment_code, failure_date, restoration_date, ttf_hours, ttbf_hours, failure_mode, root_cause, is_suspended | Universal format for any external tool |

---

*End of PRD v3.0 - Maintafox Desktop*

**Document Control:**
- Authored by: Product & Architecture Division
- Review Status: v3.0 - Technical & Configurability Engineering Revision
- Previous Version: v2.0 (Comprehensive Engineering Revision - modules 6.16-6.25 + RAMS expansion)
- v3.0 Changes: Fixed fictional 'BoostedSQL' library (replaced with sea-orm + sqlx); corrected TLS certificate pinning to SPKI public key pin; clarified dual JWT strategy (HS256 local sessions / RS256 license from VPS); improved machine fingerprinting resilience (3-of-5 factor model); corrected Tauri update bundle strategy (full bundle v1.x, delta patches Phase 6+); expanded RBAC with 9 new permission domains and dynamic custom permission builder; added comprehensive ERP connector coverage (11 platforms + 6 middleware); added Module 6.26 Configuration Engine (7 sub-systems: Workflow Designer, Level Config, Custom Fields, Terminology Override, UI Layout Engine, Module Enable/Disable, Sequence Config); added Admin Dashboard spec Section 16.5 (console.maintafox.systems); updated feature tiering, TOC, delivery phases, and all appendices
- Next Review: After Phase 3 completion
- Distribution: Engineering Team, VPS Team, Product Owner, RAMS Engineering Lead

---
