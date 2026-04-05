# Module 6.3 Equipment Asset Registry Research Brief

## 1. Research Position

This module should not be treated as a flat asset list with a few technical fields.

In a serious maintenance platform, the asset registry is the identity and lifecycle backbone for every DI, WO, PM occurrence, inspection, permit, reliability calculation, IoT condition event, budget rollup, and ERP handoff.

If Maintafox only stores names, serial numbers, and a hierarchy tree, it will still fail the harder questions:

- what exactly was installed where and when
- which maintainable boundary generated the failure history
- which external system record corresponds to this asset
- which historical reports should follow the old configuration versus the new one

## 2. Source Signals

### 2.1 Earlier Maintafox rewrites already assume a stronger asset backbone

Maintafox now depends on 6.3 for:

- 6.4 and 6.5 asset and component context on demand and execution records
- 6.9 and 6.16 asset criticality, meters, readiness scope, and PM targeting
- 6.10 stable failure and consequence history per maintainable item
- 6.21 governed equipment-to-signal binding
- 6.22 external IDs and import-safe asset synchronization
- 6.24 cost-of-failure and lifecycle cost attribution

That means 6.3 must preserve asset identity and history, not just current attributes.

### 2.2 Reference governance matters here

The 6.13 research already established that family, classification, unit, and failure semantics should be governed reference domains. That applies directly to equipment classes, family hierarchies, status codes, and criticality semantics.

### 2.3 Reliability and maintenance history require stable maintainable boundaries

The 6.10 reliability brief makes the core point clearly: weak operational identity leads to weak reliability analysis. Asset hierarchies must preserve the maintainable item boundary used by DI, WO, PM, and inspection evidence.

## 3. Operational Purpose

The operational purpose of this module is to:

- govern asset identity, hierarchy, and maintainable boundaries
- preserve installation, movement, replacement, and decommissioning history
- attach technical, documentary, meter, IoT, and ERP context to the correct asset record
- provide the canonical asset reference used by the rest of Maintafox

## 4. Data Capture Requirements

The module should capture six classes of asset-governance data.

### 4.1 Asset master identity

- stable code and class
- manufacturer and serial context
- status, commissioning, and decommissioning dates

### 4.2 Hierarchy and maintainable boundary data

- parent-child relationships
- functional positions and installed components
- which node represents the maintainable item

### 4.3 Technical and commercial context

- warranty, replacement value, supplier, and external IDs
- linked document sets and technical dossier references

### 4.4 Meter and condition context

- supported counters and primary meters
- IoT and imported condition references

### 4.5 Lifecycle history

- moves, reclassifications, replacements, and state changes
- reason and approval trail where policy requires it

### 4.6 Cross-system linkage

- ERP asset link
- document repository link
- telemetry link

## 5. Workflow Integrity

Recommended lifecycle:

Planned -> Commissioned -> In Service -> Standby or Out Of Service -> Preserved or Decommissioned

Key workflow rules:

- assets and components already referenced by historical records should not be hard-deleted
- replacement and install or remove activity should preserve before-and-after traceability
- reclassification must be version-safe so historical analytics do not silently change meaning
- document, PM, IoT, and ERP bindings should follow explicit validation rather than free-text links

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- asset classes and families through governed reference domains
- hierarchy depth and maintainable boundaries
- external ID mappings and QR label format
- display fields, technical attributes, and lifecycle statuses where allowed

The tenant administrator should not be able to:

- erase asset identity already referenced by work, cost, permit, or reliability history
- merge assets in a way that destroys historical provenance
- break the distinction between physical hierarchy and maintainable analytical boundary

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.2 for physical and organizational binding
- 6.4 and 6.5 for executable asset context
- 6.9 and 6.16 for PM targeting, counters, and planning scope
- 6.10 for failure history and bad-actor analysis
- 6.15 for technical document binding
- 6.21 for telemetry and condition-event linking
- 6.22 for asset-master synchronization and external IDs
- 6.24 for cost-of-failure and lifecycle cost views
- 6.26 for governed class, form, and field customization

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.3 as a descriptive equipment catalog.

Maintafox should position this module as:

- the governed asset-identity backbone
- the maintainable-item and lifecycle-history registry
- the canonical link point between operations, reliability, telemetry, documents, and ERP

## 9. Recommended PRD Upgrade Summary

- strengthen asset identity, hierarchy, and lifecycle governance
- preserve install, move, replace, and decommission history
- add external links, meter context, and technical dossier structure
- prevent historical evidence from being broken by asset edits

## 10. Source Set

- Maintafox research brief: MODULE_6_10_RELIABILITY_ENGINE.md
- Maintafox research brief: MODULE_6_13_LOOKUP_REFERENCE_DATA_MANAGER.md
- Maintafox research brief: MODULES_6_9_6_16_PREVENTIVE_MAINTENANCE_AND_PLANNING_SCHEDULING.md
- Maintafox research brief: MODULE_6_21_IOT_INTEGRATION_GATEWAY.md
- Maintafox research brief: MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md
