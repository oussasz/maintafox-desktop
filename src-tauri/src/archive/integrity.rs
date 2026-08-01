use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sha2::{Digest, Sha256};

use crate::archive::{Result, SqlitePool};

pub async fn verify_checksum(pool: &SqlitePool, archive_item_id: i64) -> Result<bool> {
    let row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ai.checksum_sha256, ap.payload_json_compressed
             FROM archive_items ai
             JOIN archive_payloads ap ON ap.archive_item_id = ai.id
             WHERE ai.id = ?
             LIMIT 1",
            [archive_item_id.into()],
        ))
        .await?;

    let is_valid = if let Some(row) = row {
        let stored_checksum = row.try_get::<Option<String>>("", "checksum_sha256")?;
        let payload_bytes = row.try_get::<Vec<u8>>("", "payload_json_compressed")?;
        let recomputed = hex::encode(Sha256::digest(&payload_bytes));
        stored_checksum.as_deref() == Some(recomputed.as_str())
    } else {
        false
    };

    let result_status = if is_valid { "success" } else { "failed" };
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO archive_actions
            (archive_item_id, action, action_by_id, reason_note, result_status)
         VALUES (?, 'checksum_verified', NULL, NULL, ?)",
        [archive_item_id.into(), result_status.into()],
    ))
    .await?;

    Ok(is_valid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::writer::{archive_record, ArchiveInput};
    use sea_orm::{Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn setup_archived_record(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts
                (sync_id, username, display_name, is_active, is_admin, force_password_change, created_at, updated_at)
             VALUES (?, ?, ?, 1, 0, 0, ?, ?)",
            [
                uuid::Uuid::new_v4().to_string().into(),
                "archive-integrity-user".into(),
                "Archive Integrity User".into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("insert user");

        let user_id = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'archive-integrity-user'".to_string(),
            ))
            .await
            .expect("query user")
            .expect("user row")
            .try_get::<i64>("", "id")
            .expect("user id");

        archive_record(
            db,
            ArchiveInput {
                source_module: "wo".to_string(),
                source_record_id: "WO-2001".to_string(),
                archive_class: "operational_history".to_string(),
                source_state: Some("closed".to_string()),
                archive_reason_code: "completed".to_string(),
                archived_by_id: Some(user_id),
                restore_policy: "admin_only".to_string(),
                restore_until_at: None,
                payload_json: serde_json::json!({
                    "code": "WO-2001",
                    "status": "closed"
                }),
                workflow_history_json: Some("[]".to_string()),
                attachment_manifest_json: None,
                config_version_refs_json: None,
                search_text: Some("WO-2001".to_string()),
            },
        )
        .await
        .expect("archive record")
    }

    #[tokio::test]
    async fn verify_checksum_returns_true_for_fresh_payload() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        let archive_item_id = setup_archived_record(&db).await;
        let is_valid = verify_checksum(&db, archive_item_id)
            .await
            .expect("verify checksum");
        assert!(is_valid, "freshly archived payload should verify");
    }

    #[tokio::test]
    async fn verify_checksum_returns_false_when_payload_tampered() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        let archive_item_id = setup_archived_record(&db).await;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE archive_payloads
             SET payload_json_compressed = ?
             WHERE archive_item_id = ?",
            [b"{\"tampered\":true}".to_vec().into(), archive_item_id.into()],
        ))
        .await
        .expect("tamper payload");

        let is_valid = verify_checksum(&db, archive_item_id)
            .await
            .expect("verify checksum");
        assert!(!is_valid, "tampered payload should fail checksum verification");
    }
}
