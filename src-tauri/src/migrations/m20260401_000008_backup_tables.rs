use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000008_backup_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ─── 1. backup_policies ───────────────────────────────────────────────
        // Stores the tenant's configured backup policy.
        // encryption_mode: "plaintext" | "aes256"
        // In Phase 1 only "plaintext" is implemented (AES-256 encryption is Phase 2).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("backup_policies"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("policy_name"))
                            .string()
                            .not_null()
                            .default("default"),
                    )
                    .col(ColumnDef::new(Alias::new("target_directory")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("encryption_mode"))
                            .string()
                            .not_null()
                            .default("plaintext"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_daily"))
                            .integer()
                            .not_null()
                            .default(7),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_weekly"))
                            .integer()
                            .not_null()
                            .default(4),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_monthly"))
                            .integer()
                            .not_null()
                            .default(12),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Alias::new("updated_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 2. backup_runs ───────────────────────────────────────────────────
        // Immutable audit record of every backup execution.
        // status: "success" | "failed" | "partial"
        // trigger: "manual" | "scheduled" | "pre_update"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("backup_runs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("policy_id")).integer().null())
                    .col(
                        ColumnDef::new(Alias::new("trigger"))
                            .string()
                            .not_null()
                            .default("manual"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .string()
                            .not_null()
                            .default("success"),
                    )
                    .col(ColumnDef::new(Alias::new("output_path")).text().not_null())
                    .col(ColumnDef::new(Alias::new("file_size_bytes")).big_integer().null())
                    .col(ColumnDef::new(Alias::new("sha256_checksum")).string().null())
                    .col(
                        ColumnDef::new(Alias::new("encryption_mode"))
                            .string()
                            .not_null()
                            .default("plaintext"),
                    )
                    .col(ColumnDef::new(Alias::new("db_schema_version")).integer().null())
                    .col(
                        ColumnDef::new(Alias::new("started_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Alias::new("completed_at")).timestamp().null())
                    .col(ColumnDef::new(Alias::new("error_message")).text().null())
                    .col(ColumnDef::new(Alias::new("initiated_by_id")).integer().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("backup_runs")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("backup_policies")).to_owned())
            .await?;
        Ok(())
    }
}
