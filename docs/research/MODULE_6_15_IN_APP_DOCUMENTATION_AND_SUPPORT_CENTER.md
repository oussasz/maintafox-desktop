# Module 6.15 In-App Documentation and Support Center Research Brief

## 1. Research Position

This module should not be treated as a PDF drawer plus a generic help form.

In a serious maintenance platform, documentation is a controlled operational resource. It must deliver the correct approved procedure, safety instruction, diagnostic aid, or training reference at the point of work, including offline, while preserving document lifecycle, authorization, and auditability.

The support-center side of the module should likewise do more than submit a message. It should capture enough diagnostic and context information to support real issue resolution and product improvement.

## 2. Source Signals

### 2.1 UpKeep emphasizes historical documents and auditor-ready access

UpKeep's safety and compliance material highlights two ideas that matter here:

- historical documents must be easy to find
- safety and regulatory manuals must be available in an auditable form for operational use and audit review

That supports Maintafox treating documents as operational evidence, not just downloadable attachments.

### 2.2 ISO 45001 requires documented control of safety-related information

ISO 45001 centers risk control, emergency preparedness, legal compliance, and worker participation. In practice, this supports:

- keeping approved safety procedures available at the point of work
- controlling access to hazardous-work documentation
- maintaining evidence that critical instructions were made available and, where necessary, acknowledged

### 2.3 ISO 9001 reinforces complaint handling and continual improvement

ISO 9001 emphasizes customer focus, effective complaint resolution, process control, and continual improvement. That supports the support-center side of this module:

- support requests should be tracked through a clear lifecycle
- responses should preserve context and history
- issue data should support systematic improvement, not disappear into ad hoc communication

### 2.4 Maintafox already depends on contextual procedures and authorization gates

Existing Maintafox sections already imply a stronger document model:

- 6.23 PTW requires LOTO and hazardous-work procedures
- 6.20 training and habilitation records already govern authorization
- 6.12 archive and 6.17 audit logic require preserved evidence and access traceability

That means 6.15 should become the controlled documentation and support workspace for the rest of the platform.

## 3. Operational Purpose

The operational purpose of this module is to ensure that users can:

- find the right approved technical or safety instruction quickly
- access it in context of the asset, work type, or workflow step
- use it offline when field conditions require it
- prove which version was in force and who accessed or acknowledged it
- submit and track support issues with enough context for efficient resolution

## 4. Data Capture Requirements

The module should capture six classes of information.

### 4.1 Document master data

- reference, title, category, owner, confidentiality, status

### 4.2 Document version data

- version number, checksum, effective date, superseded date, change summary

### 4.3 Context binding data

- links to equipment families, assets, work types, permit types, inspection templates, PM versions, training topics, or hazard types

### 4.4 Access and acknowledgement data

- view, download, print, and acknowledgement events
- acknowledgement type and timestamp for critical documents

### 4.5 Help-content data

- module and state scope
- role scope
- article version and active status

### 4.6 Support-ticket data

- issue details, status history, message thread, local diagnostic bundle, and sync state

## 5. Workflow Integrity

Recommended document lifecycle:

Draft -> In Review -> Approved -> Effective -> Superseded or Withdrawn

Recommended support-ticket lifecycle:

Draft -> Queued -> Submitted -> Acknowledged -> In Progress -> Waiting for Customer -> Resolved -> Closed

Key workflow rules:

- only effective document versions should be shown by default for operational use
- superseded documents remain available for audit history but should be clearly labeled
- critical procedure revisions may require acknowledgement before use or before certain work can proceed
- offline support tickets should queue locally with preserved attachments and sync state until connectivity returns

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- document categories and binding rules
- review cycles and expiry reminders
- required acknowledgements for selected document classes
- offline availability policies and pinned packs
- help-article scope and tutorial collections

The tenant administrator should not be able to:

- erase document version history that has compliance or audit value
- expose restricted hazardous-work procedures to unauthorized users
- make critical procedures effectively invisible in the workflows that depend on them

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.3 Equipment Asset Registry for asset and family-linked documents
- 6.5 Work Orders for contextual procedures and work-package attachments
- 6.9 Preventive Maintenance for task-specific instructions and revision tracking
- 6.14 Notification System for document review reminders and support-ticket responses
- 6.17 Activity Feed for auditable document and support events
- 6.18 GED integration and update delivery settings
- 6.20 Training, Certification and Habilitation for authorization gates and acknowledgement-driven training follow-up
- 6.23 and 6.25 for LOTO, ATEX, inspection, and hazardous-work procedures

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.15 as a searchable attachment library with a basic vendor contact form.

Maintafox should position this module as:

- a controlled document workspace for technical and safety instructions
- a contextual offline knowledge layer for field execution
- a structured support and feedback channel that preserves issue history and diagnostics

## 9. Recommended PRD Upgrade Summary

- add controlled document lifecycle states and review governance
- add context bindings from documents to assets, work types, permits, inspections, and PM versions
- add access-event and acknowledgement tracking beyond simple downloads
- support offline document packs and critical-procedure pinning
- strengthen support tickets with message threads, diagnostic bundles, and offline queueing
- align the module with training, authorization, archive, and notification behavior

## 10. Source Set

- UpKeep Safety and Compliance page: https://upkeep.com/product/safety-and-compliance/
- ISO 45001:2018 summary: https://www.iso.org/standard/63787.html
- ISO 9001:2015 summary: https://www.iso.org/standard/62085.html
- Maintafox PRD sections 6.20, 6.23, 6.12, and 6.17