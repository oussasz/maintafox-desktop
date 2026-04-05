# Maintafox Research Framework

## Scope And Intent

This research program covers all 26 functional modules defined in Chapter 6 of the Maintafox PRD. It is not limited to foundational modules and it is not a superficial feature benchmark. The purpose is to redesign the module research around Maintafox as a full maintenance operating system.

The user direction is explicit:

- research all modules, not only the basic or entry modules
- treat workflow modules as scientific data-capture systems, not just ticket-routing screens
- ensure maintenance requests, work orders, permits, inspections, PM, planning, reliability, and cost modules together produce calculation-grade data
- avoid hardcoded organizational structures; the tenant administrator must be able to design the operating structure, terminology, and workflow logic
- keep the analysis professional, evidence-based, and implementation-aware

This document is therefore the governing framework for all subsequent module research briefs.

## Core Product Philosophy

Maintafox should be positioned as a configurable, local-first industrial maintenance platform whose workflows are designed to generate structured operational evidence.

That means the software must do more than track status. It must capture the raw facts required for:

- KPI production
- reliability calculations
- RAMS and RCM studies
- maintainability analysis
- failure pattern analysis
- cost allocation and lifecycle costing
- planning optimization
- auditability and compliance evidence

The design consequence is simple: every important operational event must be represented as structured data, not only as free text or a final status.

## Research Rules For Every Module

Each module brief from this point forward must answer five questions.

### 1. Operational Purpose

What business problem does the module solve in a real industrial maintenance environment, and which user roles depend on it?

### 2. Data Capture Value

What useful data does the module need to collect, and what downstream calculations or decisions depend on that data?

### 3. Workflow Integrity

What are the valid states, transitions, approvals, exceptions, and audit events? Which fields become mandatory at which stage?

### 4. Configurability Boundary

What should be fixed by the product for safety and consistency, and what should be admin-configurable at runtime?

### 5. Cross-Module Integration

How does the module feed analytics, reliability, planning, inventory, ERP, training, cost, HSE, or audit layers?

## Priority Design Principles

### A. Workflow Modules Must Produce Scientific Maintenance Data

The maintenance workflow modules must be designed as event and evidence systems. In practice, that means every DI, OT, PM execution, inspection, permit, and planning action should be able to produce structured records that support analysis later.

Examples of data that must be captured in a structured way across workflow modules:

- request timestamp, approval timestamp, scheduling timestamp, actual execution start and end timestamps
- equipment and sub-assembly context
- failure symptom, failure mode, failure cause, failure effect
- detection source: operator report, inspection, PM, IoT alarm, QA, safety observation
- downtime start, downtime end, production impact, safety impact, environmental impact
- urgency, criticality, risk class, business consequence, service level compliance
- labor by person, by skill, and by time segment
- parts planned versus parts actually consumed
- delay reasons and waiting causes: spare part, permit, manpower, shutdown window, vendor delay
- repair action, temporary fix indicator, permanent fix indicator, verification method, return-to-service confirmation
- repeat failure marker and recurrence interval
- links to permit, inspection finding, PM plan, action plan, root cause analysis, and cost posting

If these values remain optional free text, the platform will look complete but will not support serious analysis.

### B. The Organization Model Must Be Admin-Designed

The organization layer should not assume a universal industrial hierarchy such as group, plant, workshop, department, line, and zone. Some companies operate by site and area, others by business unit and asset class, others by geography and function, and some require hybrid models.

Therefore the organization model should be treated as a configurable meta-model:

- administrators define entity types themselves
- administrators define which entity types can be parents or children of others
- administrators control labels, codes, icons, optional attributes, and display order
- the system supports recursive depth, but does not impose one default vocabulary as the governing truth
- the system preserves referential integrity when modules attach records to those entities

The current PRD direction is partly aligned because it already mentions recursive entities and type customization, but it still presents a predefined default structure too strongly. Future research should push this module further toward a true admin-designed operating structure.

### C. Configurability Must Not Destroy Data Quality

Runtime configurability is necessary, but unrestricted configurability can make analytics useless. The right model is controlled configurability.

The administrator should be able to configure:

- workflow states and labels
- local terminology
- organization types and hierarchy rules
- custom fields
- priority and risk scales
- module visibility
- role-based permissions
- numbering sequences

The administrator should not be able to break:

- immutable audit history
- mandatory event timestamps required for core metrics
- referential integrity between workflow modules and assets
- minimal closure data required for reliability and cost calculations
- safety gates such as permit prerequisites for hazardous work

## Maintafox Research Program Structure

The module research will no longer be treated as a flat list. It will be treated as four interdependent research tracks.

### Track 1. Maintenance Execution And Evidence Backbone

Highest priority because this is where operational truth is created.

