# Module 6.18 Application Settings and Configuration Center Research Brief

## 1. Research Position

This module should not be treated as a miscellaneous admin page.

In a serious maintenance platform, system settings are the operational control plane for security policy, secret-backed connections, notification governance, offline behavior, backup and recovery, and tenant-wide defaults.

The critical design distinction is this:

- 6.18 governs environment, policy, infrastructure, and operational controls
- 6.26 governs tenant business configuration such as workflows, forms, terminology, and structural semantics

If Maintafox mixes those responsibilities into one unstructured settings screen, it will become hard to validate, hard to audit, and risky to operate.

## 2. Source Signals

### 2.1 Maintafox authentication research already defines strong policy requirements

The authentication and session research already established key rules that 6.18 must operationalize:

- offline access requires prior online trust on the device
- device trust, offline grace, idle lock, and session maximum are distinct controls
- sensitive actions should require step-up reauthentication
- secure local storage should rely on OS-managed secret stores, not only the local database
- shared-device switching must clear decrypted state and preserve user isolation

Those are not just login concerns. They are settings-governance requirements.

### 2.2 Maintafox notification and documentation rewrites now depend on a stronger admin model

The upgraded 6.14 and 6.15 sections require 6.18 to manage:

- escalation policies
- channel retry and quiet-hours behavior
- document-service integration modes
- critical document acknowledgement reminders

That means 6.18 must move beyond static field storage and become a validated policy center.

### 2.3 Config changes need versioning, audit, and safe activation

The 6.26 governance research already established that high-impact configuration changes should be versioned, validated, and auditable. While 6.18 does not own the same business-configuration scope, sensitive settings changes still need:

- validation before activation
- audit trail of who changed what
- controlled storage of secret material
- rollback or revert path for failed operational settings

## 3. Operational Purpose

The operational purpose of this module is to let administrators safely manage:

- tenant-wide system policies
- connection endpoints and credentials
- notification and escalation settings
- document-service and analytics integrations
- backup, restore, and sync behavior
- device and session policies for local-first desktop operation

## 4. Data Capture Requirements

The module should capture six classes of settings data.

### 4.1 General settings values

- setting key and category
- scope and effective value
- validation status

### 4.2 Secret references

- secret type and backend
- secret handle or reference, not plaintext duplication
- rotation and validation timestamps

### 4.3 Policy snapshots

- policy domain
- version and activation timestamp
- who activated it

### 4.4 Connection profiles

- endpoint configuration
- auth method
- last successful test
- operational mode

### 4.5 Backup and sync controls

- schedule, retention, targets, encryption mode
- restore-test status

### 4.6 Change governance data

- who changed what
- old/new value hashes or summaries
- whether reauthentication was required
- activation or rollback result

## 5. Workflow Integrity

Not all settings need the same lifecycle.

Recommended rule:

- low-risk cosmetic settings may apply immediately but are still audited
- sensitive settings use Draft -> Test -> Activate -> Revert workflow

Sensitive settings include:

- SMTP, SMS, ERP, IoT, DMS, and analytics endpoints
- session and offline policy
- restore and recovery actions
- any setting that changes security, notification escalation, or document availability behavior

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- locale and presentation defaults
- notification policy and escalation matrices
- connection endpoints and integration profiles
- backup, retention, sync, and recovery preferences
- document-service integration modes and offline availability rules

The tenant administrator should not be able to:

- bypass secure secret handling
- weaken protected session or offline controls without audit and policy visibility
- silently activate untested high-risk endpoint settings
- confuse system-policy settings with tenant business-model configuration from 6.26

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.1 Authentication and Session Management for idle lock, session max, device trust, and offline grace policy
- 6.11 Analytics for Power BI and reporting delivery settings
- 6.14 Notification System for channel policy, retry rules, and escalation matrices
- 6.15 Documentation and Support Center for DMS connection rules and critical document reminder behavior
- 6.17 Activity Feed and Audit Log for immutable recording of sensitive settings changes
- 6.21 and 6.22 for IoT and ERP connection settings
- 6.26 Configuration Engine for boundary clarity between infrastructure settings and tenant business configuration

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.18 as a categorized list of fields with save buttons.

Maintafox should position this module as:

- a governed system-policy center
- a secure secret-backed integration control plane
- a validated backup, sync, and recovery administration workspace

## 9. Recommended PRD Upgrade Summary

- distinguish low-risk appearance settings from high-risk operational settings
- add secret references, policy snapshots, and change audit entities
- require test-before-activate for connection and security-sensitive settings
- explicitly operationalize offline grace, shared-device, and step-up reauth policies from the auth research
- tighten DMS, notification, and recovery administration behavior

## 10. Source Set

- Maintafox research brief: MODULE_6_1_AUTHENTICATION_AND_SESSION_MANAGEMENT.md
- Maintafox research brief: MODULES_6_2_6_26_ADMIN_DEFINED_OPERATING_MODEL.md
- Maintafox PRD sections 6.14 and 6.15