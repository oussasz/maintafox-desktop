//! Migration 237 — `v_ram_data_quality_issues` reads the nature of an OT type like every other
//! reader.
//!
//! Migration 198 made the view select corrective work by the EN 13306 nature of the OT type, but
//! it compared the stored tag as written (`wot.semantic_tag IN (…)`), while the program reads a
//! tag trimmed of spaces, tabs and line breaks (`wo::types::MaintenanceNature::parse`). A type whose
//! tag carries such characters was corrective for the close-out, RAMS and the work mix, but its
//! work orders without a failure mode were missing from the RAMS quality list. The view is
//! recreated with the shared reading (`wo::types::tag_is_nature_sql`); its other three parts are
//! unchanged. No row is written: a view holds no data and is not synchronised.

use sea_orm::{ConnectionTrait, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::wo::types::{tag_is_nature_sql, MaintenanceNature};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000237_ram_quality_view_trimmed_nature"
    }
}

/// The view of migration 198, corrective work selected by the trimmed nature tag.
fn view_sql() -> String {
    let corrective = tag_is_nature_sql("wot.semantic_tag", &MaintenanceNature::CORRECTIVE);
    format!(
        "CREATE VIEW v_ram_data_quality_issues AS
         SELECT DISTINCT wo.equipment_id AS equipment_id,
                'MISSING_FAILURE_MODE' AS issue_code,
                'blocking' AS severity,
                'maintafox://ram/equipment/' || wo.equipment_id || '/failure-mode' AS remediation_url
         FROM work_orders wo
         INNER JOIN work_order_failure_details wfd ON wfd.work_order_id = wo.id
         INNER JOIN reference_values wot ON wot.id = wo.type_id
         WHERE wo.equipment_id IS NOT NULL
           AND wo.closed_at IS NOT NULL
           AND wfd.failure_mode_id IS NULL
           AND {corrective}

         UNION ALL

         SELECT e.id AS equipment_id,
                'MISSING_EXPOSURE' AS issue_code,
                'blocking' AS severity,
                'maintafox://ram/equipment/' || e.id || '/exposure' AS remediation_url
         FROM equipment e
         WHERE NOT EXISTS (
           SELECT 1 FROM runtime_exposure_logs rel
           WHERE rel.equipment_id = e.id
             AND rel.recorded_at >= datetime('now', '-90 days')
         )

         UNION ALL

         SELECT DISTINCT k.equipment_id AS equipment_id,
                'LOW_SAMPLE' AS issue_code,
                'warning' AS severity,
                'maintafox://ram/equipment/' || k.equipment_id || '/kpi' AS remediation_url
         FROM reliability_kpi_snapshots k
         WHERE k.equipment_id IS NOT NULL
           AND (k.data_quality_score < 0.85 OR k.event_count < 5)

         UNION ALL

         SELECT DISTINCT s.equipment_id AS equipment_id,
                'MISSING_INSPECTION_COVERAGE' AS issue_code,
                'warning' AS severity,
                'maintafox://ram/equipment/' || s.equipment_id || '/inspection' AS remediation_url
         FROM inspection_reliability_signals s
         WHERE s.checkpoint_coverage_ratio < 0.85"
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop and create in one transaction: the view is never missing for a reader.
        let txn = manager.get_connection().begin().await?;
        txn.execute_unprepared("DROP VIEW IF EXISTS v_ram_data_quality_issues")
            .await?;
        txn.execute_unprepared(&view_sql()).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The trimmed reading only adds work orders the raw comparison missed: kept.
        Ok(())
    }
}
