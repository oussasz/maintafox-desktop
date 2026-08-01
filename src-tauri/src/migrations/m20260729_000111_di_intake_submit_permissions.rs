//! DI intake: `di.submit.own` permission and clarified `di.submit` description.
//! Also seeds permission dependencies for intake actions.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260729_000111_di_intake_submit_permissions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        // Intake: send own submitted DIs to the validation queue (pair with di.submit).
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES ('di.submit.own', 'Send own draft DIs to the validation queue', 'intervention', 0, 0, 1, ?)",
            [now.clone().into()],
        ))
        .await?;

        // Clarify catalog 029 wording: di.submit is the formal "push to review" (any), not "create DI".
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE permissions SET description = 'Send submitted DIs to the validation queue (any submitter)' \
             WHERE name = 'di.submit'".to_string(),
        ))
        .await?;

        // Permission dependencies (INSERT OR IGNORE)
        let deps: &[(&str, &str, &str)] = &[
            ("di.submit", "di.view", "hard"),
            ("di.submit.own", "di.view", "hard"),
        ];
        for (perm, req, dep_type) in deps {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO permission_dependencies \
                 (permission_name, required_permission_name, dependency_type, created_at) \
                 VALUES (?, ?, ?, ?)",
                [(*perm).into(), (*req).into(), (*dep_type).into(), now.clone().into()],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
