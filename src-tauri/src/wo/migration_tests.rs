//! Supervisor verification tests for Phase 2 SP05 File 01 Sprint S1.
//!
//! V1 — Migration 022 applies cleanly; all tables created; work_order_stubs dropped.
//! V2 — Seed row counts: work_order_types=7, work_order_statuses=12, urgency_levels=5,
//!       delay_reason_codes=10.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    /// In-memory SQLite with all migrations applied.
    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations should apply cleanly (including 022)");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Migration 022 applies cleanly
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_migration_022_creates_work_orders_table() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='work_orders';"
                    .to_string(),
            ))
            .await
            .expect("query should succeed");

        assert!(
            exists.is_some(),
            "work_orders table must exist after migration 022"
        );
    }

    #[tokio::test]
    async fn v1_migration_022_creates_wo_state_transition_log() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='wo_state_transition_log';"
                    .to_string(),
            ))
            .await
            .expect("query should succeed");

        assert!(
            exists.is_some(),
            "wo_state_transition_log table must exist after migration 022"
        );
    }

    #[tokio::test]
    async fn v1_work_order_stubs_absent_after_migration() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='work_order_stubs';"
                    .to_string(),
            ))
            .await
            .expect("query should succeed");

        assert!(
            exists.is_none(),
            "work_order_stubs table must NOT exist after migration 022"
        );
    }

    #[tokio::test]
    async fn v1_work_orders_has_all_columns() {
        let db = setup().await;

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(work_orders);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info should succeed");

        let column_names: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap())
            .collect();

        let expected_columns = [
            "id", "code", "type_id", "status_id",
            "equipment_id", "component_id", "location_id",
            "requester_id", "source_di_id", "entity_id",
            "planner_id", "approver_id", "assigned_group_id", "primary_responsible_id",
            "urgency_id", "title", "description",
            "planned_start", "planned_end", "shift", "scheduled_at",
            "actual_start", "actual_end",
            "mechanically_completed_at", "technically_verified_at",
            "closed_at", "cancelled_at",
            "expected_duration_hours", "actual_duration_hours",
            "active_labor_hours", "total_waiting_hours", "downtime_hours",
            "labor_cost", "parts_cost", "service_cost", "total_cost",
            "recurrence_risk_level", "production_impact_id",
            "root_cause_summary", "corrective_action_summary", "verification_method",
            "notes", "cancel_reason", "row_version", "created_at", "updated_at",
        ];

        for col in &expected_columns {
            assert!(
                column_names.contains(&col.to_string()),
                "work_orders is missing expected column: '{col}'"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Seed row counts
    // ═══════════════════════════════════════════════════════════════════════

    async fn count_rows(db: &sea_orm::DatabaseConnection, table: &str) -> i64 {
        let sql = format!("SELECT COUNT(*) AS cnt FROM {table}");
        let row = db
            .query_one(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .expect("count query should succeed")
            .expect("count query should return a row");
        row.try_get::<i64>("", "cnt").unwrap()
    }

    #[tokio::test]
    async fn v2_work_order_types_has_7_rows() {
        let db = setup().await;
        assert_eq!(count_rows(&db, "work_order_types").await, 7);
    }

    #[tokio::test]
    async fn v2_work_order_statuses_has_12_rows() {
        let db = setup().await;
        assert_eq!(count_rows(&db, "work_order_statuses").await, 12);
    }

    #[tokio::test]
    async fn v2_urgency_levels_has_5_rows() {
        let db = setup().await;
        assert_eq!(count_rows(&db, "urgency_levels").await, 5);
    }

    #[tokio::test]
    async fn v2_delay_reason_codes_has_10_rows() {
        let db = setup().await;
        assert_eq!(count_rows(&db, "delay_reason_codes").await, 10);
    }
}
