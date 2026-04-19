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
mod m20260615_000067_permit_domain_core;
mod m20260616_000068_permit_lifecycle_subentities;
mod m20260617_000069_work_orders_requires_permit;
mod m20260618_000070_qualification_schema;
mod m20260619_000071_loto_card_print_jobs;
mod m20260620_000072_training_sessions_attendance_ack;
mod m20260621_000073_inspection_rounds_core;
mod m20260622_000074_personnel_readiness_snapshots;
mod m20260623_000075_training_expiry_alert_events;
mod m20260624_000076_inspection_field_execution;
mod m20260625_000077_inspection_anomaly_routing;
mod m20260626_000078_inspection_reliability_signals;
mod m20260627_000079_budget_entity_sync_ids;
mod m20260628_000080_erp_export_batches_integration_exceptions;
mod m20260629_000081_failure_taxonomy;
mod m20260630_000082_failure_events_and_cost_of_failure_view;
mod m20260701_000083_runtime_exposure_and_kpi_snapshots;
mod m20260702_000084_ram_data_quality_view_and_dismissals;
mod m20260703_000085_closeout_validation_policies;
mod m20260704_000086_data_integrity_findings;
mod m20260705_000087_analytics_contract_versions;
mod m20260706_000088_reliability_kpi_analysis_input_metadata;
mod m20260707_000089_computation_jobs;
mod m20260708_000090_kpi_snapshot_plot_payload;
mod m20260709_000091_fmeca_rcm_weibull;
mod m20260710_000092_fta_rbd_event_tree;
mod m20260711_000093_markov_mc_guardrails;
mod m20260712_000094_ram_expert_sign_off;
mod m20260713_000095_user_dashboard_layouts;
mod m20260714_000096_report_library_schedules;
mod m20260715_000097_ram_ishikawa_diagrams;
mod m20260716_000098_ram_rca_links;
mod m20260717_000099_library_documents;
mod m20260718_000100_asset_photos;
mod m20260719_000101_grant_sync_view_for_ram_roles;
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
            Box::new(m20260615_000067_permit_domain_core::Migration),
            Box::new(m20260616_000068_permit_lifecycle_subentities::Migration),
            Box::new(m20260617_000069_work_orders_requires_permit::Migration),
            Box::new(m20260618_000070_qualification_schema::Migration),
            Box::new(m20260619_000071_loto_card_print_jobs::Migration),
            Box::new(m20260620_000072_training_sessions_attendance_ack::Migration),
            Box::new(m20260621_000073_inspection_rounds_core::Migration),
            Box::new(m20260622_000074_personnel_readiness_snapshots::Migration),
            Box::new(m20260623_000075_training_expiry_alert_events::Migration),
            Box::new(m20260624_000076_inspection_field_execution::Migration),
            Box::new(m20260625_000077_inspection_anomaly_routing::Migration),
            Box::new(m20260626_000078_inspection_reliability_signals::Migration),
            Box::new(m20260627_000079_budget_entity_sync_ids::Migration),
            Box::new(m20260628_000080_erp_export_batches_integration_exceptions::Migration),
            Box::new(m20260629_000081_failure_taxonomy::Migration),
            Box::new(m20260630_000082_failure_events_and_cost_of_failure_view::Migration),
            Box::new(m20260701_000083_runtime_exposure_and_kpi_snapshots::Migration),
            Box::new(m20260702_000084_ram_data_quality_view_and_dismissals::Migration),
            Box::new(m20260703_000085_closeout_validation_policies::Migration),
            Box::new(m20260704_000086_data_integrity_findings::Migration),
            Box::new(m20260705_000087_analytics_contract_versions::Migration),
            Box::new(m20260706_000088_reliability_kpi_analysis_input_metadata::Migration),
            Box::new(m20260707_000089_computation_jobs::Migration),
            Box::new(m20260708_000090_kpi_snapshot_plot_payload::Migration),
            Box::new(m20260709_000091_fmeca_rcm_weibull::Migration),
            Box::new(m20260710_000092_fta_rbd_event_tree::Migration),
            Box::new(m20260711_000093_markov_mc_guardrails::Migration),
            Box::new(m20260712_000094_ram_expert_sign_off::Migration),
            Box::new(m20260713_000095_user_dashboard_layouts::Migration),
            Box::new(m20260714_000096_report_library_schedules::Migration),
            Box::new(m20260715_000097_ram_ishikawa_diagrams::Migration),
            Box::new(m20260716_000098_ram_rca_links::Migration),
            Box::new(m20260717_000099_library_documents::Migration),
            Box::new(m20260718_000100_asset_photos::Migration),
            Box::new(m20260719_000101_grant_sync_view_for_ram_roles::Migration),
            Box::new(m_test::Migration),
        ]
    }
}
