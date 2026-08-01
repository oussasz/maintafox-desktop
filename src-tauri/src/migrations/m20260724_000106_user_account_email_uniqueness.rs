use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260724_000106_user_account_email_uniqueness"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // DB-level uniqueness policy for active (non-deleted) accounts.
        // Triggers are used instead of a unique index to avoid migration failure
        // when historical duplicates already exist; new writes are enforced.
        db.execute_unprepared(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_user_accounts_email_unique_insert
            BEFORE INSERT ON user_accounts
            WHEN NEW.email IS NOT NULL
                 AND TRIM(NEW.email) != ''
                 AND EXISTS (
                   SELECT 1
                   FROM user_accounts ua
                   WHERE ua.deleted_at IS NULL
                     AND ua.email IS NOT NULL
                     AND LOWER(TRIM(ua.email)) = LOWER(TRIM(NEW.email))
                 )
            BEGIN
              SELECT RAISE(ABORT, 'email must be unique');
            END;
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_user_accounts_email_unique_update
            BEFORE UPDATE OF email ON user_accounts
            WHEN NEW.email IS NOT NULL
                 AND TRIM(NEW.email) != ''
                 AND EXISTS (
                   SELECT 1
                   FROM user_accounts ua
                   WHERE ua.deleted_at IS NULL
                     AND ua.email IS NOT NULL
                     AND LOWER(TRIM(ua.email)) = LOWER(TRIM(NEW.email))
                     AND ua.id != NEW.id
                 )
            BEGIN
              SELECT RAISE(ABORT, 'email must be unique');
            END;
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE INDEX IF NOT EXISTS idx_user_accounts_email_lower_active
            ON user_accounts(LOWER(email))
            WHERE deleted_at IS NULL AND email IS NOT NULL;
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TRIGGER IF EXISTS trg_user_accounts_email_unique_insert")
            .await?;
        db.execute_unprepared("DROP TRIGGER IF EXISTS trg_user_accounts_email_unique_update")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_user_accounts_email_lower_active")
            .await?;
        Ok(())
    }
}
