# Module 6.22 ERP and External Systems Connector Research Brief

## 1. Research Position

This module should not be treated as a giant connector catalog or a generic field-mapping table.

In a serious maintenance platform, ERP and external-system integration is about authoritative data ownership, versioned mapping contracts, replay-safe synchronization, and auditable reconciliation. The point is not to promise unrestricted bidirectional sync. The point is to keep Maintafox operationally coherent while exchanging trusted data with systems of record.

If Maintafox only lists supported ERP brands and a few protocol names, it will still fail in the real integration problems that matter: field ownership conflicts, duplicate posting, schema drift, and financial reconciliation gaps.

## 2. Source Signals

### 2.1 Earlier Maintafox modules already define why 6.22 needs stronger governance

Maintafox research already established that:

- 6.13 governs reference domains and alias mappings used to align external codes
- 6.18 governs secret-backed integration profiles and test-before-activate behavior
- 6.24 requires cost-center import and official posting export while preserving the difference between provisional and posted values
- 6.1 requires step-up reauthentication for changing ERP credentials or other sensitive integration controls

That means 6.22 has to govern contracts, source-of-record rules, and reconciliation behavior - not just connectivity.

### 2.2 OData-style enterprise integration emphasizes discoverability, paging, batching, and validation

Microsoft's Dynamics 365 documentation for OData highlights several properties that matter directly for Maintafox:

- discoverable service roots and metadata endpoints
- query, paging, and cross-company filtering behavior
- batch requests and single-transaction changesets
- validation logic performed by the server during create, update, and delete operations

This supports a design where Maintafox can:

- inspect available external entities before mapping
- validate field mappings against real metadata
- use batched, idempotent synchronization rather than ad hoc record posting

### 2.3 Integration quality depends on explicit authority boundaries

Maintafox budget, inventory, purchasing, equipment, and personnel modules all interact with external systems differently. Some domains are imported from ERP, some are exported to ERP, and some are split.

That makes explicit source-of-record rules mandatory. Without them, the same field can be overwritten in both directions until neither side is trustworthy.

## 3. Operational Purpose

The operational purpose of this module is to:

- define authoritative contracts for ERP and external-system exchange
- version and validate field mappings before activation
- run replay-safe inbound and outbound synchronization jobs
- preserve per-record provenance, acknowledgement, and reconciliation state
- distinguish operational local truth from externally accepted official postings

## 4. Data Capture Requirements

The module should capture six classes of integration-governance data.

### 4.1 External system profile data

- system family and transport type
- operating mode and activation state
- linked secure connection profile

### 4.2 Contract data

- business domain and direction
- source-of-record definition
- cursor or delta strategy and activation policy

### 4.3 Mapping-version data

- entity mapping and field mapping rules
- transforms and validation rules
- draft, tested, active, and retired status

### 4.4 Record-link and provenance data

- external keys and version tokens
- last sync timestamp
- field-authority rules and current sync state

### 4.5 Batch and item-processing data

- run window and trigger mode
- item-level payload hash and operation status
- retry count and failure reason

### 4.6 Exception and inbox-event data

- schema drift, conflict, auth, validation, or posting rejection type
- signature-verification state for inbound events
- resolution owner and outcome

## 5. Workflow Integrity

Recommended activation flow:

Discover -> Map -> Preview -> Test -> Activate -> Monitor -> Reconcile

Recommended outbound flow:

Queue -> Send -> Acknowledge -> Reconcile -> Retry or Suspend

Key workflow rules:

- secrets and endpoint credentials belong to 6.18 connection administration, while 6.22 owns contracts, mappings, jobs, and reconciliation
- mapping changes should be versioned and applied prospectively, not silently reinterpret historical sync history
- imported master data should preserve external identifiers and field authority rather than being treated as ordinary local edits
- financial and stock postings must distinguish locally recorded operational events from externally accepted official postings
- replay must be idempotent so a retried batch does not duplicate purchase, inventory, or cost transactions

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- external-system profiles and domain contracts
- mapping profiles, transforms, and schedule policies
- webhook or event triggers and replay rules
- conflict-handling policies and notification recipients

The tenant administrator should not be able to:

- make the same field fully authoritative in both systems without an explicit split-ownership rule
- hide source provenance for imported or exported records
- collapse provisional local costs into officially posted ERP results without acknowledgement state
- bypass testing and validation when activating a new contract or mapping version

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.3 Equipment Asset Registry for asset master import and external IDs
- 6.5 Work Orders for actual-cost export, reservation requests, and work-status-driven handoff
- 6.8 Inventory and Purchasing for material, stock, supplier, and requisition exchange
- 6.13 for reference-domain aliasing, external code mapping, and controlled taxonomy alignment
- 6.14 for integration-failure and reconciliation alerts
- 6.17 for append-only recording of sync runs, replays, and conflict resolutions
- 6.18 for secret-backed connection profiles and activation controls
- 6.24 for official posting, reconciliation, and cost-center synchronization
- 6.26 for business-configuration boundaries and protected posting rules

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.22 as a long ERP compatibility table with a sync button.

Maintafox should position this module as:

- a system-of-record integration layer
- a versioned mapping and synchronization contract manager
- a reconciliation and posting-governance workspace

## 9. Recommended PRD Upgrade Summary

- replace flat connector and field-map thinking with external-system profiles, contracts, mapping versions, record links, sync batches, and exceptions
- require explicit source-of-record and field-authority rules per integration domain
- add import preview, dry run, metadata-aware mapping, idempotent replay, and reconciliation workflows
- preserve the distinction between local operational actuals and externally posted official financial state
- align permissions to the existing `erp.*` RBAC domain instead of a single admin-only toggle

## 10. Source Set

- Microsoft Dynamics 365 OData documentation: https://learn.microsoft.com/en-us/dynamics365/fin-ops-core/dev-itpro/data-entities/odata
- Maintafox research brief: MODULE_6_13_LOOKUP_REFERENCE_DATA_MANAGER.md
- Maintafox research brief: MODULE_6_18_APPLICATION_SETTINGS_AND_CONFIGURATION_CENTER.md
- Maintafox research brief: MODULE_6_24_BUDGET_AND_COST_CENTER_MANAGEMENT.md
- Maintafox research brief: MODULE_6_1_AUTHENTICATION_AND_SESSION_MANAGEMENT.md
