use sea_orm_migration::prelude::*;

mod m20260401_000001_system_tables;
mod m20260401_000002_user_tables;
mod m20260401_000008_backup_tables;
mod m20260402_000003_reference_domains;
mod m20260402_000004_org_schema;
mod m20260402_000005_equipment_schema;
mod m20260402_000006_teams_and_skills;
mod m20260404_000007_settings_tables;
mod m20260406_000009_org_audit_trail;
mod m20260401_000010_asset_registry_core;
mod m20260401_000011_asset_lifecycle_meter_docs;
mod m20260401_000012_asset_import_and_audit;
mod m20260401_000013_reference_domains_core;
mod m20260401_000014_reference_governance_maps;
mod m20260401_000015_reference_aliases_and_imports;
mod m20260401_000017_di_domain_core;
mod m20260401_000018_di_review_events;
mod m20260401_000019_di_attachments_sla;
mod m20260401_000020_di_change_events;
mod m20260408_000021_org_node_type_color;
mod m20260409_000022_wo_domain_core;
mod m20260410_000023_wo_execution_sub_entities;
mod m20260410_000024_wo_shift_column;
mod m20260410_000025_wo_closeout_and_attachments;
mod m20260411_000026_wo_change_events;
mod m20260411_000027_wo_conclusion_column;
mod m20260412_000028_rbac_scope_model;
mod m20260412_000029_permission_catalog;
mod m20260412_000030_admin_change_events;
mod m20260412_000031_rbac_settings_and_lockout;
mod m20260413_000032_rbac_hardening;
mod m20260413_000033_password_policy_settings;
mod m20260901_000034_notification_core;
mod m20261001_000035_archive_core;
mod m20261101_000036_activity_audit_log;
mod m20261201_000037_observability_permissions;
mod m20260414_000038_audit_events_integer_pk;
mod m20260415_000039_personnel_core;
mod m20260416_000040_personnel_readiness;
mod m20260417_000041_inventory_core;
mod m20260418_000043_inventory_movements_reservations;
mod m20260419_000044_inventory_item_master_hardening;
mod m20260420_000045_inventory_procurement_repairable_backbone;
mod m20260421_000046_inventory_cycle_count_and_reconciliation_controls;
mod m20260422_000047_inventory_valuation_cost_provenance;
mod m20260423_000048_pm_strategy_core;
mod m20260424_000049_equipment_status_code_normalization;
mod m20260425_000050_pm_occurrence_governance;
mod m20260426_000051_pm_execution_followup_notifications;
mod m20260427_000052_planning_core;
mod m20260428_000053_planning_capacity_commitment;
mod m20260429_000054_planning_breakins_notifications;
mod m20260430_000055_budget_baseline_core;
mod m20260501_000056_budget_actuals_commitments_forecasts;
mod m20260502_000057_budget_variance_erp_alignment;
mod m20260503_000058_budget_controls_alerting_reporting;
mod m20260504_000059_sync_envelope_contracts;
mod m20260505_000060_sync_conflicts_and_replay_workflows;
mod m20260506_000061_sync_repair_and_observability;
mod m20260507_000062_entitlement_envelopes_and_cache;
mod m20260508_000063_machine_activation_and_offline_policy;
mod m20260509_000064_license_enforcement_matrix_and_actions;
mod m20260510_000065_licensing_security_and_trace_chain;
mod m20260511_000066_vendor_console_permissions;
mod m_test;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260401_000001_system_tables::Migration),
            Box::new(m20260401_000002_user_tables::Migration),
            Box::new(m20260402_000003_reference_domains::Migration),
            Box::new(m20260402_000004_org_schema::Migration),
            Box::new(m20260402_000005_equipment_schema::Migration),
            Box::new(m20260402_000006_teams_and_skills::Migration),
            Box::new(m20260404_000007_settings_tables::Migration),
            Box::new(m20260401_000008_backup_tables::Migration),
            Box::new(m20260406_000009_org_audit_trail::Migration),
            Box::new(m20260401_000010_asset_registry_core::Migration),
            Box::new(m20260401_000011_asset_lifecycle_meter_docs::Migration),
            Box::new(m20260401_000012_asset_import_and_audit::Migration),
            Box::new(m20260401_000013_reference_domains_core::Migration),
            Box::new(m20260401_000014_reference_governance_maps::Migration),
            Box::new(m20260401_000015_reference_aliases_and_imports::Migration),
            Box::new(m20260401_000017_di_domain_core::Migration),
            Box::new(m20260401_000018_di_review_events::Migration),
            Box::new(m20260401_000019_di_attachments_sla::Migration),
            Box::new(m20260401_000020_di_change_events::Migration),
            Box::new(m20260408_000021_org_node_type_color::Migration),
            Box::new(m20260409_000022_wo_domain_core::Migration),
            Box::new(m20260410_000023_wo_execution_sub_entities::Migration),
            Box::new(m20260410_000024_wo_shift_column::Migration),
            Box::new(m20260410_000025_wo_closeout_and_attachments::Migration),
            Box::new(m20260411_000026_wo_change_events::Migration),
            Box::new(m20260411_000027_wo_conclusion_column::Migration),
            Box::new(m20260412_000028_rbac_scope_model::Migration),
            Box::new(m20260412_000029_permission_catalog::Migration),
            Box::new(m20260412_000030_admin_change_events::Migration),
            Box::new(m20260412_000031_rbac_settings_and_lockout::Migration),
            Box::new(m20260413_000032_rbac_hardening::Migration),
            Box::new(m20260413_000033_password_policy_settings::Migration),
            Box::new(m20260901_000034_notification_core::Migration),
            Box::new(m20261001_000035_archive_core::Migration),
            Box::new(m20261101_000036_activity_audit_log::Migration),
            Box::new(m20261201_000037_observability_permissions::Migration),
            Box::new(m20260414_000038_audit_events_integer_pk::Migration),
            Box::new(m20260415_000039_personnel_core::Migration),
            Box::new(m20260416_000040_personnel_readiness::Migration),
            Box::new(m20260417_000041_inventory_core::Migration),
            Box::new(m20260418_000043_inventory_movements_reservations::Migration),
            Box::new(m20260419_000044_inventory_item_master_hardening::Migration),
            Box::new(m20260420_000045_inventory_procurement_repairable_backbone::Migration),
            Box::new(m20260421_000046_inventory_cycle_count_and_reconciliation_controls::Migration),
            Box::new(m20260422_000047_inventory_valuation_cost_provenance::Migration),
            Box::new(m20260423_000048_pm_strategy_core::Migration),
            Box::new(m20260424_000049_equipment_status_code_normalization::Migration),
            Box::new(m20260425_000050_pm_occurrence_governance::Migration),
            Box::new(m20260426_000051_pm_execution_followup_notifications::Migration),
            Box::new(m20260427_000052_planning_core::Migration),
            Box::new(m20260428_000053_planning_capacity_commitment::Migration),
            Box::new(m20260429_000054_planning_breakins_notifications::Migration),
            Box::new(m20260430_000055_budget_baseline_core::Migration),
            Box::new(m20260501_000056_budget_actuals_commitments_forecasts::Migration),
            Box::new(m20260502_000057_budget_variance_erp_alignment::Migration),
            Box::new(m20260503_000058_budget_controls_alerting_reporting::Migration),
            Box::new(m20260504_000059_sync_envelope_contracts::Migration),
            Box::new(m20260505_000060_sync_conflicts_and_replay_workflows::Migration),
            Box::new(m20260506_000061_sync_repair_and_observability::Migration),
            Box::new(m20260507_000062_entitlement_envelopes_and_cache::Migration),
            Box::new(m20260508_000063_machine_activation_and_offline_policy::Migration),
            Box::new(m20260509_000064_license_enforcement_matrix_and_actions::Migration),
            Box::new(m20260510_000065_licensing_security_and_trace_chain::Migration),
            Box::new(m20260511_000066_vendor_console_permissions::Migration),
            Box::new(m_test::Migration),
        ]
    }
}
