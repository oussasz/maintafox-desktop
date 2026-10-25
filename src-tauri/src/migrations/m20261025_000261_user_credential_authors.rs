//! Migration 261 — who changed an account's password, and who enabled or disabled it.
//!
//! A password reset or an account re-enabled travels by sync with the account row. The receiving
//! PC checks that the person who made it could (`sync::credential_check`), so the row names them:
//! - `password_changed_by_id`: who wrote the current password (the account itself for a change of
//!   one's own password, the administrator for a reset, the creator for a new account);
//! - `password_set_by_id` / `password_set_at`: who last set the password other than the account
//!   itself, and when (an administrator's reset, the creator; the account itself only for the first
//!   administrator and a vendor recovery account, which are their own root). A change of one's own
//!   password keeps them: it is only as trusted as the password it replaces (a temporary password
//!   an unauthorised administrator set stays refused once its holder changes it);
//! - `status_changed_by_id`: who last enabled, disabled, restored or deleted the account.
//!
//! The authors are references to `user_accounts` (sent as `password_changed_by_sync_id`,
//! `password_set_by_sync_id` and `status_changed_by_sync_id`; soft in `sync::row_spec`: a change of
//! one's own password names the row itself). Existing rows keep NULL authors ("not recorded"): no
//! author is guessed from the ledgers. An existing password is dated, though: `password_set_at`
//! takes its `password_changed_at` (its `created_at` when the password was never changed; both are
//! the same on every PC that holds that password), `password_set_by_id` staying NULL ("set before
//! 0.9.2, setter not recorded"). A change of one's own password that keeps that date is accepted on
//! the other PCs; one carrying another date with nobody named is not (a reset made on a PC still
//! before 0.9.2, carried over by its upgrade: `sync::credential_check`).
//! Already-present columns are left alone and only undated passwords are dated (a re-run changes
//! nothing).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261025_000261_user_credential_authors"
    }
}

async fn has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

const COLUMNS: &[(&str, &str)] = &[
    ("password_changed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("status_changed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("password_set_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("password_set_at", "TEXT NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for (column, definition) in COLUMNS {
            if !has_column(&txn, "user_accounts", column).await? {
                txn.execute_unprepared(&format!("ALTER TABLE user_accounts ADD COLUMN {column} {definition}"))
                    .await?;
            }
        }
        txn.execute_unprepared(
            "UPDATE user_accounts SET password_set_at = COALESCE(password_changed_at, created_at) \
              WHERE password_hash IS NOT NULL AND password_set_at IS NULL AND password_set_by_id IS NULL",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    use crate::sync::row_layout::{Role, Target};
    use crate::sync::test_support::{exec, scalar_i64, started_db, started_db_before_migration};

    #[tokio::test]
    async fn an_upgraded_database_keeps_its_passwords_and_sends_both_authors_as_account_links() {
        let db = started_db_before_migration("m20261025_000261_user_credential_authors").await;
        // Accounts as an earlier version leaves them: a password changed on a date, one never
        // changed since its creation, and an account without a password (SSO).
        for (sync_id, hash, changed_at) in [
            ("legacy-changed", Some("h1"), Some("2026-05-01T08:00:00Z")),
            ("legacy-never-changed", Some("h2"), None),
            ("legacy-sso", None, None),
        ] {
            exec(
                &db,
                "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, is_active, \
                   is_admin, force_password_change, password_changed_at, failed_login_attempts, created_at, \
                   updated_at, row_version) \
                 VALUES (?, ?, 'local', ?, 1, 0, 0, ?, 0, '2026-04-01T08:00:00Z', '2026-04-01T08:00:00Z', 1)",
                vec![
                    sync_id.into(),
                    sync_id.into(),
                    hash.map(str::to_string).into(),
                    changed_at.map(str::to_string).into(),
                ],
            )
            .await;
        }
        let hashes = "SELECT COUNT(*) FROM user_accounts WHERE password_hash IS NOT NULL";
        let before = scalar_i64(&db, "SELECT COUNT(*) FROM user_accounts", vec![]).await;
        let hashed = scalar_i64(&db, hashes, vec![]).await;
        crate::migrations::Migrator::up(&db, None).await.expect("migration 261");
        assert_eq!(
            scalar_i64(&db, "SELECT COUNT(*) FROM user_accounts", vec![]).await,
            before
        );
        assert_eq!(scalar_i64(&db, hashes, vec![]).await, hashed, "passwords kept");
        for column in ["password_changed_by_id", "status_changed_by_id", "password_set_by_id"] {
            assert_eq!(
                scalar_i64(
                    &db,
                    &format!("SELECT COUNT(*) FROM user_accounts WHERE {column} IS NOT NULL"),
                    vec![],
                )
                .await,
                0,
                "{column}: not recorded, never guessed"
            );
        }
        // A password is dated by when it was set (else created), its setter not recorded; an account
        // without a password stays undated.
        let set_at = |sync_id: &'static str| {
            let db = db.clone();
            async move {
                scalar_i64(
                    &db,
                    "SELECT COUNT(*) FROM user_accounts WHERE entity_sync_id = ? AND password_set_at IS NOT NULL \
                       AND password_set_at = COALESCE(password_changed_at, created_at)",
                    vec![sync_id.into()],
                )
                .await
            }
        };
        assert_eq!(set_at("legacy-changed").await, 1);
        assert_eq!(set_at("legacy-never-changed").await, 1);
        assert_eq!(set_at("legacy-sso").await, 0);
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM user_accounts WHERE password_hash IS NULL AND password_set_at IS NOT NULL",
                vec![],
            )
            .await,
            0
        );
        // Run again: nothing changes.
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migration 261 again");

        let db = started_db().await;
        let layout = crate::sync::row_layout::layout(&db, "user_accounts")
            .await
            .expect("layout");
        for (column, key) in [
            ("password_changed_by_id", "password_changed_by_sync_id"),
            ("status_changed_by_id", "status_changed_by_sync_id"),
            ("password_set_by_id", "password_set_by_sync_id"),
        ] {
            let found = layout
                .columns
                .iter()
                .find(|c| c.name == column)
                .unwrap_or_else(|| panic!("{column} in the layout"));
            match &found.role {
                Role::Ref {
                    key: k,
                    target: Target::Entity { entity_type, .. },
                    soft,
                    ..
                } => {
                    assert_eq!(k, key);
                    assert_eq!(*entity_type, "user_accounts");
                    assert!(*soft, "{column}: an author not here yet never holds the account back");
                }
                other => panic!("{column} must travel as an account link, got {other:?}"),
            }
        }
        let set_at = layout
            .columns
            .iter()
            .find(|c| c.name == "password_set_at")
            .expect("password_set_at in the layout");
        assert!(matches!(&set_at.role, Role::Value { key } if key == "password_set_at"));
    }
}
