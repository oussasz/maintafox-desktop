# Module 6.14 Notification System Research Brief

## 1. Research Position

This module should not be treated as a pop-up utility.

In a serious maintenance platform, notifications are the event-routing, acknowledgement, and escalation layer that sits on top of workflow evidence. They exist to move the right person to the right response quickly, while preserving who was alerted, when, through which channel, and whether the alert was acknowledged.

If Maintafox treats notifications as simple toasts plus email toggles, it will create noise, miss critical escalations, and fail to support auditability for safety, SLA, and compliance events.

## 2. Source Signals

### 2.1 Maintafox already depends on timed escalation and acknowledgment

The current Maintafox PRD already contains multiple behaviors that require a real notification engine:

- DI SLA breaches must escalate when review deadlines are missed
- analytical alerts in 6.11 must notify configured recipients
- 6.18 already defines an escalation matrix by notification severity
- PTW, inspection, certification, and integration failures all require urgent role-based attention

This means 6.14 should become a governed alert-response layer for the rest of the platform, not a side utility.

### 2.2 ISO 45001 emphasizes emergency preparedness, worker participation, and response discipline

ISO 45001 describes an OH&S management framework centered on risk control, emergency planning, incident investigation, legal compliance, and continual improvement. That directly supports notification requirements such as:

- fast routing of safety-critical events
- explicit acknowledgement of urgent safety notifications
- escalation when hazards or compliance gaps remain unattended
- auditability of who was informed and when

### 2.3 Signal quality matters as much as delivery

The Maintafox research framework already emphasizes that operational systems should generate structured, calculation-grade data. The same principle applies here: notifications should be driven by governed source events, deduplicated, and linked to the source record. Otherwise the alerting layer becomes noisy and analytically useless.

## 3. Operational Purpose

The operational purpose of this module is to transform maintenance, safety, planning, reliability, and integration events into actionable alerts that:

- reach the right recipient or role
- use the right channel
- escalate when not acknowledged or resolved
- preserve an audit trail of delivery and acknowledgment

## 4. Data Capture Requirements

The module should capture five layers of notification data.

### 4.1 Source event data

- source module and source record
- event code and severity
- dedupe key and event payload
- event timestamp

### 4.2 Routing data

- recipient user, role, team, or entity manager
- routing rule that selected the recipients
- channel policy and escalation policy

### 4.3 Delivery data

- channel used
- delivery attempts and result
- send and delivery timestamps

### 4.4 Response data

- read timestamp
- acknowledgement timestamp
- snooze and escalation actions
- acknowledgement note where required

### 4.5 Notification-governance data

- suppression or dedupe action
- closure reason
- retention and archive behavior

## 5. Workflow Integrity

Recommended minimum notification lifecycle:

Source event detected -> Routed -> Delivered
Delivered -> Read
Read -> [Acknowledged | Snoozed | Auto-Closed]
Unacknowledged critical alert -> Escalated

Key workflow rules:

- the source record remains the system of record; acknowledging a notification does not silently close the underlying problem
- only critical or policy-defined alerts require explicit acknowledgement
- duplicate notifications for the same unresolved condition should be suppressed or rolled up
- resolved source records should auto-close their still-open notifications where appropriate

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- categories and severity defaults
- channel policy by category
- escalation steps and delay windows
- routing rules by role, team, entity, or assignee pattern
- quiet-hours or digest behavior for non-critical alerts

The tenant administrator should not be able to:

- disable minimum notification behavior required for protected safety or compliance events
- remove auditability of critical delivery or acknowledgement events
- change source-record truth through notification actions alone

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.4 and 6.5 for DI review, assignment, overdue work, and escalation
- 6.9 and 6.16 for PM due, missed work, schedule break-ins, and readiness blockers
- 6.11 for analytical alerts and KPI-derived exceptions
- 6.15 for support-ticket responses and critical document acknowledgements
- 6.17 for append-only activity capture of notification events and acknowledgements
- 6.18 for channel settings, SMTP/SMS configuration, and escalation matrix administration
- 6.20 for certification expiry and qualification-gap alerts
- 6.21 and 6.22 for IoT threshold and integration failure notifications
- 6.23 and 6.25 for permit expiry, suspension, missed inspections, and anomaly escalation

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.14 as a timer-driven toast system with category toggles.

Maintafox should position this module as:

- a governed event-routing and alert-response layer
- an acknowledgement and escalation engine for critical workflows
- a noise-controlled, auditable attention system for the whole platform

## 9. Recommended PRD Upgrade Summary

- add source notification events, routing rules, delivery attempts, and acknowledgement records
- distinguish event detection from channel delivery
- support dedupe, suppression, and escalation instead of one-alert-per-trigger noise
- make offline behavior explicit for local-first desktop use
- preserve source-of-truth discipline between notifications and operational records
- expose support-ticket and critical-document notifications as first-class cross-module events

## 10. Source Set

- Maintafox PRD section 6.4 Intervention Requests
- Maintafox PRD section 6.11 Analytical Alert Engine
- Maintafox PRD section 6.18 Notifications and Alerts settings
- ISO 45001:2018 summary: https://www.iso.org/standard/63787.html
- UpKeep Safety and Compliance page: https://upkeep.com/product/safety-and-compliance/