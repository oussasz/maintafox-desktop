//! Migration 209 — notification categories, subscriptions and escalation defaults (data).
//!
//! **Categories (SYSTEM, fixed by Maintafox).** Every category gets the module it belongs to
//! (`module_code`). New categories with a generator in this version: `inspection_round_missed`,
//! `ptw_expiring`, `sync_conflict`, and `planning.schedule` (planning already raised it, but no
//! category or rule existed outside the tests, so those notifications were dropped). The two
//! categories without any generator (`integration_failure`, `support_response`) are marked not
//! offered: hidden from the preferences and the rules. `stock_critical` becomes what its generator
//! reports — an article at or below its reorder point — a warning the user may silence, not a
//! critical alarm.
//!
//! **Subscriptions (COMPANY, editable in Données de référence → Notifications).** Each rule gets
//! `audiences_json` from its previous single routing mode (same recipients as before), with the
//! audiences of the categories that never reached anyone (`stock_critical` routed to an unnamed
//! role) or had no generator before. Rules a plant already changed keep their routing.
//!
//! **Escalation defaults (COMPANY).** The three shipped steps targeted roles named
//! `entity_manager`, `supervisor` and `system_admin`, which no installation has: an unacknowledged
//! critical notification escalated to nobody. They now target the supervisor of the person
//! notified (after 30 min), the responsibles of the record's site / org node (60 min) and the
//! people allowed to approve work orders (`ot.approve`, 120 min). Steps a plant changed are kept.
//!
//! Every device seeds these rows itself: new rows carry the seeded identity of their key
//! (`sync::seeded_identity`), updates touch only rows still holding the shipped values.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261016_000209_notification_subscriptions"
    }
}

/// `(code, label, module, severity, requires_ack, user_configurable)` of the new categories.
const NEW_CATEGORIES: &[(&str, &str, &str, &str, i64, i64)] = &[
    (
        "inspection_round_missed",
        "Inspection round missed",
        "ins",
        "warning",
        0,
        1,
    ),
    ("ptw_expiring", "Work permit expiring", "ptw", "warning", 0, 1),
    (
        "sync_conflict",
        "Synchronisation conflict to review",
        "sync",
        "warning",
        0,
        1,
    ),
    ("planning.schedule", "Planning schedule", "plan", "info", 0, 1),
];

/// `(category, routing_mode, requires_ack, dedupe minutes, lead minutes)` of the new rules.
const NEW_RULES: &[(&str, &str, i64, i64, Option<i64>)] = &[
    ("inspection_round_missed", "assignee", 0, 60, None),
    ("ptw_expiring", "assignee", 0, 60, Some(120)),
    ("sync_conflict", "manual", 0, 60, None),
    ("planning.schedule", "team", 0, 120, None),
];

/// Module of every shipped category.
const CATEGORY_MODULES: &[(&str, &str)] = &[
    ("budget_control_alert", "fin"),
    ("cert_expiry", "per"),
    ("daily_digest", "system"),
    ("di_approved", "di"),
    ("di_closed", "di"),
    ("di_converted", "di"),
    ("di_deferred", "di"),
    ("di_pending_review", "di"),
    ("di_returned", "di"),
    ("di_sla_breach", "di"),
    ("integration_failure", "system"),
    ("personnel_critical_block", "per"),
    ("pm_deferred", "pm"),
    ("pm_due", "pm"),
    ("pm_follow_up_created", "pm"),
    ("pm_missed", "pm"),
    ("ptw_critical", "ptw"),
    ("stock_critical", "inv"),
    ("support_response", "system"),
    ("wo_assigned", "ot"),
    ("wo_overdue", "ot"),
];

