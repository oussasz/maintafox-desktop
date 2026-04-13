//! Migration 032 — RBAC Hardening Policy Knobs
//!
//! Phase 2 - Sub-phase 06 - File 04.
//!
//! Extends the `rbac_settings` table (created in migration 031) with:
//! - `is_sensitive` column to mark keys whose values should be masked in
//!   non-admin views (e.g. retention policies, security thresholds).
//! - 5 RBAC governance policy knobs consumed by the resolver, emergency
//!   elevation enforcement, step-up reauthentication, and the audit journal.
//!
//! Prerequisites: migration 031 (rbac_settings table with lockout rows).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260413_000032_rbac_hardening"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Add is_sensitive column ───────────────────────────────────
        // SQLite ALTER TABLE ADD COLUMN is supported since 3.2.0.
        // DEFAULT 0 ensures existing rows (lockout_max_attempts, etc.) get
        // the correct value without a backfill step.
        db.execute_unprepared(
            "ALTER TABLE rbac_settings ADD COLUMN is_sensitive INTEGER NOT NULL DEFAULT 0",
        )
        .await?;

        // ── 2. Seed RBAC hardening policy knobs ─────────────────────────
        // INSERT OR IGNORE makes this idempotent if re-run.
        db.execute_unprepared(
            "INSERT OR IGNORE INTO rbac_settings (key, value, description, is_sensitive) VALUES
                ('emergency_max_minutes',          '480',  'Maximum minutes for emergency elevation grants (default 8h)', 0),
                ('session_idle_timeout_min',        '60',  'Minutes of inactivity before session is considered idle', 0),
                ('require_step_up_on_role_change',  '1',   '1 = all role mutations require step-up reauthentication', 0),
                ('admin_event_retention_days',      '730', 'Days to retain admin_change_events (min 365 for compliance)', 1),
                ('deny_all_fallback',               '1',   '1 = users with no active scope assignments get no permissions', 0)",
        )
        .await?;

        tracing::info!("migration_032::rbac_hardening — is_sensitive column added, 5 policy knobs seeded");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Remove seeded rows (lockout rows from 031 are untouched).
        db.execute_unprepared(
            "DELETE FROM rbac_settings WHERE key IN (
                'emergency_max_minutes',
                'session_idle_timeout_min',
                'require_step_up_on_role_change',
                'admin_event_retention_days',
                'deny_all_fallback'
            )",
        )
        .await?;

        // SQLite does not support DROP COLUMN; is_sensitive will remain
        // but is harmless (DEFAULT 0, not referenced after rollback).

        Ok(())
    }
}
