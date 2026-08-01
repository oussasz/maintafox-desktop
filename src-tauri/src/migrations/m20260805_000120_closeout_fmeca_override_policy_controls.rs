use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260805_000120_closeout_fmeca_override_policy_controls"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let has_distinct = db
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT 1 FROM pragma_table_info('closeout_validation_policies') \
                 WHERE name = 'fmeca_parts_override_require_distinct_signer' LIMIT 1"
                    .to_string(),
            ))
            .await?
            .is_some();
        if !has_distinct {
            db.execute_unprepared(
                "ALTER TABLE closeout_validation_policies \
                 ADD COLUMN fmeca_parts_override_require_distinct_signer INTEGER NOT NULL DEFAULT 1",
            )
            .await?;
        }
        let has_roles = db
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT 1 FROM pragma_table_info('closeout_validation_policies') \
                 WHERE name = 'fmeca_parts_override_allowed_roles_json' LIMIT 1"
                    .to_string(),
            ))
            .await?
            .is_some();
        if !has_roles {
            db.execute_unprepared(
                "ALTER TABLE closeout_validation_policies \
                 ADD COLUMN fmeca_parts_override_allowed_roles_json TEXT NOT NULL DEFAULT '[\"Supervisor\",\"Maintenance Supervisor\",\"Administrator\",\"Superadmin\"]'",
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
