use sea_orm_migration::prelude::*;

mod m20260401_000001_system_tables;
mod m20260401_000002_user_tables;
mod m20260401_000008_backup_tables;
mod m20260402_000003_reference_domains;
mod m20260402_000004_org_schema;
mod m20260402_000005_equipment_schema;
mod m20260402_000006_teams_and_skills;
mod m20260404_000007_settings_tables;

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
        ]
    }
}
