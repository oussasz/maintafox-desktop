# Modules 6.17 and 6.19 Research

## Activity Feed, Audit Trail, and User Self-Service as Controlled Visibility Layers

## 1. Research Position

These two modules should not be treated as a news feed and a profile page.

In a serious maintenance platform:

- 6.17 is the append-only event visibility layer for operations, security, configuration, and compliance.
- 6.19 is the bounded personal control surface where users manage their own profile, preferences, sessions, and trusted devices without weakening administrative policy.

If Maintafox keeps them shallow, critical signals become hard to trace and personal security actions happen without clear audit boundaries.

## 2. Source Signals

### 2.1 Notifications and security already require a stronger event backbone

Maintafox now depends on 6.17 for:

- 6.14 notification-source traceability
- 6.18 settings-change and policy activation history
- 6.21 and 6.22 integration failure, replay, and webhook evidence
- 6.1 login, lock, unlock, and reauthentication events

That means 6.17 must separate operational feed needs from compliance-grade audit needs while keeping both append-only.

### 2.2 Self-service must stay within policy boundaries

The 6.1 and 6.18 research already established:

- trusted-device and offline policy are security concerns
- notification categories can be user-configurable only within admin guardrails
- fast unlock and device trust must not become a bypass around full identity policy

That means 6.19 should empower users without turning into an administrative back door.

## 3. Operational Purpose

Together these modules must:

- expose important cross-module operational events in a usable feed
- preserve immutable audit evidence for sensitive and regulated actions
- let users manage their own profile, preferences, and device trust within policy
- give users a personal lens on readiness, training, notifications, and security activity

## 4. Data Capture Requirements

The combined model should capture six classes of visibility data.

### 4.1 Operational event data

- event class, source, severity, scope, summary, and correlation ID

### 4.2 Audit event data

- actor, target, auth context, result, and before or after hashes where relevant

### 4.3 Event-linkage data

- caused-by and related-event chains across modules

### 4.4 Personal preference data

- notification, display, language, and saved-view choices

### 4.5 Trusted-device and session data

- enrolled device, last seen time, offline eligibility, and revoke state

### 4.6 Self-service action data

- password, PIN, contact, or device-management actions with outcome trail

## 5. Workflow Integrity

Recommended event rule:

source record changes create events; events never become the source of truth for operational state.

Recommended self-service rule:

users may edit only their own bounded preferences, profile fields, and trusted-device or local-unlock controls allowed by tenant policy.

Key workflow rules:

- activity events and audit events are append-only and archiveable, not editable logs
- event correlation should preserve chains such as IoT anomaly -> DI -> WO -> permit -> close-out
- self-service changes that affect security should require reauthentication where policy says so
- users cannot opt themselves out of mandatory compliance or safety notifications that the administrator marked as non-optional

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- event retention, visibility scope, and export policy
- which profile fields are user-editable
- whether local PIN or biometric unlock is allowed
- which notification categories may be personally muted or redirected

The tenant administrator should not be able to:

- disable audit capture for protected security or configuration actions
- let users silently revoke mandatory security controls through self-service
- mutate historical event meaning by editing stored event payloads

## 7. Integration Expectations With The Rest Of Maintafox

These modules must integrate tightly with:

- 6.1 for sign-in, session, trusted-device, and reauthentication history
- 6.14 for notification events and acknowledgement trails
- 6.18 for settings and policy change visibility
- 6.20 for personal qualification and training visibility
- 6.21 and 6.22 for integration and telemetry failure evidence
- 6.26 for user-visible layout and module-preference boundaries

## 8. Bottom-Line Position For Maintafox

The design mistake would be to keep 6.17 as a simple live feed and 6.19 as a static account page.

Maintafox should position them as:

- an operational and compliance event-journal layer
- a bounded personal control surface for user-owned settings and security visibility

## 9. Recommended PRD Upgrade Summary

- separate operational activity visibility from compliance-grade audit evidence
- add correlation tracing, retention tiers, and export governance
- strengthen profile, preference, device, and session self-service without weakening admin policy
- make self-service a personal readiness and security workspace rather than a generic settings page

## 10. Source Set

- Maintafox research brief: MODULE_6_1_AUTHENTICATION_AND_SESSION_MANAGEMENT.md
- Maintafox research brief: MODULE_6_14_NOTIFICATION_SYSTEM.md
- Maintafox research brief: MODULE_6_18_APPLICATION_SETTINGS_AND_CONFIGURATION_CENTER.md
- Maintafox research brief: MODULE_6_20_TRAINING_CERTIFICATION_AND_HABILITATION.md
- Maintafox research brief: MODULE_6_21_IOT_INTEGRATION_GATEWAY.md
- Maintafox research brief: MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md
