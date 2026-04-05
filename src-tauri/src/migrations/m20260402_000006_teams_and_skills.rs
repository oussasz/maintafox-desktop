// src-tauri/src/migrations/m20260402_000006_teams_and_skills.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260402_000006_teams_and_skills"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── skill_categories ──────────────────────────────────────────────────
        // Top-level grouping for skills (e.g. "Mechanical", "Electrical",
        // "Instrumentation", "Safety"). Referenced by skill_definitions.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("skill_categories"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── skill_definitions ──────────────────────────────────────────────────
        // Governed skill vocabulary: programming, electrical, hydraulic, welding, etc.
        // is_authorization_required: true means having this skill requires a formal
        // qualification record in the training module (§6.20).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("skill_definitions"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("category_id")).integer())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // does possessing this skill require a training qualification record?
                    .col(
                        ColumnDef::new(Alias::new("is_authorization_required"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // typical revalidation period in months (0 = no expiry)
                    .col(
                        ColumnDef::new(Alias::new("revalidation_months"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
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

        // ── teams ─────────────────────────────────────────────────────────────
        // Maintenance teams scoped to org nodes. Used by work-order assignment,
        // planning, and workforce capacity views.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("teams"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    // "maintenance" | "inspection" | "planning" | "contractor" | "hse"
                    .col(
                        ColumnDef::new(Alias::new("team_type"))
                            .text()
                            .not_null()
                            .default("maintenance"),
                    )
                    .col(ColumnDef::new(Alias::new("primary_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // "active" | "inactive" | "disbanded"
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("active"))
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
                    .to_owned(),
            )
            .await?;

        // ── team_skill_requirements ───────────────────────────────────────────
        // Defines which skills a team expects to have coverage of.
        // Used by the workforce readiness dashboard.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("team_skill_requirements"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("team_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("skill_id")).integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("min_headcount"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Alias::new("required_proficiency"))
                            .integer()
                            .not_null()
                            .default(3),
                    )
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in [
            "team_skill_requirements",
            "teams",
            "skill_definitions",
            "skill_categories",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
