use sea_orm_migration::prelude::*;

/// Reliability governance tables:
/// - Industrial beta benchmark references by equipment class code
/// - Dashboard policy thresholds/labels (danger, PM threshold)
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260804_000117_reliability_benchmarks_and_policies"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS reliability_beta_benchmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                equipment_class_code TEXT NOT NULL,
                standard_code TEXT NOT NULL,
                beta_reference REAL NOT NULL,
                source_document TEXT NOT NULL,
                revision_tag TEXT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_reliability_beta_benchmarks_class_standard
             ON reliability_beta_benchmarks(equipment_class_code, standard_code)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS reliability_dashboard_policies (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                scope_code TEXT NOT NULL UNIQUE,
                danger_threshold_r REAL NOT NULL,
                pm_threshold_r REAL NOT NULL,
                pm_label TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO reliability_dashboard_policies
                (scope_code, danger_threshold_r, pm_threshold_r, pm_label, created_at, updated_at)
             SELECT 'global', 0.5, 0.8, 'Suggested PM Intervention', datetime('now'), datetime('now')
             WHERE NOT EXISTS (
                SELECT 1 FROM reliability_dashboard_policies WHERE scope_code = 'global'
             )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_reliability_beta_benchmarks_class_standard")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS reliability_beta_benchmarks")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS reliability_dashboard_policies")
            .await?;
        Ok(())
    }
}
