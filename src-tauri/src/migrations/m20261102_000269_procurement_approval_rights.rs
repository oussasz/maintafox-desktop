//! Migration 269 — Purchase approval rights (L3-03)
//!
//! Requesting, approving and receiving purchases are separated:
//! - `inv.procure` creates and manages requisitions and purchase orders (unchanged name);
//! - `inv.approve` (new) approves or rejects them, within the role's approval limit;
//! - `inv.receive` (new) records goods receipts.
//!
//! This migration:
//! - stores the two permissions (`INSERT OR IGNORE`, metadata from the generated catalog) and their
//!   hard dependency on `inv.view`;
//! - grants `inv.receive` to every role that holds `inv.procure` today, so that whoever received
//!   goods keeps doing so;
//! - grants `inv.approve` only to the Administrator / Superadmin roles and to the manager system
//!   roles (Supervisor, Maintenance Supervisor: their default is every operational right). Every
//!   other role holding `inv.procure` (buyers, planners, custom roles) loses approval, which an
//!   administrator re-grants explicitly where wanted;
//! - seeds the Administrator's visible, editable unlimited approval limit per document type, since
//!   approval limits now fail closed (`inventory::approvals`). On a new database the Administrator
//!   role does not exist yet; the startup seeder seeds the same rows after creating it.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::rbac::permissions::{CATALOG, INV_APPROVE, INV_PROCURE, INV_RECEIVE, INV_VIEW};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261102_000269_procurement_approval_rights"
    }
}

