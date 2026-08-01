use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260726_000108_trusted_devices_user_fingerprint_unique"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Rebuild `trusted_devices` to switch uniqueness from
        // `device_fingerprint` (global) to `(user_id, device_fingerprint)` (per-user).
        // This allows multiple local accounts on the same machine to bootstrap trust.
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS trusted_devices_v2 (
                id                 TEXT PRIMARY KEY,
                device_fingerprint TEXT NOT NULL,
                device_label       TEXT NULL,
                user_id            TEXT NOT NULL,
                trusted_at         TEXT NOT NULL,
                last_seen_at       TEXT NULL,
                is_revoked         INTEGER NOT NULL DEFAULT 0,
                revoked_at         TEXT NULL,
                revoked_reason     TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO trusted_devices_v2
                (id, device_fingerprint, device_label, user_id, trusted_at, last_seen_at, is_revoked, revoked_at, revoked_reason)
             SELECT
                id, device_fingerprint, device_label, user_id, trusted_at, last_seen_at, is_revoked, revoked_at, revoked_reason
             FROM trusted_devices",
        )
        .await?;

        db.execute_unprepared("DROP TABLE trusted_devices").await?;
        db.execute_unprepared("ALTER TABLE trusted_devices_v2 RENAME TO trusted_devices")
            .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_trusted_devices_user_fp
             ON trusted_devices(user_id, device_fingerprint)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Best-effort rollback to legacy shape (global unique fingerprint).
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS trusted_devices_legacy (
                id                 TEXT PRIMARY KEY,
                device_fingerprint TEXT NOT NULL UNIQUE,
                device_label       TEXT NULL,
                user_id            TEXT NOT NULL,
                trusted_at         TEXT NOT NULL,
                last_seen_at       TEXT NULL,
                is_revoked         INTEGER NOT NULL DEFAULT 0,
                revoked_at         TEXT NULL,
                revoked_reason     TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO trusted_devices_legacy
                (id, device_fingerprint, device_label, user_id, trusted_at, last_seen_at, is_revoked, revoked_at, revoked_reason)
             SELECT
                id, device_fingerprint, device_label, user_id, trusted_at, last_seen_at, is_revoked, revoked_at, revoked_reason
             FROM trusted_devices",
        )
        .await?;

        db.execute_unprepared("DROP TABLE trusted_devices").await?;
        db.execute_unprepared("ALTER TABLE trusted_devices_legacy RENAME TO trusted_devices")
            .await?;

        Ok(())
    }
}
