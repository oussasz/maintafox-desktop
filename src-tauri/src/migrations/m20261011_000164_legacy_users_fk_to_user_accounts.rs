//! Migration 164 — Point the last foreign keys on the legacy `users` stub to `user_accounts`.
//!
//! `pm_executions.completed_by_id`, `schedule_break_ins.created_by_id` / `approved_by_user_id` and
//! `schedule_change_log.actor_id` hold user account ids (the session user), but their declared
//! foreign key still names `users`, an id-only stub that no code fills (migration 140). With
//! foreign keys enforced, writing a real user id there failed, and the rows could not be applied
//! on another device either. Only the constraint target changes; no data moves.
//!
//! The change is made in the stored table definition (SQLite "other kinds of table schema
//! changes" procedure: `writable_schema`, bump `schema_version`), then verified.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000164_legacy_users_fk_to_user_accounts"
    }
}

const TABLES: &[&str] = &["pm_executions", "schedule_break_ins", "schedule_change_log"];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let version: i64 = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA schema_version".to_string(),
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("schema_version unavailable".into()))?
            .try_get_by_index(0)?;
        db.execute_unprepared("PRAGMA writable_schema = ON").await?;
        for table in TABLES {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sqlite_master
                 SET sql = replace(replace(sql, 'REFERENCES users(id)', 'REFERENCES user_accounts(id)'),
                                   'REFERENCES \"users\"(id)', 'REFERENCES user_accounts(id)')
                 WHERE type = 'table' AND name = ?",
                [(*table).into()],
            ))
            .await?;
        }
        db.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
            .await?;
        db.execute_unprepared("PRAGMA writable_schema = OFF").await?;

        for table in TABLES {
            let still_legacy = db
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("PRAGMA foreign_key_list('{table}')"),
                ))
                .await?
                .iter()
                .any(|r| r.try_get::<String>("", "table").is_ok_and(|t| t == "users"));
            if still_legacy {
                return Err(DbErr::Custom(format!(
                    "{table}: a foreign key to the legacy users table could not be retargeted"
                )));
            }
        }
        let integrity: String = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA integrity_check".to_string(),
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("integrity_check returned nothing".into()))?
            .try_get_by_index(0)?;
        if integrity != "ok" {
            return Err(DbErr::Custom(format!(
                "integrity check after migration 164: {integrity}"
            )));
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
