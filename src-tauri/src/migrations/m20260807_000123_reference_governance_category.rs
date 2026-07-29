use sea_orm_migration::prelude::*;

/// Adds `governance_category` as the A/B/C source of truth on `reference_domains`.
/// Backfills from known domain codes and legacy `governance_level`.
/// Does not change runtime permissions by itself — policy module decides.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE reference_domains ADD COLUMN governance_category TEXT NOT NULL DEFAULT 'operational_dictionary'",
        )
        .await?;

        // Category A — system catalogs
        db.execute_unprepared(
            "UPDATE reference_domains SET governance_category = 'system_catalog' \
             WHERE UPPER(TRIM(code)) IN ( \
               'EQUIPMENT.STATUS', 'EQUIPMENT.CRITICALITY', 'EQUIPMENT.CLASS', \
               'DI.PRIORITY', 'DI.IMPACT_LEVEL' \
             ) \
             OR governance_level IN ('protected_analytical', 'system_seeded', 'erp_synced')",
        )
        .await?;

        // Category B — operational dictionaries (override A backfill where needed)
        db.execute_unprepared(
            "UPDATE reference_domains SET governance_category = 'operational_dictionary' \
             WHERE UPPER(TRIM(code)) IN ( \
               'EQUIPMENT.FAMILY', 'EQUIPMENT.SUBFAMILY', \
               'DI.SYMPTOM', 'DI.ORIGIN', 'PERSONNEL.SKILLS', \
               'ORG.SCHEDULE_CLASS' \
             ) \
             OR governance_level = 'tenant_managed'",
        )
        .await?;

        // Category C — controlled catalogs (must win over system_seeded / tenant_managed)
        db.execute_unprepared(
            "UPDATE reference_domains SET governance_category = 'controlled_catalog' \
             WHERE UPPER(TRIM(code)) IN ( \
               'WORK.FAILURE_MODES', 'PM.MAINTENANCE_TASK_LIST' \
             )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_reference_domains_governance_category \
             ON reference_domains(governance_category)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
