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
        ]
    }
}