/// Roles that receive `inv.approve`: the administrator roles and the manager system roles.
pub const APPROVER_ROLES: &[&str] = &["Administrator", "Superadmin", "Supervisor", "Maintenance Supervisor"];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        for name in [INV_APPROVE, INV_RECEIVE] {
            let meta = CATALOG
                .iter()
                .find(|m| m.name == name)
                .ok_or_else(|| DbErr::Custom(format!("{name} missing from the permission catalog")))?;
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
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO permission_dependencies
                       (permission_name, required_permission_name, dependency_type, created_at)
                   VALUES (?, ?, 'hard', ?)",
                [name.into(), INV_VIEW.into(), now.clone().into()],
            ))
            .await?;
        }
        // Approving goes through the procurement commands, which need inv.procure first: an
        // approver role without it would be a dead role, so the role editor refuses it.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permission_dependencies
                   (permission_name, required_permission_name, dependency_type, created_at)
               VALUES (?, ?, 'hard', ?)",
            [INV_APPROVE.into(), INV_PROCURE.into(), now.clone().into()],
        ))
        .await?;

        let receivers = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                   SELECT rp.role_id, pr.id, ?
                   FROM role_permissions rp
                   JOIN permissions pp ON pp.id = rp.permission_id AND pp.name = ?
                   JOIN roles r ON r.id = rp.role_id AND r.deleted_at IS NULL
                   CROSS JOIN permissions pr
                   WHERE pr.name = ?",
                [now.clone().into(), INV_PROCURE.into(), INV_RECEIVE.into()],
            ))
            .await?
            .rows_affected();

        let placeholders = vec!["?"; APPROVER_ROLES.len()].join(", ");
        let mut values: Vec<sea_orm::Value> = vec![now.into(), INV_APPROVE.into()];
        values.extend(APPROVER_ROLES.iter().map(|r| (*r).into()));
        let approvers = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
                       SELECT r.id, p.id, ?
                       FROM roles r CROSS JOIN permissions p
                       WHERE p.name = ? AND r.deleted_at IS NULL AND r.name IN ({placeholders})"
                ),
                values,
            ))
            .await?
            .rows_affected();

        let limits = crate::inventory::approvals::seed_administrator_unlimited_limits(db)
            .await
            .map_err(|e| DbErr::Custom(format!("seed administrator approval limits: {e}")))?;

        tracing::info!(
            receive_grants = receivers,
            approve_grants = approvers,
            administrator_limits = limits,
            "migration_269::procurement_approval_rights"
        );
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentional no-op: withdrawing rights or limits must be an explicit administrator decision.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::DatabaseConnection;

    async fn role_has(db: &DatabaseConnection, role: &str, permission: &str) -> bool {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT COUNT(*) AS n FROM role_permissions rp
               JOIN roles r ON r.id = rp.role_id
               JOIN permissions p ON p.id = rp.permission_id
               WHERE r.name = ? AND p.name = ?",
            [role.into(), permission.into()],
        ))
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "n").ok())
        .unwrap_or(0)
            > 0
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap()
            .and_then(|row| row.try_get_by_index::<i64>(0).ok())
            .unwrap_or(0)
    }

    async fn custom_role(db: &DatabaseConnection, name: &str, permissions: &[&str]) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at)
               VALUES (?, ?, 'test', 0, 'custom', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [format!("m269-{name}").into(), name.into()],
        ))
        .await
        .unwrap();
        for permission in permissions {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO role_permissions (role_id, permission_id, granted_at)
                   SELECT r.id, p.id, '2026-01-01T00:00:00Z' FROM roles r, permissions p
                   WHERE r.name = ? AND p.name = ?",
                [name.into(), (*permission).into()],
            ))
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn seeded_roles_separate_request_approval_and_receipt() {
        let db = crate::db::test_db::seeded().await;
        for role in APPROVER_ROLES {
            assert!(role_has(&db, role, INV_APPROVE).await, "{role} approves purchases");
            assert!(role_has(&db, role, INV_RECEIVE).await, "{role} receives goods");
        }
        // The planner creates purchases and receives goods, but does not approve them.
        assert!(role_has(&db, "Planner/Scheduler", INV_PROCURE).await);
        assert!(role_has(&db, "Planner/Scheduler", INV_RECEIVE).await);
        assert!(!role_has(&db, "Planner/Scheduler", INV_APPROVE).await);
        for role in [
            "Operator",
            "Readonly",
            "Maintenance Technician",
            "Read Only Observer",
            "Finance Manager",
        ] {
            assert!(
                !role_has(&db, role, INV_APPROVE).await,
                "{role} must not approve purchases"
            );
        }
        let deps = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM permission_dependencies
                  WHERE permission_name IN (?, ?) AND required_permission_name = ? AND dependency_type = 'hard'",
                [INV_APPROVE.into(), INV_RECEIVE.into(), INV_VIEW.into()],
            ))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "n").ok());
        assert_eq!(deps, Some(2));
        let approve_needs_procure = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM permission_dependencies
                  WHERE permission_name = ? AND required_permission_name = ? AND dependency_type = 'hard'",
                [INV_APPROVE.into(), INV_PROCURE.into()],
            ))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "n").ok());
        assert_eq!(approve_needs_procure, Some(1), "no dead approver role");
        // Every seeded holder of the approval right also holds the purchasing right.
        for role in APPROVER_ROLES {
            assert!(role_has(&db, role, INV_PROCURE).await, "{role} holds inv.procure");
        }
    }

    #[tokio::test]
    async fn migration_grants_receipt_to_buyers_and_approval_to_managers_only() {
        let db = crate::db::test_db::seeded().await;
        custom_role(&db, "Acheteur test", &[INV_VIEW, INV_PROCURE]).await;
        custom_role(&db, "Magasinier test", &[INV_VIEW]).await;
        // As before this version: nobody holds the two new rights yet.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"DELETE FROM role_permissions
               WHERE permission_id IN (SELECT id FROM permissions WHERE name IN (?, ?))",
            [INV_APPROVE.into(), INV_RECEIVE.into()],
        ))
        .await
        .unwrap();

        Migration.up(&SchemaManager::new(&db)).await.unwrap();

        assert!(
            role_has(&db, "Acheteur test", INV_RECEIVE).await,
            "a buyer keeps receiving"
        );
        assert!(
            !role_has(&db, "Acheteur test", INV_APPROVE).await,
            "a buyer does not approve"
        );
        assert!(
            !role_has(&db, "Magasinier test", INV_RECEIVE).await,
            "no purchasing right, nothing added"
        );
        assert!(role_has(&db, "Planner/Scheduler", INV_RECEIVE).await);
        assert!(!role_has(&db, "Planner/Scheduler", INV_APPROVE).await);
        assert!(role_has(&db, "Supervisor", INV_APPROVE).await);
        assert!(role_has(&db, "Supervisor", INV_RECEIVE).await);
    }

    #[tokio::test]
    async fn exactly_one_unlimited_administrator_limit_per_document_type_never_recreated() {
        let db = crate::db::test_db::seeded().await;
        let seeded = "SELECT COUNT(*) FROM procurement_approval_limits l JOIN roles r ON r.id = l.role_id
                      WHERE r.name = 'Administrator' AND l.max_amount IS NULL AND l.is_active = 1";
        assert_eq!(scalar(&db, seeded).await, 2, "seeded by the startup seeder");
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) FROM procurement_approval_limits").await,
            2,
            "no other limit is invented"
        );

        // Running again adds nothing.
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM procurement_approval_limits").await, 2);

        // The administrator capped one: it is never reset nor duplicated.
        db.execute_unprepared(
            "UPDATE procurement_approval_limits SET max_amount = 1000, row_version = row_version + 1
              WHERE document_type = 'PURCHASE_ORDER'",
        )
        .await
        .unwrap();
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        crate::db::seeder::seed_system_data(&db).await.unwrap();
        assert_eq!(scalar(&db, "SELECT COUNT(*) FROM procurement_approval_limits").await, 2);
        assert_eq!(
            scalar(
                &db,
                "SELECT CAST(max_amount AS INTEGER) FROM procurement_approval_limits WHERE document_type = 'PURCHASE_ORDER'"
            )
            .await,
            1000
        );
    }

    #[tokio::test]
    async fn existing_database_gets_the_administrator_limit_from_the_migration() {
        let db = crate::db::test_db::seeded().await;
        db.execute_unprepared("DELETE FROM procurement_approval_limits")
            .await
            .unwrap();
        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        let limits = crate::inventory::approvals::list_limits(&db).await.unwrap();
        assert_eq!(limits.len(), 2);
        assert!(limits
            .iter()
            .all(|l| l.role_name == "Administrator" && l.max_amount.is_none() && l.is_seeded_default));
    }
}
