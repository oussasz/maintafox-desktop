use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260723_000105_user_account_contact_fields"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Contact fields for Alerts/Notifications profile completeness.
        db.execute_unprepared("ALTER TABLE user_accounts ADD COLUMN email TEXT")
            .await?;
        db.execute_unprepared("ALTER TABLE user_accounts ADD COLUMN phone TEXT")
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support DROP COLUMN safely without table rebuild.
        Ok(())
    }
}
