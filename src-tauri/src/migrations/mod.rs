use sea_orm_migration::prelude::*;

mod m20260401_000001_system_tables;
mod m20260401_000002_user_tables;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260401_000001_system_tables::Migration),
            Box::new(m20260401_000002_user_tables::Migration),
        ]
    }
}
