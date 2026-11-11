# Migration Registry

Auto-updated as migrations are added. Never manually reorder this list.
This document is the human-readable companion to migrations/mod.rs.

## Format

| # | File Stem | Tables Created / Modified | Phase | Sub-phase |
|---|-----------|--------------------------|-------|-----------|
| 001 | m20260401_000001_system_tables | system_config, trusted_devices, audit_events, app_sessions | 1 | 01 |
| 002 | m20260401_000002_user_tables | user_accounts, roles, permissions, role_permissions, user_scope_assignments | 1 | 01 |
| 003 | m20260402_000003_reference_domains | lookup_domains, lookup_values, lookup_value_aliases | 1 | 03 |
| 004 | m20260402_000004_org_schema | org_structure_models, org_node_types, org_type_relationship_rules, org_nodes, org_node_responsibilities, org_entity_bindings | 1 | 03 |
| 005 | m20260402_000005_equipment_schema | equipment_classes, equipment, equipment_hierarchy, equipment_meters, equipment_lifecycle_events | 1 | 03 |
| 006 | m20260402_000006_teams_and_skills | skill_categories, skill_definitions, teams, team_skill_requirements | 1 | 03 |
| 277 | m20261104_000277_wo_depannage | work_orders (is_depannage, breakdown_at, machine_stopped, validated_by_id, validated_at, validation_auto_approved: the dépannage fast path and its validation after the fact), notification_categories (wo_depannage_started, wo_depannage_permit_needed) | ux-fix | L1-05 |
| 279 | m20261104_000279_wo_required_permit_types | work_orders (permit_required_types_json: every permit type the plant's rules require, backfilled from the rule each work order carries) | ux-fix | L4-10b |
| 282 | m20261106_000282_di_safety_alert | notification_categories, notification_rules (locked safety notice `di_safety_alert`: a request flagged "sécurité" told to di.review and ptw.issue holders of its site; local to each PC) | ux-fix | L4-11 |
| 283 | m20261106_000283_permit_isolation_zero_energy | permit_isolations (zero_energy_note: the zero-energy test recorded when an isolation is verified) | ux-fix | L4-13 |

## Upcoming (reserved for Phase 2)

| Planned # | Working Title | Planned Sub-phase |
|-----------|---------------|-------------------|
| 007 | personnel_tables | Phase 2 · SP01 Personnel |
| 008 | di_request_tables | Phase 2 · SP02 DI |
| 009 | work_order_tables | Phase 2 · SP03 Work Orders |
| 010 | inventory_tables | Phase 2 · SP04 Inventory |
| 011 | pm_tables | Phase 2 · SP05 PM |
| 012 | notification_tables | Phase 2 · SP07 Notifications |
| 013 | planning_tables | Phase 2 · SP08 Planning |
| 014 | activity_audit_tables | Phase 2 · SP09 Activity/Audit |

## Destructive Migration History

No destructive migrations have been applied yet. This section records every migration
that included a DROP or RENAME statement plus the pre-destructive backup path.

*(empty)*
