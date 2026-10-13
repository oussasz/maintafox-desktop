//! Migration 151 — Finance approval rights (Phase 1.5)
//!
//! `fin.approve` is now enforced (budget version → approved, variance review → accepted/closed).
//! Approval belongs to Administrators and Finance Managers only, so:
//! - the grant that the "all operational permissions" default gave the Supervisor and
//!   Maintenance Supervisor system roles is removed (the seeder no longer grants it either:
//!   `fin.approve` is in `SUPERVISOR_EXCLUSIONS`);
//! - the `fin.approve → fin.view` hard dependency is recorded.
//!
//! The Finance Manager role itself is created by the seeder (`seed_system_roles`), which runs at
//! every startup after migrations.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261010_000151_finance_approval_rights"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permission_dependencies
                   (permission_name, required_permission_name, dependency_type, created_at)
               VALUES ('fin.approve', 'fin.view', 'hard', ?)",
            [now.into()],
        ))
        .await?;

        let removed = db
            .execute_unprepared(
                r"DELETE FROM role_permissions
                   WHERE permission_id IN (SELECT id FROM permissions WHERE name = 'fin.approve')
                     AND role_id IN (
                         SELECT id FROM roles WHERE name IN ('Supervisor', 'Maintenance Supervisor')
                     )",
            )
            .await?
            .rows_affected();

        tracing::info!(
            removed_supervisor_grants = removed,
            "migration_151::finance_approval_rights"
        );
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Intentional no-op: re-granting approval rights must be an explicit administrator decision.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

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

    async fn seeded_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        crate::db::seeder::seed_system_data(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn only_administrator_and_finance_manager_hold_fin_approve() {
        let db = seeded_db().await;
        assert!(role_has(&db, "Administrator", "fin.approve").await);
        assert!(role_has(&db, "Finance Manager", "fin.approve").await);
        assert!(role_has(&db, "Finance Manager", "fin.view").await, "hard dependency");
        for role in [
            "Supervisor",
            "Maintenance Supervisor",
            "Operator",
            "Readonly",
            "Planner/Scheduler",
        ] {
            assert!(
                !role_has(&db, role, "fin.approve").await,
                "{role} must not approve budgets"
            );
        }
        // Supervisors keep budget authoring.
        assert!(role_has(&db, "Supervisor", "fin.budget").await);
    }

    #[tokio::test]
    async fn migration_removes_existing_supervisor_approval_grants() {
        let db = seeded_db().await;
        db.execute_unprepared(
            r"INSERT INTO role_permissions (role_id, permission_id, granted_at)
               SELECT r.id, p.id, '2026-01-01T00:00:00Z' FROM roles r, permissions p
               WHERE r.name = 'Supervisor' AND p.name = 'fin.approve'",
        )
        .await
        .unwrap();
        assert!(role_has(&db, "Supervisor", "fin.approve").await);

        Migration.up(&SchemaManager::new(&db)).await.unwrap();
        assert!(!role_has(&db, "Supervisor", "fin.approve").await);
        assert!(
            role_has(&db, "Administrator", "fin.approve").await,
            "other roles untouched"
        );
    }
}
