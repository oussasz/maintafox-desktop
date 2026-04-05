# Module 6.20 Training, Certification and Habilitation Management Research Brief

## 1. Research Position

This module should not be treated as a certificate-expiry register.

In a serious maintenance platform, training and habilitation are part of execution control. The module must determine whether a person is actually qualified to be assigned, scheduled, permitted, or released into hazardous work, and whether critical procedural acknowledgements are still current.

If Maintafox only stores expiry dates and PDF certificates, it will still allow unsafe or non-compliant assignment decisions elsewhere in the system.

## 2. Source Signals

### 2.1 IBM Maximo: qualifications are assignment constraints, not just HR records

IBM Maximo documents qualification requirements on both job plans and work orders. Those qualification requirements are then validated when labor and assignments are added. Historical qualification-requirement data is also preserved.

That matters directly for Maintafox because qualifications should flow from planned work into actual assignment validation, not remain a passive personnel reference.

### 2.2 ISO 45001 emphasizes competence, worker safety, and controlled operational readiness

ISO 45001 establishes a framework centered on risk management, operational control, legal compliance, worker participation, and continual improvement. For Maintafox, that supports:

- ensuring only qualified workers perform hazardous tasks
- linking competence to operational authorization
- treating expired or missing qualifications as execution blockers, not merely HR deficiencies

### 2.3 Maintafox now has document acknowledgements and hazardous-work controls

The upgraded 6.15 and 6.23 sections introduce two critical facts:

- some procedures may require acknowledgement before work proceeds
- some hazardous-work flows require explicit authorization and competency checks

That means 6.20 must link certification status, qualification requirements, and document acknowledgements into a unified readiness decision.

## 3. Operational Purpose

The operational purpose of this module is to:

- govern certifications, qualifications, and mandatory training evidence
- identify who may perform specific work safely and legally
- block assignment or permit activation when required competence is missing
- generate training needs from position, work-package, permit, and document-change requirements

## 4. Data Capture Requirements

The module should capture six classes of information.

### 4.1 Certification master data

- certification type, validity, authority, renewal rules, domain

### 4.2 Personnel qualification evidence

- issue and expiry dates
- issuing body and certificate reference
- verification status

### 4.3 Requirement profiles

- requirements originating from position, job plan, work order, permit type, or policy
- whether the requirement is mandatory, conditional, or overrideable

### 4.4 Training delivery data

- sessions, attendance, exam results, completion, and certificate issuance

### 4.5 Competence evaluation data

- readiness result for a specific person against a specific work requirement
- blocking reason

### 4.6 Document-acknowledgement linkage

- critical procedure acknowledgement requirements
- acknowledgement freshness and resulting training need or block state

## 5. Workflow Integrity

Recommended logic:

- requirement defined on position, job plan, work order, or permit type
- requirement evaluated at planning, assignment, and permit-activation points
- person status resolved as qualified, expiring, expired, missing, suspended, or awaiting document acknowledgement

Training and certification lifecycle should support:

Planned -> In Progress -> Completed -> Verified -> Issued or Renewed

Emergency overrides should be exceptional, policy-driven, time-limited, and auditable.

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- certification types and validity rules
- role and position requirement templates
- which work and permit types require which qualifications
- reminder lead times and dashboard thresholds
- override rules by domain

The tenant administrator should not be able to:

- disable qualification checks for protected hazardous-work scenarios without traceable policy
- treat document acknowledgements as optional where the product marks them as required for critical procedures
- erase historical qualification evidence or override history that affects auditability

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.5 Work Orders and planned labor for qualification requirements on executable work
- 6.9 PM plans and task packages for recurring qualification requirements
- 6.15 Documentation and Support Center for required procedure acknowledgements
- 6.16 Planning and Scheduling for readiness blocking when assigned resources are not qualified
- 6.14 Notification System for expiry, gap, and blocked-assignment alerts
- 6.23 Work Permit for hazardous-work authorization gating
- 6.26 Configuration Engine for requirement rules and protected safety guardrails

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.20 as a matrix of dates and uploaded certificate files.

Maintafox should position this module as:

- a competence and authorization control layer
- an assignment and permit gating input
- a training-needs engine driven by real operational requirements and document changes

## 9. Recommended PRD Upgrade Summary

- add qualification requirement profiles that can originate from position, job plan, work order, or permit type
- add competence evaluation and blocked-assignment logic
- link critical document acknowledgements to qualification readiness
- support audited overrides instead of silent non-compliant assignment
- align permissions with the existing `trn.*` RBAC domain

## 10. Source Set

- IBM Maximo Adding Qualification Requirements to Work Orders: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-adding-qualification-requirements-work
- IBM Maximo Adding Qualification Requirements to Job Plans: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-adding-qualification-requirements-job-plans
- ISO 45001:2018 summary: https://www.iso.org/standard/63787.html
- Maintafox PRD section 6.15 In-App Documentation & Support Center
- Maintafox PRD section 6.23 Work Permit System