//! Migration 193 — certification expiry notifications reach the person's managers.
//!
//! The `cert_expiry` notification rule (migration 034) routed to `assignee`, which only resolves
//! work orders, requests and PM occurrences: an expiring habilitation notified nobody. It now
//! routes to `entity_manager` — the users scoped to the person's organisation node — and the
//! expiry scan sends the node in the event payload. A rule a tenant already changed is kept.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000193_cert_expiry_routing"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE notification_rules SET routing_mode = 'entity_manager' \
                 WHERE category_code = 'cert_expiry' AND routing_mode = 'assignee'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE notification_rules SET routing_mode = 'assignee' \
                 WHERE category_code = 'cert_expiry' AND routing_mode = 'entity_manager'",
            )
            .await?;
        Ok(())
    }
}
