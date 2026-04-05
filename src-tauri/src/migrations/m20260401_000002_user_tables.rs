use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000002_user_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── roles ────────────────────────────────────────────────────────────
        // System roles are seeded by the Rust seeder and cannot be deleted.
        // Custom roles are created by tenant administrators.
        // role_type: "system" | "custom"
        // status:    "draft" | "active" | "retired"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("roles"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("is_system")).integer().not_null().default(0))
                    .col(
                        ColumnDef::new(Alias::new("role_type"))
                            .text()
                            .not_null()
                            .default("custom"),
                    )
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
                    .to_owned(),
            )
            .await?;

        // ── permissions ───────────────────────────────────────────────────────
        // Permission name convention: domain.action[.scope]
        // Examples: eq.view, ot.create, adm.users, eq.import
        // is_dangerous=1: step-up reauthentication may be required
        // requires_step_up=1: MUST trigger step-up regardless of role settings
        // category: groups permissions by domain for UI display
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permissions"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("name")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(
                        ColumnDef::new(Alias::new("category"))
                            .text()
                            .not_null()
                            .default("general"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_dangerous"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("requires_step_up"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("is_system")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── role_permissions ──────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("role_permissions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("role_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("permission_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("granted_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("granted_by_id")).integer())
                    .primary_key(
                        Index::create()
                            .col(Alias::new("role_id"))
                            .col(Alias::new("permission_id")),
                    )
                    .to_owned(),
            )
            .await?;

        // ── user_accounts ─────────────────────────────────────────────────────
        // identity_mode: "local" | "sso" | "hybrid"
        // password_hash: argon2id hash; NULL for SSO-only users
        // pin_hash: optional fast-unlock PIN hash (argon2id, shorter parameters)
        // oauth_subject: external identity subject for SSO; NULL for local users
        // failed_login_attempts: reset on successful login; lockout at policy threshold
        // locked_until: populated after N failed logins; NULL = not locked
        // force_password_change: set for new accounts; clears on first password change
        // personnel_id: optional link to personnel record (migration 006+)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("user_accounts"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("username")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("display_name")).text())
                    .col(
                        ColumnDef::new(Alias::new("identity_mode"))
                            .text()
                            .not_null()
                            .default("local"),
                    )
                    .col(ColumnDef::new(Alias::new("password_hash")).text())
                    .col(ColumnDef::new(Alias::new("pin_hash")).text())
                    .col(ColumnDef::new(Alias::new("oauth_subject")).text())
                    .col(ColumnDef::new(Alias::new("personnel_id")).integer())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("is_admin")).integer().not_null().default(0))
                    .col(
                        ColumnDef::new(Alias::new("force_password_change"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Alias::new("failed_login_attempts"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("locked_until")).text())
                    .col(ColumnDef::new(Alias::new("last_login_at")).text())
                    .col(ColumnDef::new(Alias::new("last_seen_at")).text())
                    .col(ColumnDef::new(Alias::new("password_changed_at")).text())
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

        // username is used as login key — case-insensitive index
        manager
            .create_index(
                Index::create()
                    .name("idx_user_accounts_username_lower")
                    .table(Alias::new("user_accounts"))
                    .col(Alias::new("username"))
                    .to_owned(),
            )
            .await?;

        // ── user_scope_assignments ────────────────────────────────────────────
        // Binds a user to a role within a specific scope.
        // scope_type: "tenant" | "entity" | "site" | "team" | "org_node"
        // scope_reference: the id of the scope object (org_node.id, etc.)
        //                  NULL means tenant-wide scope
        // valid_from / valid_to: support temporary assignments and acting coverage
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("user_scope_assignments"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("user_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("role_id")).integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("scope_type"))
                            .text()
                            .not_null()
                            .default("tenant"),
                    )
                    .col(ColumnDef::new(Alias::new("scope_reference")).text())
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("assigned_by_id")).integer())
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
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_user_scope_assignments_user_id")
                    .table(Alias::new("user_scope_assignments"))
                    .col(Alias::new("user_id"))
                    .to_owned(),
            )
            .await?;

        // ── permission_dependencies ───────────────────────────────────────────
        // Warn or block configurations that combine dangerous permissions with
        // missing prerequisite visibility or edit permissions.
        // dependency_type: "hard" (blocked) | "warn" (advisory)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permission_dependencies"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("permission_name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("required_permission_name")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("dependency_type"))
                            .text()
                            .not_null()
                            .default("warn"),
                    )
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in &[
            "permission_dependencies",
            "user_scope_assignments",
            "user_accounts",
            "role_permissions",
            "permissions",
            "roles",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(*tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