/// Audiences of the categories whose recipients differ from their old single routing mode.
const AUDIENCE_OVERRIDES: &[(&str, &str)] = &[
    ("stock_critical", r#"["permission:inv.manage"]"#),
    ("wo_overdue", r#"["assignee","planner"]"#),
    ("pm_due", r#"["assignee","team"]"#),
    ("inspection_round_missed", r#"["assignee","team_lead"]"#),
    ("ptw_expiring", r#"["assignee","planner","entity_manager"]"#),
    ("ptw_critical", r#"["assignee","planner","entity_manager"]"#),
    ("sync_conflict", r#"["permission:sync.resolve"]"#),
    ("cert_expiry", r#"["entity_manager","supervisor"]"#),
    ("budget_control_alert", "[]"),
];

/// `(level, shipped role name, new target type, new reference)`.
const STEP_FIXES: &[(i64, &str, &str, Option<&str>)] = &[
    (1, "entity_manager", "supervisor", None),
    (2, "supervisor", "entity_manager", None),
    (3, "system_admin", "permission", Some("ot.approve")),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        let db = &txn;

        for (code, label, module, severity, ack, configurable) in NEW_CATEGORIES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO notification_categories \
                 (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
                  module_code, is_offered) VALUES (?, ?, ?, ?, ?, ?, ?, 1)",
                [
                    seeded_entity_sync_id("notification_categories", code).into(),
                    (*code).into(),
                    (*label).into(),
                    (*severity).into(),
                    (*ack).into(),
                    (*configurable).into(),
                    (*module).into(),
                ],
            ))
            .await?;
        }
        for (code, module) in CATEGORY_MODULES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notification_categories SET module_code = ? WHERE code = ? AND module_code IS NULL",
                [(*module).into(), (*code).into()],
            ))
            .await?;
        }
        db.execute_unprepared(
            "UPDATE notification_categories SET is_offered = 0 \
             WHERE code IN ('integration_failure', 'support_response')",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE notification_categories SET default_severity = 'warning', default_requires_ack = 0, \
             is_user_configurable = 1, label = 'Article at or below its reorder point' \
             WHERE code = 'stock_critical' AND default_severity = 'critical'",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE notification_rules SET requires_ack = 0, escalation_policy_id = NULL \
             WHERE category_code = 'stock_critical' AND routing_mode = 'role' AND audiences_json IS NULL",
        )
        .await?;

        for (category, mode, ack, dedupe, lead) in NEW_RULES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO notification_rules \
                 (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
                  escalation_policy_id, is_active, lead_time_minutes) \
                 SELECT ?, ?, ?, ?, ?, NULL, 1, ? \
                 WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
                [
                    seeded_entity_sync_id("notification_rules", category).into(),
                    (*category).into(),
                    (*mode).into(),
                    (*ack).into(),
                    (*dedupe).into(),
                    (*lead).into(),
                    (*category).into(),
                ],
            ))
            .await?;
        }
        for (category, audiences) in AUDIENCE_OVERRIDES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notification_rules SET audiences_json = ? WHERE category_code = ? AND audiences_json IS NULL",
                [(*audiences).into(), (*category).into()],
            ))
            .await?;
        }
        // Every other rule: its single routing mode becomes its one audience.
        db.execute_unprepared(
            "UPDATE notification_rules SET audiences_json = CASE routing_mode \
                WHEN 'assignee' THEN '[\"assignee\"]' \
                WHEN 'reviewer' THEN '[\"reviewer\"]' \
                WHEN 'submitter' THEN '[\"submitter\"]' \
                WHEN 'entity_manager' THEN '[\"entity_manager\"]' \
                WHEN 'team' THEN '[\"team\"]' \
                WHEN 'watcher' THEN '[\"watcher\"]' \
                ELSE '[]' END \
             WHERE audiences_json IS NULL",
        )
        .await?;

        for (level, old_role, new_type, new_ref) in STEP_FIXES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notification_escalation_steps SET route_to_type = ?, route_to_reference = ? \
                 WHERE level_no = ? AND route_to_type = 'role' AND route_to_reference = ? \
                   AND escalation_policy_id IN \
                       (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation')",
                [
                    (*new_type).into(),
                    new_ref.map(str::to_string).into(),
                    (*level).into(),
                    (*old_role).into(),
                ],
            ))
            .await?;
        }

        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (level, old_role, new_type, _) in STEP_FIXES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notification_escalation_steps SET route_to_type = 'role', route_to_reference = ? \
                 WHERE level_no = ? AND route_to_type = ?",
                [(*old_role).into(), (*level).into(), (*new_type).into()],
            ))
            .await?;
        }
        for (category, ..) in NEW_RULES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM notification_rules WHERE category_code = ?",
                [(*category).into()],
            ))
            .await?;
        }
        for (code, ..) in NEW_CATEGORIES {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM notification_categories WHERE code = ?",
                [(*code).into()],
            ))
            .await?;
        }
        Ok(())
    }
}
