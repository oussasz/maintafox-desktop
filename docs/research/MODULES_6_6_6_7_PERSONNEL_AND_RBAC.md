# Modules 6.6 and 6.7 Research

## Personnel Management and RBAC as Workforce Readiness and Authorization Control

## 1. Research Position

These two modules should not be treated as separate admin pages.

In a serious maintenance platform:

- Personnel Management defines who is operationally available, skilled, authorized, and costed for work.
- RBAC defines what those people are allowed to see, approve, configure, and export.

If Maintafox keeps 6.6 as an HR-style roster and 6.7 as a checkbox matrix, planning, permits, training, security, and audit controls will still fail in practice.

## 2. Source Signals

### 2.1 Planning, training, and permits already require stronger workforce readiness logic

Maintafox now depends on 6.6 and 6.7 for:

- 6.16 readiness-aware scheduling and skill coverage
- 6.20 qualification and habilitation gating
- 6.23 permit issuer, witness, and authorized-person controls
- 6.24 labor-rate and cost-owner visibility where labor cost matters
- 6.1 identity binding between people, users, and devices

### 2.2 Authentication and settings now require stronger dangerous-action control

The 6.1 and 6.18 research already established that sensitive actions such as changing roles, disabling controls, restoring backups, or modifying integration credentials require step-up reauthentication and stronger auditability.

That means 6.7 must model dangerous permissions explicitly, not as ordinary checkboxes.

### 2.3 Tenant customization increases RBAC complexity, not lessens it

The 6.26 configuration engine already supports tenant-defined workflows, forms, module visibility, and custom permissions. That makes scoped authorization, permission dependencies, and delegated administration mandatory.

## 3. Operational Purpose

Together these modules must:

- define the maintenance workforce and contractor pool
- preserve availability, skill, authorization, and labor-cost context
- bind users to personnel identities and governed role assignments
- control dangerous actions with scope, delegation, and auditability

## 4. Data Capture Requirements

The combined model should capture six classes of control data.

### 4.1 Workforce identity data

- person, position, employer type, team, and supervisor

### 4.2 Availability and capacity data

- schedule template, exceptions, training blocks, leave, and restrictions

### 4.3 Skill and authorization data

- skill level, validation, permit authorization, and qualification linkage

### 4.4 Labor-cost and contractor data

- rate cards, vendor affiliation, contract validity, and onboarding status

### 4.5 User and role-assignment data

- linked user account, role, scope, and effective dates

### 4.6 Dangerous-action and delegation data

- permission dependencies
- delegated admin boundaries
- time-limited elevation or emergency access history

## 5. Workflow Integrity

Recommended workforce logic:

Available does not mean assignable.

Personnel can be blocked from assignment because of:

- training attendance
- qualification gap
- leave or medical restriction
- contractor onboarding issue
- manual safety hold

Recommended RBAC logic:

Draft or review role changes -> validate dependencies and scope -> activate -> audit

High-risk permission changes should require step-up reauthentication and append-only audit capture.

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- workforce structures, teams, shifts, and rate-card rules
- roles, custom permissions, and scoped assignments
- delegated-admin boundaries and role templates

The tenant administrator should not be able to:

- bypass qualification and hazardous-work gate logic through hidden role shortcuts
- grant contradictory or structurally invalid permission sets without warning or validation
- erase historical user-role assignments or dangerous-action audit history

## 7. Integration Expectations With The Rest Of Maintafox

These modules must integrate tightly with:

- 6.1 for identity, trusted devices, and reauthentication
- 6.16 for capacity, skill coverage, and scheduling readiness
- 6.18 for sensitive-action policy and step-up rules
- 6.20 for qualification and training status
- 6.23 for permit authorization roles
- 6.26 for custom permissions, module visibility, and workflow guards

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.6 as a personnel card directory and 6.7 as a static permission matrix.

Maintafox should position them as:

- a workforce readiness and labor-capacity registry
- a scoped authorization and dangerous-action control system

## 9. Recommended PRD Upgrade Summary

- strengthen workforce availability, skill, contractor, and labor-cost modeling
- bind users to scoped role assignments instead of only one global role
- add dangerous-permission handling, delegation, dependency checks, and access simulation
- align RBAC with step-up authentication and audit expectations already established elsewhere

## 10. Source Set

- Maintafox research brief: MODULE_6_1_AUTHENTICATION_AND_SESSION_MANAGEMENT.md
- Maintafox research brief: MODULES_6_9_6_16_PREVENTIVE_MAINTENANCE_AND_PLANNING_SCHEDULING.md
- Maintafox research brief: MODULE_6_20_TRAINING_CERTIFICATION_AND_HABILITATION.md
- Maintafox research brief: MODULE_6_18_APPLICATION_SETTINGS_AND_CONFIGURATION_CENTER.md
- Maintafox PRD section 6.23 Work Permit System
