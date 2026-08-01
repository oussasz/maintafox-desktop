//! Migration 028 — RBAC Scope Model Augmentation
//!
//! Phase 2 - Sub-phase 06 - File 01.
//!
//! Augments the Phase 1 RBAC baseline (migration 002) with:
//! - Emergency elevation columns on `user_scope_assignments`
//! - Additional indexes on `user_scope_assignments` (unique composite, role, scope)
//! - Unique pair index on `permission_dependencies`
//! - `role_templates` table
//! - `delegated_admin_policies` table
//! - 5 domain-specific system roles (seeded)
//! - 4 role templates (seeded)
//! - 19 permission dependency pairs (seeded, mapped to actual permission names)
//!
//! Prerequisites: migrations 001-002 (user_accounts, roles, permissions,
//! role_permissions, user_scope_assignments, permission_dependencies).

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260412_000028_rbac_scope_model"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Augment user_scope_assignments with emergency columns ─────
        // SQLite does not support ADD COLUMN IF NOT EXISTS; use PRAGMA check.
        if !column_exists(db, "user_scope_assignments", "is_emergency").await? {
            db.execute_unprepared(
                "ALTER TABLE user_scope_assignments ADD COLUMN is_emergency INTEGER NOT NULL DEFAULT 0",
            )
            .await?;
        }

        if !column_exists(db, "user_scope_assignments", "emergency_reason").await? {
            db.execute_unprepared(
                "ALTER TABLE user_scope_assignments ADD COLUMN emergency_reason TEXT NULL",
            )
            .await?;
        }

        if !column_exists(db, "user_scope_assignments", "emergency_expires_at").await? {
            db.execute_unprepared(
                "ALTER TABLE user_scope_assignments ADD COLUMN emergency_expires_at TEXT NULL",
            )
            .await?;
        }

        // ── 2. Additional indexes on user_scope_assignments ──────────────
        // Deduplicate existing rows before adding the UNIQUE index.
        // Keep only the row with the lowest id for each (user_id, role_id,
        // scope_type, scope_reference) combination — removes duplicates
        // accumulated by earlier seeder runs that lacked a unique guard.
        db.execute_unprepared(
            "DELETE FROM user_scope_assignments \
             WHERE id NOT IN ( \
               SELECT MIN(id) FROM user_scope_assignments \
               GROUP BY user_id, role_id, scope_type, COALESCE(scope_reference, '') \
             )",
        )
        .await?;

        // Unique composite: prevent duplicate (user, role, scope) assignments.
        // COALESCE handles NULL scope_reference for tenant-wide assignments.
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_usa_user_role_scope \
             ON user_scope_assignments(user_id, role_id, scope_type, COALESCE(scope_reference, ''))",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_usa_role ON user_scope_assignments(role_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_usa_scope \
             ON user_scope_assignments(scope_type, scope_reference)",
        )
        .await?;

        // ── 3. Unique pair index on permission_dependencies ──────────────
        // Required for INSERT OR IGNORE semantics on dependency seeding.
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_pd_pair \
             ON permission_dependencies(permission_name, required_permission_name)",
        )
        .await?;

        // ── 4. Create role_templates table ───────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS role_templates (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL UNIQUE,
                description     TEXT NULL,
                module_set_json TEXT NOT NULL DEFAULT '[]',
                is_system       INTEGER NOT NULL DEFAULT 0,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        // ── 5. Create delegated_admin_policies table ─────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS delegated_admin_policies (
                id                            INTEGER PRIMARY KEY AUTOINCREMENT,
                admin_role_id                 INTEGER NOT NULL REFERENCES roles(id),
                managed_scope_type            TEXT NOT NULL,
                managed_scope_reference       TEXT NULL,
                allowed_domains_json          TEXT NOT NULL DEFAULT '[]',
                requires_step_up_for_publish  INTEGER NOT NULL DEFAULT 1,
                created_at                    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        // ── 6. Seed 5 domain-specific system roles ──────────────────────
        // These complement the Phase 1 baseline roles (Administrator,
        // Supervisor, Operator, Readonly) with CMMS-specific archetypes
        // as defined in PRD §6.7. INSERT OR IGNORE respects the UNIQUE
        // constraint on roles.name — safe to re-run.
        let now = chrono::Utc::now().to_rfc3339();

        let system_roles: &[(&str, &str)] = &[
            (
                "Superadmin",
                "Full system access, all permissions, cannot be restricted",
            ),
            (
                "Maintenance Supervisor",
                "Manages WOs, DIs, personnel assignments, and planning",
            ),
            (
                "Maintenance Technician",
                "Executes WOs, submits DIs, records labor and parts",
            ),
            (
                "Planner/Scheduler",
                "Plans and schedules WOs and PM occurrences",
            ),
            (
                "Read Only Observer",
                "Read-only access to all operational modules, no write permissions",
            ),
        ];

        for (name, desc) in system_roles {
            let sync_id = uuid::Uuid::new_v4().to_string();
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO roles \
                 (sync_id, name, description, role_type, status, is_system, created_at, updated_at, row_version) \
                 VALUES (?, ?, ?, 'system', 'active', 1, ?, ?, 1)",
                [
                    sync_id.into(),
                    (*name).into(),
                    (*desc).into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        // ── 7. Seed 4 role templates ─────────────────────────────────────
        // Templates are starting points for custom role creation — applying
        // a template copies permissions into a new role, not a permanent link.
        let templates: &[(&str, &str, &str)] = &[
            (
                "Supervisor Template",
                "Pre-packaged permissions for a site maintenance supervisor",
                r#"["ot","di","pm","per","eq","inv"]"#,
            ),
            (
                "Technician Template",
                "Pre-packaged permissions for a field technician",
                r#"["ot","di","eq"]"#,
            ),
            (
                "Planner Template",
                "Pre-packaged permissions for a PM planner/scheduler",
                r#"["ot","di","pm","plan","inv"]"#,
            ),
            (
                "Observer Template",
                "Read-only access to main operational modules",
                r#"["ot.view","di.view","pm.view","eq.view"]"#,
            ),
        ];

        for (name, desc, module_set) in templates {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO role_templates \
                 (name, description, module_set_json, is_system) \
                 VALUES (?, ?, ?, 1)",
                [(*name).into(), (*desc).into(), (*module_set).into()],
            ))
            .await?;
        }

        // ── 8. Seed 19 permission dependency pairs ───────────────────────
        // Mapped to actual seeded permission names from the system seeder.
        //
        // Adaptations from roadmap spec to real permission names:
        //   di.submit  → di.create      (actual name in di_permission_domain)
        //   eq.edit    → eq.manage       (actual name in seeder)
        //   pm.edit    → pm.manage       (actual name in seeder)
        //   pm.delete  → pm.approve      (closest hierarchical equivalent)
        //   inv.procure→ inv.order       (actual name in seeder)
        //   inv.count  → inv.adjust      (actual name in seeder)
        //   Added: di.create.own→di.view (sub-permission, same dependency logic)
        let deps: &[(&str, &str, &str)] = &[
            // ── Work Orders (ot) ──
            ("ot.create", "ot.view", "hard"),
            ("ot.edit", "ot.view", "hard"),
            ("ot.approve", "ot.view", "hard"),
            ("ot.close", "ot.edit", "hard"),
            ("ot.reopen", "ot.close", "warn"),
            // ── Intervention Requests (di) ──
            ("di.create", "di.view", "hard"),
            ("di.create.own", "di.view", "hard"),
            ("di.review", "di.view", "hard"),
            ("di.approve", "di.review", "hard"),
            ("di.convert", "di.approve", "hard"),
            // ── Equipment (eq) ──
            ("eq.manage", "eq.view", "hard"),
            ("eq.delete", "eq.manage", "warn"),
            // ── Preventive Maintenance (pm) ──
            ("pm.manage", "pm.view", "hard"),
            ("pm.approve", "pm.manage", "warn"),
            // ── Inventory (inv) ──
            ("inv.manage", "inv.view", "hard"),
            ("inv.order", "inv.view", "hard"),
            ("inv.adjust", "inv.view", "hard"),
            // ── Administration (adm) ──
            ("adm.roles", "adm.users", "warn"),
            ("adm.permissions", "adm.roles", "hard"),
        ];

        for (perm, req, dep_type) in deps {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO permission_dependencies \
                 (permission_name, required_permission_name, dependency_type, created_at) \
                 VALUES (?, ?, ?, ?)",
                [
                    (*perm).into(),
                    (*req).into(),
                    (*dep_type).into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        tracing::info!("migration_028::rbac_scope_model applied");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Remove seeded permission dependencies (by permission_name values from this migration)
        db.execute_unprepared(
            "DELETE FROM permission_dependencies WHERE permission_name IN \
             ('ot.create','ot.edit','ot.approve','ot.close','ot.reopen',\
              'di.create','di.create.own','di.review','di.approve','di.convert',\
              'eq.manage','eq.delete','pm.manage','pm.approve',\
              'inv.manage','inv.order','inv.adjust',\
              'adm.roles','adm.permissions')",
        )
        .await?;

        // Remove seeded role templates
        db.execute_unprepared("DELETE FROM role_templates WHERE is_system = 1")
            .await?;

        // Drop new tables
        db.execute_unprepared("DROP TABLE IF EXISTS delegated_admin_policies")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS role_templates")
            .await?;

        // Drop augmentation indexes
        db.execute_unprepared("DROP INDEX IF EXISTS uidx_usa_user_role_scope")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_usa_role")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_usa_scope")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uidx_pd_pair")
            .await?;

        // Remove seeded system roles (only the 5 added by this migration)
        db.execute_unprepared(
            "DELETE FROM roles WHERE name IN \
             ('Superadmin','Maintenance Supervisor','Maintenance Technician',\
              'Planner/Scheduler','Read Only Observer') \
             AND role_type = 'system'",
        )
        .await?;

        // Note: emergency columns on user_scope_assignments are left in place.
        // SQLite does not support DROP COLUMN reliably in older versions, and
        // the columns are nullable/defaulted — harmless if present after rollback.

        Ok(())
    }
}

/// Checks whether a column exists on a table using `PRAGMA table_info`.
/// Returns `false` if the table doesn't exist or the column is not found.
async fn column_exists(
    db: &impl ConnectionTrait,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let sql = format!("PRAGMA table_info('{table}')");
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    for row in rows {
        let col_name: String = row.try_get("", "name").unwrap_or_default();
        if col_name == column {
            return Ok(true);
        }
    }
    Ok(false)
}
