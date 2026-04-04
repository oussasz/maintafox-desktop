use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260404_000007_settings_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // app_settings: governed settings by key + scope
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("app_settings"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("setting_key")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("setting_scope"))
                            .text()
                            .not_null()
                            .default("tenant"),
                    )
                    .col(ColumnDef::new(Alias::new("setting_value_json")).text().not_null())
                    .col(ColumnDef::new(Alias::new("category")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("setting_risk"))
                            .text()
                            .not_null()
                            .default("low"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("validation_status"))
                            .text()
                            .not_null()
                            .default("valid"),
                    )
                    .col(ColumnDef::new(Alias::new("secret_ref_id")).integer())
                    .col(ColumnDef::new(Alias::new("last_modified_by_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("last_modified_at"))
                            .text()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .index(
                        Index::create()
                            .if_not_exists()
                            .name("uq_app_settings_key_scope")
                            .table(Alias::new("app_settings"))
                            .col(Alias::new("setting_key"))
                            .col(Alias::new("setting_scope"))
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        // secure_secret_refs: keychain handle metadata only (no plaintext secrets)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("secure_secret_refs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("secret_scope")).text().not_null())
                    .col(ColumnDef::new(Alias::new("backend_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("secret_handle")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Alias::new("last_rotated_at")).text())
                    .col(ColumnDef::new(Alias::new("last_validated_at")).text())
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // connection_profiles: integration connector profile state
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("connection_profiles"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("integration_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("profile_name")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("config_json"))
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(ColumnDef::new(Alias::new("secret_ref_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null()
                            .default("draft"),
                    )
                    .col(ColumnDef::new(Alias::new("last_tested_at")).text())
                    .col(ColumnDef::new(Alias::new("last_test_result")).text())
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // policy_snapshots: versioned policy documents with active flag
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("policy_snapshots"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("policy_domain")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("version_no"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Alias::new("snapshot_json")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("activated_at")).text())
                    .col(ColumnDef::new(Alias::new("activated_by_id")).integer())
                    .to_owned(),
            )
            .await?;

        // settings_change_events: immutable append-only settings audit log
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("settings_change_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("setting_key_or_domain")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("change_summary"))
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Alias::new("old_value_hash")).text())
                    .col(ColumnDef::new(Alias::new("new_value_hash")).text())
                    .col(ColumnDef::new(Alias::new("changed_by_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("changed_at"))
                            .text()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Alias::new("required_step_up"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("apply_result"))
                            .text()
                            .not_null()
                            .default("applied"),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "settings_change_events",
            "policy_snapshots",
            "connection_profiles",
            "secure_secret_refs",
            "app_settings",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(table)).to_owned())
                .await?;
        }

        Ok(())
    }
}