- 6.4 Intervention Requests
- 6.5 Work Orders
- 6.9 Preventive Maintenance Planning
- 6.16 Planning & Scheduling Engine
- 6.23 Work Permit System
- 6.25 Inspection Rounds & Checklists
- 6.10 Reliability Engineering Engine
- 6.24 Budget & Cost Center Management

Research emphasis:

- workflow design for complete event capture
- structured closure and root cause capture
- direct support for MTBF, MTTR, failure distribution, backlog aging, compliance, and cost metrics

### Track 2. Configurable Operating Model And Governance

This track defines how each tenant shapes the system.

- 6.2 Organization & Site Management
- 6.6 Personnel Management
- 6.7 Users, Roles & Permissions
- 6.18 Application Settings & Configuration Center
- 6.26 Configuration Engine & Tenant Customization
- 6.19 User Profile & Self-Service
- 6.20 Training, Certification & Habilitation Management

Research emphasis:

- admin-designed org model
- permission-driven workflow control
- competence and authorization linkage to execution work

### Track 3. Asset, Materials, And Technical Context

This track supplies the technical master data that makes workflow evidence useful.

- 6.3 Equipment Asset Registry
- 6.8 Spare Parts & Inventory Management
- 6.13 Lookup / Reference Data Manager
- 6.21 IoT Integration Gateway
- 6.22 ERP & External Systems Connector

Research emphasis:

- master-data quality
- failure coding systems
- material and cost traceability
- meter and sensor context for condition-based reasoning

### Track 4. Intelligence, Communication, And Control

This track converts operational evidence into action and visibility.

- 6.1 Authentication & Session Management
- 6.11 Analytics & Dashboard
- 6.12 Archive Explorer
- 6.14 Notification System
- 6.15 In-App Documentation & Support Center
- 6.17 Activity Feed & Operational Audit Log

Research emphasis:

- trusted access to evidence
- analytics grounded in structured events
- auditable user action history

## Required Data-Capture Standard For Workflow Research

For the workflow-centered modules, every future research brief must explicitly identify three layers of data.

### Layer 1. Transactional Control Data

The data needed to move the workflow itself:

- status
- assignee
- priority
- approval decision
- due date
- comments

### Layer 2. Operational Evidence Data

The data needed to understand what actually happened in the field:

- technical context
- failure classification
- intervention actions
- manpower and duration
- material usage
- delay causes
- verification outcome

### Layer 3. Analytical Derivation Data

The fields and timestamps needed to compute metrics later:

- time-to-acknowledge
- time-to-approve
- time-to-schedule
- waiting time before execution
- wrench time versus delay time
- downtime duration
- maintenance cost per event
- recurrence rate
- closure quality completeness

If a module brief does not define all three layers, the research is incomplete.

## Specific Correction Direction For Key Modules

### Organization Module

The research direction for 6.2 must shift from predefined structure management to configurable organizational architecture.

The target model should support:

- a tenant-defined catalog of organization node types
- optional rules for permitted parent-child relationships
- tenant-defined terminology per node type
- optional metadata per node type such as cost center, maintenance area, production line, region, shutdown window, or HSE zone
- ability to attach assets, personnel, budgets, and workflows to any valid node type

### Intervention Requests And Work Orders

The research direction for 6.4 and 6.5 must shift from workflow completeness toward analysis-grade maintenance evidence.

The target model should support:

- mandatory structured coding at the right workflow stage, not only at request intake
- progressive enrichment of the record as the case moves from request to execution to closure
- separate recording of symptom, diagnosis, root cause, corrective action, and verification result
- separation of active repair time from waiting and delay time
- clear links to downtime, cost, permit, part consumption, and recurrence analysis
- closure-quality rules so incomplete records cannot silently pollute analytics

## Deliverable Format For Future Module Briefs

Each module brief should follow this structure:

1. Module role in the maintenance operating model
2. Research-backed operating pattern from standards and competitor systems
3. Required workflow and state model
4. Required data model for operational evidence and calculations
5. UX and user-role behavior
6. Configurability rules for administrators
7. Cross-module integration and downstream analytics value
8. Recommended corrections to the current PRD

This format is stricter than the first 6.1 brief and will be used for the remaining modules.

## Immediate Consequence For The Research Sequence

The next research steps should not simply continue numerically without adjustment. The right sequence is:

1. Reframe the workflow-heavy modules first because they define the evidence backbone.
2. Reframe the organization and configuration modules next because they define tenant adaptability.
3. Then continue through the remaining modules with this standard.

Numerical order can still be preserved in the written outputs, but the analytical method must now follow this framework.

## Bottom Line

The correct interpretation of the Maintafox research mission is no longer "describe each module professionally." It is:

"Design every module as part of a configurable industrial maintenance system whose workflows generate structured, scientifically usable operational data."

That interpretation will govern the research from this point onward.