//! Migration 037 — Observability permissions seed (SP07-F04)
//!
//! Idempotent `INSERT OR IGNORE` for log.*, arc.* (including `arc.export`), `adm.audit`,
//! and `adm.settings`, plus `permission_dependencies` for the observability domain.
//!
//! Most `log.*` / `arc.view` / `arc.restore` / `arc.purge` rows already exist from
//! migration 029 (`permission_catalog`); this migration repeats them safely and adds
//! rows that were missing (`arc.export`, `adm.audit`, `adm.settings`).
//!
//! Prerequisites: migrations 002 (permissions, permission_dependencies), 029 (catalog).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261201_000037_observability_permissions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        let permissions: &[(&str, &str, &str, i32, i32)] = &[
            // Activity log
            ("log.view", "View activity feed events", "log", 0, 0),
            ("log.export", "Export audit log", "log", 1, 0),
            ("log.admin", "Manage activity feed settings", "log", 1, 1),
            // Audit journal access
            (
                "adm.audit",
                "Access full immutable audit journal, review security events",
                "adm",
                1,
                0,
            ),
            // Archive
            ("arc.view", "Browse archived records", "arc", 0, 0),
            ("arc.restore", "Restore eligible archived records", "arc", 1, 1),
            ("arc.export", "Export archived record payloads", "arc", 0, 0),
            ("arc.purge", "Purge records past retention policy", "arc", 1, 1),
            // Notification / system admin (PRD §6.18)
            (
                "adm.settings",
                "Manage system settings, connection profiles, and admin policies",
                "adm",
                1,
                1,
            ),
        ];

        for (name, description, category, is_dangerous, requires_step_up) in permissions {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permissions
                       (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
                   VALUES (?, ?, ?, ?, ?, 1, ?)",
                [
                    (*name).into(),
                    (*description).into(),
                    (*category).into(),
                    (*is_dangerous).into(),
                    (*requires_step_up).into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        let deps: &[(&str, &str, &str)] = &[
            ("log.export", "log.view", "hard"),
            ("log.admin", "log.view", "hard"),
            ("arc.restore", "arc.view", "hard"),
            ("arc.export", "arc.view", "hard"),
            ("arc.purge", "arc.restore", "hard"),
            ("adm.audit", "log.view", "warn"),
        ];

        for (perm, req, dep_type) in deps {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permission_dependencies
                       (permission_name, required_permission_name, dependency_type, created_at)
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

        tracing::info!("migration_037::observability_permissions_seeded");
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentional no-op: removing permissions could break role_permissions FKs.
        Ok(())
    }
}
