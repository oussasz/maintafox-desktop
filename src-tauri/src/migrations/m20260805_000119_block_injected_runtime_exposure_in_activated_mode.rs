use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260805_000119_block_injected_runtime_exposure_in_activated_mode"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TRIGGER IF NOT EXISTS trg_runtime_exposure_logs_block_injected_on_activated
             BEFORE INSERT ON runtime_exposure_logs
             WHEN NEW.source_type = 'rams_injector'
               AND EXISTS (
                   SELECT 1
                   FROM app_settings s
                   WHERE s.setting_key = 'product.license_onboarding'
                     AND s.setting_scope = 'device'
                     AND instr(lower(COALESCE(s.setting_value_json, '')), '\"status\":\"active\"') > 0
               )
             BEGIN
               SELECT RAISE(ABORT, 'GATE_RUNTIME_EXPOSURE_SOURCE_NOT_ALLOWED_IN_PRODUCTION');
             END",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TRIGGER IF EXISTS trg_runtime_exposure_logs_block_injected_on_activated")
            .await?;
        Ok(())
    }
}
