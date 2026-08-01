//! Vendor admin console RBAC (Phase 4 control plane).
//!
//! Granular permissions for the dedicated vendor console surface; `console.view`
//! gates shell access. Dangerous actions use `requires_step_up` per PRD section 16.5.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260511_000066_vendor_console_permissions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        let rows: &[(&str, &str, &str, i32, i32)] = &[
            (
                "console.view",
                "Access vendor control-plane console shell",
                "vendor_console",
                0,
                0,
            ),
            (
                "customer.manage",
                "Manage tenant customer records in vendor console",
                "vendor_console",
                1,
                0,
            ),
            (
                "entitlement.manage",
                "Change entitlements, suspension, and license posture",
                "vendor_console",
                1,
                1,
            ),
            (
                "sync.operate",
                "Vendor-console sync operations (queues, repair windows)",
                "vendor_console",
                1,
                1,
            ),
            (
                "rollout.manage",
                "Publish and roll back control-plane update rollouts",
                "vendor_console",
                1,
                1,
            ),
            (
                "platform.observe",
                "View platform health, SLOs, and integration status",
                "vendor_console",
                0,
                0,
            ),
            (
                "audit.view",
                "Read vendor-scoped audit and evidence trails",
                "vendor_console",
                0,
                0,
            ),
        ];

        for (name, description, category, is_dangerous, requires_step_up) in rows {
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

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
