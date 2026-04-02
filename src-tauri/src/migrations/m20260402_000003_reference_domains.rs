use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260402_000003_reference_domains"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── lookup_domains ────────────────────────────────────────────────────
        // Each domain is a named, versioned, governed set of values.
        // domain_key is the stable programmatic identifier (e.g. "equipment.class").
        // domain_type distinguishes operational list (user can extend) from
        // system-protected list (only admin can alter).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_domains"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("sync_id"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("domain_key"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    // human name shown in the UI
                    .col(
                        ColumnDef::new(Alias::new("display_name"))
                            .text()
                            .not_null(),
                    )
                    // "system" | "tenant" | "module"
                    .col(
                        ColumnDef::new(Alias::new("domain_type"))
                            .text()
                            .not_null()
                            .default("tenant"),
                    )
                    // which modules use this domain — informational, JSON array of module keys
                    .col(ColumnDef::new(Alias::new("consumer_modules_json")).text())
                    // whether values in this domain are orderable (have a sort_order)
                    .col(
                        ColumnDef::new(Alias::new("is_ordered"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // whether tenant can add values (0 = strictly system-managed)
                    .col(
                        ColumnDef::new(Alias::new("is_extensible"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // locked domains cannot be published after first install
                    .col(
                        ColumnDef::new(Alias::new("is_locked"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // minor version incremented on each publish cycle
                    .col(
                        ColumnDef::new(Alias::new("schema_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Alias::new("published_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("published_at")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("updated_at"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .to_owned(),
            )
            .await?;

        // ── lookup_values ─────────────────────────────────────────────────────
        // Individual governed values within a domain.
        // code is the stable programmatic key (e.g. "CORRECTIVE") — never changes.
        // label is the displayable name; fr_label and en_label allow i18n coupling.
        // sort_order controls display ordering for ordered domains.
        // is_system marks values that ship with the product and cannot be deleted.
        // color is an optional hex for status/badge color rendering.
        // metadata_json is an open extension bag for domain-specific attributes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_values"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("sync_id"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("domain_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("label")).text().not_null())
                    .col(ColumnDef::new(Alias::new("fr_label")).text())
                    .col(ColumnDef::new(Alias::new("en_label")).text())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(
                        ColumnDef::new(Alias::new("sort_order"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_system"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // optional hex color for badge/status rendering
                    .col(ColumnDef::new(Alias::new("color")).text())
                    // optional parent value id — for hierarchical domains (e.g. failure categories)
                    .col(ColumnDef::new(Alias::new("parent_value_id")).integer())
                    // free JSON extension bag for domain-specific extra attributes
                    .col(ColumnDef::new(Alias::new("metadata_json")).text())
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("updated_at"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // Compound unique: one code per domain (active values only enforced at app layer)
        manager
            .create_index(
                Index::create()
                    .name("idx_lookup_values_domain_code")
                    .table(Alias::new("lookup_values"))
                    .col(Alias::new("domain_id"))
                    .col(Alias::new("code"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ── lookup_value_aliases ───────────────────────────────────────────────
        // Aliases allow legacy codes, import abbreviations, and ERP synonyms to
        // resolve to the canonical governed value without polluting the master list.
        // Used during import mapping and ERP connector reconciliation (§6.13, §6.22).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("lookup_value_aliases"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("value_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("alias_code"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("alias_type"))
                            .text()
                            .not_null()
                            .default("import"),
                    )
                    // "import" | "erp" | "legacy" | "translation"
                    .col(ColumnDef::new(Alias::new("source_system")).text())
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("lookup_value_aliases"))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("lookup_values"))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("lookup_domains"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
