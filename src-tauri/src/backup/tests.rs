//! Integration tests for the backup module.
//!
//! These tests verify Sprint S1 acceptance criteria:
//!   V1 — Migration 008 applies and creates backup_policies + backup_runs tables
//!   V2 — backup_runs table has all expected columns
//!   V3 — run_manual_backup produces a file with a non-null SHA-256 checksum

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    /// V1 — Migration 008 applies. After running Migrator::up, both
    /// `backup_policies` and `backup_runs` tables must exist.
    #[tokio::test]
    async fn v1_migration_008_creates_backup_tables() {
        let db = Database::connect("sqlite::memory:").await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("All migrations should apply (including 008)");

        // Verify backup_policies exists
        let policies_exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='backup_policies';".to_string(),
            ))
            .await
            .expect("query should succeed");
        assert!(
            policies_exists.is_some(),
            "backup_policies table must exist after migration 008"
        );

        // Verify backup_runs exists
        let runs_exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='backup_runs';".to_string(),
            ))
            .await
            .expect("query should succeed");
        assert!(
            runs_exists.is_some(),
            "backup_runs table must exist after migration 008"
        );

        // Verify migration count is now 8
        let migration_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM seaql_migrations;".to_string(),
            ))
            .await
            .expect("query should succeed")
            .expect("should return a row")
            .try_get::<i64>("", "cnt")
            .expect("cnt should be readable");

        assert_eq!(migration_count, 11, "11 migrations should be applied (001-011)");
    }

    /// V2 — backup_runs table has all expected columns.
    /// Uses PRAGMA table_info to enumerate every column.
    #[tokio::test]
    async fn v2_backup_runs_has_correct_columns() {
        let db = Database::connect("sqlite::memory:").await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(backup_runs);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info should succeed");

        let column_names: Vec<String> = rows.iter().map(|r| r.try_get::<String>("", "name").unwrap()).collect();

        let expected_columns = [
            "id",
            "policy_id",
            "trigger",
            "status",
            "output_path",
            "file_size_bytes",
            "sha256_checksum",
            "encryption_mode",
            "db_schema_version",
            "started_at",
            "completed_at",
            "error_message",
            "initiated_by_id",
        ];

        for expected in &expected_columns {
            assert!(
                column_names.contains(&expected.to_string()),
                "backup_runs is missing column '{}'. Present columns: {:?}",
                expected,
                column_names
            );
        }

        assert_eq!(
            column_names.len(),
            expected_columns.len(),
            "backup_runs should have exactly {} columns, found {}: {:?}",
            expected_columns.len(),
            column_names.len(),
            column_names
        );
    }

    /// V2b — backup_policies table has all expected columns.
    #[tokio::test]
    async fn v2b_backup_policies_has_correct_columns() {
        let db = Database::connect("sqlite::memory:").await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(backup_policies);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info should succeed");

        let column_names: Vec<String> = rows.iter().map(|r| r.try_get::<String>("", "name").unwrap()).collect();

        let expected_columns = [
            "id",
            "policy_name",
            "target_directory",
            "encryption_mode",
            "retention_daily",
            "retention_weekly",
            "retention_monthly",
            "is_active",
            "created_at",
            "updated_at",
        ];

        for expected in &expected_columns {
            assert!(
                column_names.contains(&expected.to_string()),
                "backup_policies is missing column '{}'. Present columns: {:?}",
                expected,
                column_names
            );
        }
    }

    /// V3 — run_manual_backup produces a valid file with a 64-char SHA-256 checksum,
    /// and validate_backup_file confirms integrity.
    ///
    /// This test uses a file-based SQLite DB (tempfile) because VACUUM INTO
    /// does not work on in-memory databases.
    #[tokio::test]
    async fn v3_manual_backup_produces_checksum_and_validates() {
        use crate::backup;

        // Create a temp directory for the test DB and backup
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let db_path = temp_dir.path().join("test_live.db");
        let backup_path = temp_dir.path().join("test_backup.db");

        let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
        let db = Database::connect(&db_url).await.expect("create file-based DB");

        // Apply WAL mode + all migrations
        db.execute_unprepared("PRAGMA journal_mode=WAL;")
            .await
            .expect("set WAL mode");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        // Run the manual backup
        let backup_path_str = backup_path.to_string_lossy().to_string();
        let result = backup::run_manual_backup(&db, &backup_path_str, 1)
            .await
            .expect("run_manual_backup should succeed");

        // V3 check: SHA-256 checksum is a 64-character lowercase hex string
        assert_eq!(
            result.sha256_checksum.len(),
            64,
            "SHA-256 checksum must be 64 hex chars, got: '{}'",
            result.sha256_checksum
        );
        assert!(
            result
                .sha256_checksum
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "SHA-256 checksum must be lowercase hex: '{}'",
            result.sha256_checksum
        );

        // V3 check: file exists and has non-zero size
        assert!(backup_path.exists(), "backup file should exist at {}", backup_path_str);
        assert!(result.file_size_bytes > 0, "backup file size should be > 0");
        assert_eq!(result.status, "success");

        // V3 check: query backup_runs directly — checksum is NOT NULL
        let stored = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT sha256_checksum FROM backup_runs ORDER BY id DESC LIMIT 1;".to_string(),
            ))
            .await
            .expect("query should succeed")
            .expect("should have a row");

        let stored_checksum = stored
            .try_get::<Option<String>>("", "sha256_checksum")
            .expect("column should be readable");

        assert!(
            stored_checksum.is_some(),
            "sha256_checksum in backup_runs must not be NULL"
        );
        assert_eq!(
            stored_checksum.unwrap(),
            result.sha256_checksum,
            "stored checksum must match the returned checksum"
        );

        // V3 bonus: validate_backup_file confirms integrity and checksum match
        let validation = backup::validate_backup_file(&db, &backup_path_str)
            .await
            .expect("validate_backup_file should succeed");

        assert!(validation.integrity_ok, "backup file should pass integrity check");
        assert!(
            validation.checksum_match,
            "stored checksum should match computed checksum"
        );
        assert_eq!(
            validation.integrity_check_output.trim(),
            "ok",
            "PRAGMA integrity_check should return 'ok'"
        );
        assert_eq!(
            validation.computed_checksum, result.sha256_checksum,
            "computed checksum should match the original backup checksum"
        );
    }

    /// validate_backup_file returns NotFound for a non-existent path.
    #[tokio::test]
    async fn validate_backup_file_not_found() {
        use crate::backup;

        let db = Database::connect("sqlite::memory:").await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let err = backup::validate_backup_file(&db, "/nonexistent/path/does_not_exist.db")
            .await
            .expect_err("should fail for non-existent file");

        assert!(
            matches!(err, crate::errors::AppError::NotFound { .. }),
            "expected NotFound error, got: {:?}",
            err
        );
    }

    /// list_backup_runs returns an empty vec on a fresh database.
    #[tokio::test]
    async fn list_backup_runs_empty() {
        use crate::backup;

        let db = Database::connect("sqlite::memory:").await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let runs = backup::list_backup_runs(&db, 20)
            .await
            .expect("list_backup_runs should succeed");

        assert!(runs.is_empty(), "no backup runs should exist on a fresh database");
    }
}
