# Module 6.21 IoT Integration Gateway Research Brief

## 1. Research Position

This module should not be treated as a generic sensor list or a thin protocol bridge.

In a serious maintenance platform, the IoT layer is only valuable if it produces trusted condition and runtime evidence that other modules can act on safely. Raw telemetry volume alone does not improve maintenance decisions. In practice, 6.21 needs to function as an edge-aware condition evidence pipeline.

If Maintafox simply stores tags, last values, and threshold alerts, it will create noisy automation, weak audit trails, and poor analytical trust.

## 2. Source Signals

### 2.1 Existing Maintafox modules already depend on governed condition evidence

Earlier Maintafox research already established that:

- 6.4 and 6.5 need structured detection-source evidence, including IoT-triggered events and sensor snapshots
- 6.9 and 6.16 need trustworthy counters and condition-based triggers, not uncontrolled raw values
- 6.10 depends on condition and exposure inputs for reliability reasoning
- 6.14 must route IoT anomaly and gateway-failure notifications as governed operational events
- 6.18 now governs the high-risk connection settings and secret-backed profiles that 6.21 depends on

That means 6.21 must deliver semantically meaningful, quality-aware events to the rest of the product.

### 2.2 OPC UA shows industrial telemetry is more than periodic polling

The OPC Foundation describes OPC UA as a platform-independent architecture with hierarchical address spaces, subscriptions, events, information modeling, encryption, authentication, and auditing.

That matters for Maintafox because serious industrial integration should support:

- report-by-exception and event subscription where the source supports it
- contextual modeling of signals beyond a flat tag list
- certificate-based trust and auditable access

### 2.3 Edge telemetry needs store-and-forward and ordered replay

Microsoft's Azure IoT Edge offline guidance highlights several operationally important behaviors:

- upstream telemetry can be stored locally during disconnection
- locally cached state can keep downstream modules operating while offline
- replay should preserve message order when connectivity returns
- retention depends on explicit TTL and available disk capacity

Maintafox is local-first, so 6.21 should take offline buffering and replay seriously rather than assuming uninterrupted connectivity.

### 2.4 MQTT in industrial environments benefits from explicit session-state conventions

Eclipse Tahu positions Sparkplug as guidance for applying MQTT in industrial OT environments with:

- a defined topic namespace
- lifecycle verbs and session-state awareness
- efficient payload conventions for constrained links

For Maintafox, that supports treating gateway birth/death and device session state as first-class operational signals instead of silent disconnects.

## 3. Operational Purpose

The operational purpose of this module is to:

- acquire trusted telemetry and runtime counters from industrial sources
- normalize signal meaning, units, cadence, and quality state
- derive condition events that can feed DI, PM, inspection, reliability, and notification workflows
- preserve evidence windows and provenance so triggered work remains explainable later

## 4. Data Capture Requirements

The module should capture six classes of telemetry governance data.

### 4.1 Gateway and edge-runtime data

- protocol family and operating mode
- heartbeat and health status
- buffer backlog, replay lag, and last error

### 4.2 Signal-definition data

- source path or node reference
- equipment binding and semantic type
- engineering unit, expected cadence, and quality policy

### 4.3 Observation data

- source timestamp and ingest timestamp
- measured value and quality state
- replay or backfill status

### 4.4 Derived-rule data

- rule type and severity
- persistence, hysteresis, cooldown, and minimum quality requirements
- activation version and status

### 4.5 Condition-event data

- open and close timestamps
- evidence summary and trigger reason
- downstream outputs such as notification, DI, WO, or inspection linkage

### 4.6 Counter-application data

- counter delta and application window
- reset or rollover detection
- acceptance or rejection reason

## 5. Workflow Integrity

Recommended pipeline:

Receive -> Validate -> Normalize -> Persist -> Derive Event -> Route Action

Key workflow rules:

- a single noisy point should not create a WO unless a governed rule explicitly allows it
- stale, offline, simulated, backfilled, and bad-quality values must remain distinguishable from good live observations
- evidence around triggered events must be pinned before older raw data is compacted into aggregates
- runtime counters used for PM should record reset or rollover events instead of silently continuing accumulation
- protocol settings and credentials are administered in 6.18, but signal semantics and event logic belong in 6.21

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- gateway profiles and protocol behavior
- signal bindings to equipment and counters
- rule profiles for thresholds, persistence, and condition logic
- retention, buffering, and replay policies
- downstream actions such as notify, create DI, or update PM counters

The tenant administrator should not be able to:

- rewrite historical source timestamps or hide replay/backfill status
- treat bad-quality or simulated data as equivalent to trusted live measurements without visibility
- use 6.21 as a general-purpose SCADA write-back surface by default
- silently change active condition logic without versioning and activation control

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.3 Equipment Asset Registry for asset hierarchy, component context, and criticality
- 6.4 and 6.5 for DI and WO creation with evidence-rich IoT origin context
- 6.9 and 6.16 for counter-fed PM generation, readiness checks, and condition-based planning
- 6.10 for reliability inputs derived from governed condition evidence
- 6.14 for anomaly, gateway-failure, and backlog notifications
- 6.18 for secret-backed IoT connection profiles and activation testing
- 6.25 for inspection prioritization where condition events drive follow-up rounds
- 6.26 for rule-governance boundaries and protected operational guardrails

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.21 as a tag dashboard with direct threshold-to-work automation.

Maintafox should position this module as:

- a trusted telemetry acquisition layer
- a governed condition-event engine
- a runtime-counter and evidence service for maintenance workflows

## 9. Recommended PRD Upgrade Summary

- move from raw sensor CRUD to gateway, signal, observation, rule, event, and buffer governance
- distinguish live, stale, offline, simulated, and replayed data states
- add persistence, hysteresis, cooldown, and evidence-window logic before automatic downstream action
- treat store-and-forward buffering and replay lag as first-class operational data
- keep command and write-back out of default scope so the module stays maintenance-focused rather than SCADA-like

## 10. Source Set

- OPC Foundation OPC UA overview: https://opcfoundation.org/about/opc-technologies/opc-ua/
- Microsoft Azure IoT Edge offline capabilities: https://learn.microsoft.com/en-us/azure/iot-edge/offline-capabilities
- Eclipse Tahu project overview: https://projects.eclipse.org/projects/iot.tahu
- Maintafox research brief: MODULE_6_10_RELIABILITY_ENGINE.md
- Maintafox research brief: MODULES_6_9_6_16_PREVENTIVE_MAINTENANCE_AND_PLANNING_SCHEDULING.md
- Maintafox research brief: MODULE_6_14_NOTIFICATION_SYSTEM.md
