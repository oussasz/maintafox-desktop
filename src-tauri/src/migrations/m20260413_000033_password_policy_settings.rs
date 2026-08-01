//! Migration 033 — Password Policy Settings
//!
//! Phase 2 - Sub-phase 06 - File 04, Sprint S4 (GAP-08).
//!
//! Seeds 7 password policy rows into `rbac_settings` (created in migration 031).
//! These settings control password expiry, warning period, minimum length,
//! and complexity requirements. Consumed by `auth::password_policy::PasswordPolicy::load()`.
//!
//! Prerequisites: migration 031 (rbac_settings table).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260413_000033_password_policy_settings"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "INSERT OR IGNORE INTO rbac_settings (key, value, description, is_sensitive) VALUES
                ('password_max_age_days',      '90',  'Days before password expiry (0 = disabled)',            0),
                ('password_warn_days',         '14',  'Days before expiry to show warning',                   0),
                ('password_min_length',        '8',   'Minimum password length',                              0),
                ('password_require_uppercase', '1',   'Require at least one uppercase letter',                0),
                ('password_require_lowercase', '1',   'Require at least one lowercase letter',                0),
                ('password_require_digit',     '1',   'Require at least one digit',                           0),
                ('password_require_special',   '0',   'Require at least one special character (!@#$%^&*...)', 0)",
        )
        .await?;

        tracing::info!("migration_033::password_policy_settings — 7 password policy rows seeded");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "DELETE FROM rbac_settings WHERE key IN (
                'password_max_age_days',
                'password_warn_days',
                'password_min_length',
                'password_require_uppercase',
                'password_require_lowercase',
                'password_require_digit',
                'password_require_special'
            )",
        )
        .await?;

        Ok(())
    }
}
