//! Migration 137 — Normalize RBAC permissions to the canonical catalog.
//!
//! Ensures all `CATALOG` permissions exist with current metadata, copies
//! `role_permissions` grants from alias names onto canonical names (without
//! deleting alias rows), rebuilds `permission_dependencies` from `DEPENDENCIES`,
//! and backfills `integrity.repair` / `sync.*` for Administrator and Superadmin.
//!
//! Prerequisites: migration 029 (permission catalog), 002 (RBAC tables).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::rbac::permissions::{
    ALIAS_MAP, CATALOG, DEPENDENCIES, INTEGRITY_REPAIR, PM_CREATE, PM_DELETE, PM_EDIT, SYNC_MANAGE, SYNC_REPAIR,
    SYNC_REPLAY, SYNC_RESOLVE, SYNC_VIEW,
};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260729_000137_rbac_permission_normalization"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Ensure every canonical catalog permission exists.
        for meta in CATALOG {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permissions
                       (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
                   VALUES (?, ?, ?, ?, ?, 1, ?)",
                [
                    meta.name.into(),
                    meta.description.into(),
                    meta.category.into(),
                    i32::from(meta.is_dangerous).into(),
                    i32::from(meta.requires_step_up).into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        // 2. Reconcile metadata for existing system permission rows.
        for meta in CATALOG {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"UPDATE permissions
                   SET description = ?, category = ?, is_dangerous = ?, requires_step_up = ?
                 WHERE name = ? AND is_system = 1",
                [
                    meta.description.into(),
                    meta.category.into(),
                    i32::from(meta.is_dangerous).into(),
                    i32::from(meta.requires_step_up).into(),
                    meta.name.into(),
                ],
            ))
            .await?;
        }

        // 3–4. Copy alias grants onto canonical names; expand pm.manage.
        // Alias permission rows are preserved for history — do not delete them.
        for (alias, canonical) in ALIAS_MAP {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                   SELECT rp.role_id, canon.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                   FROM role_permissions rp
                   INNER JOIN permissions alias_p
                     ON alias_p.id = rp.permission_id AND alias_p.name = ?
                   INNER JOIN permissions canon
                     ON canon.name = ?",
                [(*alias).into(), (*canonical).into()],
            ))
            .await?;

            if *alias == "pm.manage" {
                for expand in [PM_CREATE, PM_EDIT, PM_DELETE] {
                    db.execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                           SELECT rp.role_id, expand_p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                           FROM role_permissions rp
                           INNER JOIN permissions alias_p
                             ON alias_p.id = rp.permission_id AND alias_p.name = ?
                           INNER JOIN permissions expand_p
                             ON expand_p.name = ?",
                        [(*alias).into(), expand.into()],
                    ))
                    .await?;
                }
            }
        }

        // 4b. WO lifecycle split: ot.edit holders gain ot.close + ot.approve;
        // ot.admin holders gain ot.reopen (previous reopen gate).
        for (source, targets) in [
            (
                crate::rbac::permissions::OT_EDIT,
                &[crate::rbac::permissions::OT_CLOSE, crate::rbac::permissions::OT_APPROVE][..],
            ),
            (
                crate::rbac::permissions::OT_ADMIN,
                &[crate::rbac::permissions::OT_REOPEN][..],
            ),
        ] {
            for target in targets {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                       SELECT rp.role_id, target_p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                       FROM role_permissions rp
                       INNER JOIN permissions source_p
                         ON source_p.id = rp.permission_id AND source_p.name = ?
                       INNER JOIN permissions target_p
                         ON target_p.name = ?",
                    [source.into(), (*target).into()],
                ))
                .await?;
            }
        }

        // 5. Rebuild permission_dependencies for catalog permission names.
        for meta in CATALOG {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"DELETE FROM permission_dependencies WHERE permission_name = ?",
                [meta.name.into()],
            ))
            .await?;
        }
        for dep in DEPENDENCIES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permission_dependencies
                       (permission_name, required_permission_name, dependency_type, created_at)
                   VALUES (?, ?, ?, ?)",
                [
                    dep.permission.into(),
                    dep.requires.into(),
                    dep.dep_type.into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }

        // 7. Backfill integrity.repair and sync.* for admin roles if missing.
        let admin_grants: &[&str] = &[
            INTEGRITY_REPAIR,
            SYNC_VIEW,
            SYNC_MANAGE,
            SYNC_RESOLVE,
            SYNC_REPLAY,
            SYNC_REPAIR,
        ];
        for perm in admin_grants {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                   SELECT r.id, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                   FROM roles r
                   CROSS JOIN permissions p
                   WHERE r.deleted_at IS NULL
                     AND r.name IN ('Administrator', 'Superadmin')
                     AND p.name = ?",
                [(*perm).into()],
            ))
            .await?;
        }

        tracing::info!(
            catalog = CATALOG.len(),
            aliases = ALIAS_MAP.len(),
            deps = DEPENDENCIES.len(),
            "migration_137::rbac_permission_normalization"
        );

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive grant/metadata migration — rollback would drop live role grants.
        Ok(())
    }
}
