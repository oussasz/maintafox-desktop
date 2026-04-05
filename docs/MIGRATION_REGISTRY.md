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
