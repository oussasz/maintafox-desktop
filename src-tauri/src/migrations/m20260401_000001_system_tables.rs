use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000001_system_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // system_config: app-level key-value settings
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("system_config"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("key")).string().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("value")).text())
                    .col(ColumnDef::new(Alias::new("updated_at")).timestamp().not_null())
                    .to_owned(),
            )
            .await?;

        // trusted_devices: devices that have completed online first-login
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("trusted_devices"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).text().not_null().primary_key())
                    .col(
                        ColumnDef::new(Alias::new("device_fingerprint"))
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Alias::new("device_label")).string())
                    .col(ColumnDef::new(Alias::new("user_id")).text().not_null())
                    .col(ColumnDef::new(Alias::new("trusted_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("last_seen_at")).timestamp())
                    .col(
                        ColumnDef::new(Alias::new("is_revoked"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Alias::new("revoked_at")).timestamp())
                    .col(ColumnDef::new(Alias::new("revoked_reason")).text())
                    .to_owned(),
            )
            .await?;

        // audit_events: immutable append-only event journal
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("audit_events"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).text().not_null().primary_key())
                    .col(ColumnDef::new(Alias::new("event_type")).string().not_null())
                    .col(ColumnDef::new(Alias::new("actor_id")).text())
                    .col(ColumnDef::new(Alias::new("actor_name")).string())
                    .col(ColumnDef::new(Alias::new("entity_type")).string())
                    .col(ColumnDef::new(Alias::new("entity_id")).text())
                    .col(ColumnDef::new(Alias::new("summary")).text())
                    .col(ColumnDef::new(Alias::new("detail_json")).text())
                    .col(ColumnDef::new(Alias::new("device_id")).text())
                    .col(ColumnDef::new(Alias::new("occurred_at")).timestamp().not_null())
                    .to_owned(),
            )
            .await?;

        // app_sessions: active local sessions
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("app_sessions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).text().not_null().primary_key())
                    .col(ColumnDef::new(Alias::new("user_id")).text().not_null())
                    .col(ColumnDef::new(Alias::new("device_id")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("expires_at")).timestamp().not_null())
                    .col(ColumnDef::new(Alias::new("last_activity_at")).timestamp())
                    .col(
                        ColumnDef::new(Alias::new("is_revoked"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("app_sessions")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("audit_events")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("trusted_devices")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("system_config")).to_owned())
            .await?;
        Ok(())
    }
}
