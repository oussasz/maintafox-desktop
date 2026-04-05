# Module 6.12 Archive Explorer Research Brief

## 1. Research Position

This module should not be treated as a recycle bin with a nicer UI.

In a serious maintenance system, the archive is a governed historical-evidence workspace. It exists to preserve operational truth, auditability, retention policy, and cross-module traceability after records leave the active operational surface.

The key design distinction is this:

- some records are archived because they are finished historical evidence
- some records are soft-deleted and may be recoverable
- some records are retained only because audit, legal, or configuration-governance rules require them

If Maintafox treats all of those cases as the same "restore or hard delete" bucket, it will damage historical meaning and create compliance risk.

## 2. Source Signals

### 2.1 Maintafox already requires immutable historical meaning

The current Maintafox research and PRD direction already established several non-negotiable rules:

- 6.17 Activity Feed uses append-only events and explicitly states that older events are archived and remain accessible from Archive Explorer
- 6.26 Configuration Engine requires historical records to preserve the configuration version that governed them when they were created or transitioned
- 6.4 and 6.5 research already treated archived states as terminal historical outcomes rather than editable workflow records

That means 6.12 must become the read-only historical access layer for governed records, not a general-purpose trash folder.

### 2.2 Enterprise asset systems treat work history as retained evidence

IBM Maximo documentation repeatedly treats work orders, failure histories, and classifications as historical maintenance records that remain useful for later analysis. The implication is practical and important: completed maintenance records are not transient application rows. They are part of the evidence base for reliability, cost review, compliance, and continuous improvement.

### 2.3 Archive design must preserve cross-module meaning

Because Maintafox now links DI, WO, PM, inspection, permit, reliability, and budget modules into one evidence chain, archive behavior cannot preserve only the main header record. It must preserve:

- workflow state history
- attachments and evidence
- linked technical and organizational context
- configuration-version context where relevant

Otherwise archived records become visually present but analytically empty.

## 3. Operational Purpose

The operational purpose of Archive Explorer is to give controlled access to historical records that are no longer active but still matter for:

- audit and compliance review
- failure and maintenance history analysis
- cost and lifecycle review
- legal or contractual retention
- operational traceability after organizational or configuration changes
- recovery of eligible soft-deleted records

## 4. Data Capture Requirements

The module should preserve four layers of archive information.

### 4.1 Archive catalog metadata

- source module and source record
- archive class
- archived state and archive reason
- archive date and actor
- retention policy and purge eligibility
- restore eligibility

### 4.2 Immutable business snapshot

- the full record payload at archive time
- child records or linked evidence manifest where required
- workflow history snapshot
- linked asset, entity, and cost context

### 4.3 Retention and legal-control metadata

- retention end date
- legal hold flag
- purge policy
- export history

### 4.4 Recovery or purge journal

- restore requests and outcomes
- purge approvals and execution evidence
- archive access or export events where needed for sensitive records

## 5. Workflow Integrity

Archive behavior should follow explicit rules instead of acting like a generic delete action.

Recommended minimum model:

Active record -> Archived historical record
Soft-deleted eligible record -> Archive recovery bucket
Archived historical record -> Clone or follow-up only
Archive recovery bucket -> Restored when policy allows
Archived item -> Purged only after retention policy and approval checks

Key workflow rules:

- closed WOs, completed inspections, posted budget events, executed permits, and append-only audit logs should normally be non-restorable historical evidence
- some draft or administrative records may be restorable if policy allows
- purging must leave an immutable purge journal entry even if the archived payload is removed
- archive views must be read-only; operational correction should happen through follow-up records or clone actions, not by mutating archived evidence

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- retention policies by module and archive class
- archive folder or view taxonomy
- which modules support restore versus clone-only behavior
- export and purge approval rules

The tenant administrator should not be able to:

- rewrite archived payload content
- silently destroy protected historical records before retention conditions are met
- restore records whose analytical or compliance meaning depends on their terminal historical state
- sever archived records from their workflow or attachment history

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.4 and 6.5 for archived demand and work history
- 6.9 and 6.16 for historical PM and schedule evidence
- 6.10 for reliability traceability and bad-actor history
- 6.15 for archived document references and generated report packages
- 6.17 for archived audit and activity events
- 6.24 for archived financial snapshots and variance evidence
- 6.26 for configuration-version traceability and archived change sets

## 8. Bottom-Line Position For Maintafox

The design mistake would be to build 6.12 as a Windows Explorer clone with restore and delete buttons.

Maintafox should position this module as:

- a governed historical evidence workspace
- a retention-aware recovery surface for the limited records that are actually restorable
- a read-only traceability layer across operational, financial, and audit history

That is what makes Archive Explorer useful after the rest of the platform becomes more evidence-driven.

## 9. Recommended PRD Upgrade Summary

- separate operational archive, soft-delete recovery, and audit-retention use cases
- add immutable archive snapshots and retention-policy entities
- make restore eligibility explicit rather than universal
- support clone/follow-up actions for non-restorable historical records
- add legal hold, purge control, and purge journal requirements
- preserve linked workflow history, attachments, and configuration context

## 10. Source Set

- Maintafox PRD section 6.17 Activity Feed and Operational Audit Log
- Maintafox PRD section 6.26 Configuration Engine and Tenant Customization
- Maintafox research brief: MODULES_6_4_6_5_REQUEST_TO_WORK_ORDER_LIFECYCLE.md
- IBM Maximo documentation pattern: work orders and failure histories are retained as historical maintenance evidence