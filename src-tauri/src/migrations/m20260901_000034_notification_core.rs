//! Migration 034 — Notification Core Schema
//!
//! Phase 2 - Sub-phase 07 - File 01.
//!
//! Creates notification event/routing/delivery/acknowledgement tables and seeds
//! baseline categories, escalation policy and default rules for the
//! Notification System (PRD §6.14).
//!
//! Prerequisites: migration 002 (user_accounts, roles).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260901_000034_notification_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_events (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                source_module    TEXT NOT NULL,
                source_record_id TEXT NULL,
                event_code       TEXT NOT NULL,
                category_code    TEXT NOT NULL,
                severity         TEXT NOT NULL DEFAULT 'info',
                occurred_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                dedupe_key       TEXT NULL,
                payload_json     TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ne_category ON notification_events(category_code)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ne_dedupe ON notification_events(dedupe_key)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_categories (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                code                 TEXT NOT NULL UNIQUE,
                label                TEXT NOT NULL,
                default_severity     TEXT NOT NULL DEFAULT 'info',
                default_requires_ack INTEGER NOT NULL DEFAULT 0,
                is_user_configurable INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_rules (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                category_code           TEXT NOT NULL REFERENCES notification_categories(code),
                routing_mode            TEXT NOT NULL DEFAULT 'assignee',
                requires_ack            INTEGER NOT NULL DEFAULT 0,
                dedupe_window_minutes   INTEGER NOT NULL DEFAULT 60,
                quiet_hours_policy_json TEXT NULL,
                escalation_policy_id    INTEGER NULL,
                is_active               INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_escalation_policies (
                id   INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_escalation_steps (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                escalation_policy_id INTEGER NOT NULL REFERENCES notification_escalation_policies(id),
                level_no             INTEGER NOT NULL,
                wait_minutes         INTEGER NOT NULL DEFAULT 60,
                route_to_type        TEXT NOT NULL DEFAULT 'role',
                route_to_reference   TEXT NULL,
                channel_set_json     TEXT NOT NULL DEFAULT '[\"in_app\",\"os\"]'
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notifications (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                notification_event_id INTEGER NOT NULL REFERENCES notification_events(id),
                recipient_user_id     INTEGER NULL REFERENCES user_accounts(id),
                recipient_role_id     INTEGER NULL REFERENCES roles(id),
                delivery_state        TEXT NOT NULL DEFAULT 'pending',
                title                 TEXT NOT NULL,
                body                  TEXT NULL,
                action_url            TEXT NULL,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                read_at               TEXT NULL,
                acknowledged_at       TEXT NULL,
                snoozed_until         TEXT NULL,
                closed_at             TEXT NULL,
                escalation_level      INTEGER NOT NULL DEFAULT 0
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_notif_user ON notifications(recipient_user_id, delivery_state)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_notif_event ON notifications(notification_event_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_deliveries (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                notification_id INTEGER NOT NULL REFERENCES notifications(id),
                channel         TEXT NOT NULL,
                attempt_no      INTEGER NOT NULL DEFAULT 1,
                delivery_status TEXT NOT NULL DEFAULT 'queued',
                attempted_at    TEXT NULL,
                delivered_at    TEXT NULL,
                failure_reason  TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_acknowledgements (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                notification_id      INTEGER NOT NULL REFERENCES notifications(id),
                acknowledged_by_id   INTEGER NOT NULL REFERENCES user_accounts(id),
                acknowledged_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                acknowledgement_note TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS notification_preferences (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id        INTEGER NOT NULL REFERENCES user_accounts(id),
                category_code  TEXT NOT NULL REFERENCES notification_categories(code),
                in_app_enabled INTEGER NOT NULL DEFAULT 1,
                os_enabled     INTEGER NOT NULL DEFAULT 1,
                email_enabled  INTEGER NOT NULL DEFAULT 0,
                sms_enabled    INTEGER NOT NULL DEFAULT 0,
                digest_mode    TEXT NOT NULL DEFAULT 'instant',
                muted_until    TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_np_user_cat ON notification_preferences(user_id, category_code)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO notification_categories
                (code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES
                ('di_pending_review',   'DI Pending Review',          'info',     0, 1),
                ('di_sla_breach',       'DI SLA Breach',              'warning',  1, 1),
                ('wo_assigned',         'WO Assigned',                'info',     0, 1),
                ('wo_overdue',          'WO Overdue',                 'warning',  0, 1),
                ('pm_due',              'PM Due Soon',                'info',     0, 1),
                ('pm_missed',           'PM Missed',                  'warning',  0, 1),
                ('stock_critical',      'Stock Critical',             'critical', 1, 0),
                ('cert_expiry',         'Certification Expiry',       'critical', 1, 0),
                ('ptw_critical',        'Work Permit Critical Event', 'critical', 1, 0),
                ('integration_failure', 'Integration Failure',        'error',    1, 0),
                ('support_response',    'Support Response Available', 'info',     0, 1)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_escalation_policies (name)
             SELECT 'Default Critical Escalation'
             WHERE NOT EXISTS (
               SELECT 1 FROM notification_escalation_policies
               WHERE name = 'Default Critical Escalation'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_escalation_steps
                (escalation_policy_id, level_no, wait_minutes, route_to_type, route_to_reference, channel_set_json)
             SELECT p.id, 1, 30,  'role', 'entity_manager', '[\"in_app\",\"os\"]'
               FROM notification_escalation_policies p
              WHERE p.name = 'Default Critical Escalation'
                AND NOT EXISTS (
                    SELECT 1 FROM notification_escalation_steps s
                    WHERE s.escalation_policy_id = p.id AND s.level_no = 1
                )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_escalation_steps
                (escalation_policy_id, level_no, wait_minutes, route_to_type, route_to_reference, channel_set_json)
             SELECT p.id, 2, 60,  'role', 'supervisor', '[\"in_app\",\"os\"]'
               FROM notification_escalation_policies p
              WHERE p.name = 'Default Critical Escalation'
                AND NOT EXISTS (
                    SELECT 1 FROM notification_escalation_steps s
                    WHERE s.escalation_policy_id = p.id AND s.level_no = 2
                )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_escalation_steps
                (escalation_policy_id, level_no, wait_minutes, route_to_type, route_to_reference, channel_set_json)
             SELECT p.id, 3, 120, 'role', 'system_admin', '[\"in_app\",\"os\"]'
               FROM notification_escalation_policies p
              WHERE p.name = 'Default Critical Escalation'
                AND NOT EXISTS (
                    SELECT 1 FROM notification_escalation_steps s
                    WHERE s.escalation_policy_id = p.id AND s.level_no = 3
                )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'di_pending_review', 'reviewer', 0, 120, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'di_pending_review'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'di_sla_breach', 'entity_manager', 1, 60,
                (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'di_sla_breach'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'wo_assigned', 'assignee', 0, 60, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'wo_assigned'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'wo_overdue', 'assignee', 0, 240, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'wo_overdue'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'pm_due', 'assignee', 0, 720, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'pm_due'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'pm_missed', 'entity_manager', 0, 240, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'pm_missed'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'stock_critical', 'role', 1, 60,
                (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'stock_critical'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'cert_expiry', 'assignee', 1, 720,
                (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'cert_expiry'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'ptw_critical', 'entity_manager', 1, 30,
                (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'ptw_critical'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'integration_failure', 'manual', 1, 60,
                (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'integration_failure'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'support_response', 'assignee', 0, 60, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'support_response'
             )",
        )
        .await?;

        tracing::info!("migration_034::notification_core applied");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS uidx_np_user_cat").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_notif_event").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_notif_user").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ne_dedupe").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ne_category").await?;

        db.execute_unprepared("DROP TABLE IF EXISTS notification_preferences").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_acknowledgements").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_deliveries").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notifications").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_escalation_steps").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_rules").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_escalation_policies").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_categories").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS notification_events").await?;

        Ok(())
    }
}
