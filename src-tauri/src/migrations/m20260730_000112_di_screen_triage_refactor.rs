//! Replaces author-driven `di.submit` / `di.submit.own` with supervisor `di.screen` (triage).
//! Removes deprecated permissions and grants `di.screen` to roles that already have `di.review`.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260730_000112_di_screen_triage_refactor"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        // 1) New permission: screen / triage incoming submitted DIs
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES (
                 'di.screen',
                 'Triage incoming intervention requests (submitted → review queue)',
                 'intervention',
                 0,
                 0,
                 1,
                 ?
               )",
            [now.clone().into()],
        ))
        .await?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permission_dependencies \
             (permission_name, required_permission_name, dependency_type, created_at) \
             VALUES ('di.screen', 'di.view', 'hard', ?)",
            [now.clone().into()],
        ))
        .await?;

        // 2) Grant di.screen to every role that already has di.review (supervisor / planner pattern)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
               SELECT rp.role_id, p_screen.id, ?
               FROM role_permissions rp
               JOIN permissions p_rev ON p_rev.id = rp.permission_id AND p_rev.name = 'di.review'
               CROSS JOIN permissions p_screen ON p_screen.name = 'di.screen'",
            [now.clone().into()],
        ))
        .await?;

        // 2b) Full-admin roles: ensure di.screen (catalog insert may land after role snapshot)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
               SELECT r.id, p.id, ?
               FROM roles r
               CROSS JOIN permissions p
               WHERE p.name = 'di.screen'
                 AND r.name IN ('Administrator', 'Superadmin', 'Maintenance Supervisor')",
            [now.clone().into()],
        ))
        .await?;

        // 3) Remove deprecated author "submit" permissions from role assignments, then delete rows
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM role_permissions WHERE permission_id IN \
             (SELECT id FROM permissions WHERE name IN ('di.submit', 'di.submit.own'))"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM permission_dependencies WHERE permission_name IN ('di.submit', 'di.submit.own')"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM permissions WHERE name IN ('di.submit', 'di.submit.own')"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
