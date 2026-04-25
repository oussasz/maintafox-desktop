//! Links work orders and FMECA items to Ishikawa (RCA) diagram nodes for traceability.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260716_000098_ram_rca_links"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN source_ram_ishikawa_diagram_id INTEGER NULL \
             REFERENCES ram_ishikawa_diagrams(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN source_ishikawa_flow_node_id TEXT NULL",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN source_rca_cause_text TEXT NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_work_orders_source_ram_ishikawa \
             ON work_orders(source_ram_ishikawa_diagram_id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE fmeca_items ADD COLUMN source_ram_ishikawa_diagram_id INTEGER NULL \
             REFERENCES ram_ishikawa_diagrams(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE fmeca_items ADD COLUMN source_ishikawa_flow_node_id TEXT NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_items_source_ram_ishikawa \
             ON fmeca_items(source_ram_ishikawa_diagram_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_fmeca_items_source_ram_ishikawa")
            .await?;
        db.execute_unprepared("ALTER TABLE fmeca_items DROP COLUMN source_ishikawa_flow_node_id")
            .await?;
        db.execute_unprepared("ALTER TABLE fmeca_items DROP COLUMN source_ram_ishikawa_diagram_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_work_orders_source_ram_ishikawa")
            .await?;
        db.execute_unprepared("ALTER TABLE work_orders DROP COLUMN source_rca_cause_text")
            .await?;
        db.execute_unprepared("ALTER TABLE work_orders DROP COLUMN source_ishikawa_flow_node_id")
            .await?;
        db.execute_unprepared("ALTER TABLE work_orders DROP COLUMN source_ram_ishikawa_diagram_id")
            .await?;
        Ok(())
    }
}
