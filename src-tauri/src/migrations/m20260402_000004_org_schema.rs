use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260402_000004_org_schema"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── org_structure_models ──────────────────────────────────────────────
        // A versioned model of the tenant's organizational structure schema.
        // When the tenant publishes a structural change, a new version is created.
        // This allows historical records to reference the model version active
        // at the time the work was performed.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_structure_models"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(
                        ColumnDef::new(Alias::new("version_number"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // "draft" | "active" | "superseded" | "archived"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("draft"))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("activated_at")).text())
                    .col(ColumnDef::new(Alias::new("activated_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("superseded_at")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_node_types ────────────────────────────────────────────────────
        // Tenant-defined node type vocabulary (e.g. "Site", "Plant", "Zone",
        // "Building", "Process Unit", "Functional Position").
        // Capability flags define what governance rules apply to this node type.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_node_types"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("structure_model_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("label")).text().not_null())
                    .col(ColumnDef::new(Alias::new("icon_key")).text())
                    .col(ColumnDef::new(Alias::new("depth_hint")).integer())
                    // Capability flags (0/1 booleans as INTEGER):
                    .col(
                        ColumnDef::new(Alias::new("can_host_assets"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("can_own_work"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("can_carry_cost_center"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("can_aggregate_kpis"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("can_receive_permits"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_root_type"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_type_relationship_rules ───────────────────────────────────────
        // Defines which parent node types may contain which child node types.
        // This prevents invalid structural configurations at publish time.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_type_relationship_rules"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("structure_model_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("parent_type_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("child_type_id")).integer().not_null())
                    // min/max child count (NULL = unrestricted)
                    .col(ColumnDef::new(Alias::new("min_children")).integer())
                    .col(ColumnDef::new(Alias::new("max_children")).integer())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_nodes ─────────────────────────────────────────────────────────
        // The actual tenant-defined organizational nodes (e.g. "Usine Sud",
        // "Zone de production A", "Atelier mecanique").
        // parent_id is self-referencing. NULL parent_id = root node.
        // effective_from / effective_to support versioned structural changes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_nodes"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("node_type_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("parent_id")).integer())
                    // hierarchy path string for fast descendant queries
                    // format: "/1/4/17/" — id path from root to this node
                    .col(
                        ColumnDef::new(Alias::new("ancestor_path"))
                            .text()
                            .not_null()
                            .default("/"),
                    )
                    // depth in the hierarchy tree (0 = root)
                    .col(ColumnDef::new(Alias::new("depth")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("cost_center_code")).text())
                    .col(ColumnDef::new(Alias::new("external_reference")).text())
                    // "active" | "inactive" | "decommissioned" | "under_construction"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("active"))
                    .col(ColumnDef::new(Alias::new("effective_from")).text())
                    .col(ColumnDef::new(Alias::new("effective_to")).text())
                    .col(ColumnDef::new(Alias::new("erp_reference")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .col(ColumnDef::new(Alias::new("last_synced_checkpoint")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_org_nodes_parent_id")
                    .table(Alias::new("org_nodes"))
                    .col(Alias::new("parent_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_org_nodes_ancestor_path")
                    .table(Alias::new("org_nodes"))
                    .col(Alias::new("ancestor_path"))
                    .to_owned(),
            )
            .await?;

        // ── org_node_responsibilities ──────────────────────────────────────────
        // Named responsibility bindings on an org node.
        // responsibility_type is a governed value from lookup_domain
        // "org.responsibility.type" (e.g. maintenance_owner, production_owner,
        // hse_owner, planner, approver).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_node_responsibilities"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("node_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("responsibility_type")).text().not_null())
                    // references personnel.id (table created in a later migration)
                    .col(ColumnDef::new(Alias::new("person_id")).integer())
                    // references teams.id (table created in migration 006)
                    .col(ColumnDef::new(Alias::new("team_id")).integer())
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── org_entity_bindings ────────────────────────────────────────────────
        // Associates external identifiers (ERP plant codes, SAP functional locations,
        // legacy system codes) to org nodes for import and sync mapping.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_entity_bindings"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("node_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("binding_type")).text().not_null())
                    // "erp_plant" | "erp_cost_center" | "sap_fl" | "legacy_code" | "external_api"
                    .col(ColumnDef::new(Alias::new("external_system")).text().not_null())
                    .col(ColumnDef::new(Alias::new("external_id")).text().not_null())
                    .col(ColumnDef::new(Alias::new("is_primary")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in [
            "org_entity_bindings",
            "org_node_responsibilities",
            "org_nodes",
            "org_type_relationship_rules",
            "org_node_types",
            "org_structure_models",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
