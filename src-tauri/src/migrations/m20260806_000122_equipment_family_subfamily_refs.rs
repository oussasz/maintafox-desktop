use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN equipment_family_ref_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN equipment_subfamily_ref_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_family_ref ON equipment(equipment_family_ref_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_subfamily_ref ON equipment(equipment_subfamily_ref_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
