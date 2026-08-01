use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sha2::{Digest, Sha256};

use crate::archive::{Result, SqlitePool};

#[derive(Debug, Clone)]
pub struct ArchiveInput {
    pub source_module: String,
    pub source_record_id: String,
    pub archive_class: String,
    pub source_state: Option<String>,
    pub archive_reason_code: String,
    pub archived_by_id: Option<i64>,
    pub restore_policy: String,
    pub restore_until_at: Option<String>,
    pub payload_json: serde_json::Value,
    pub workflow_history_json: Option<String>,
    pub attachment_manifest_json: Option<String>,
    pub config_version_refs_json: Option<String>,
    pub search_text: Option<String>,
}

pub async fn archive_record(pool: &SqlitePool, input: ArchiveInput) -> Result<i64> {
    let payload_bytes = serde_json::to_vec(&input.payload_json)?;
    let payload_size_bytes = payload_bytes.len() as i64;
    let checksum_sha256 = hex::encode(Sha256::digest(&payload_bytes));

    let retention_policy_id = lookup_retention_policy_id(pool, &input.source_module, &input.archive_class).await?;

    let item_insert = pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO archive_items
                (source_module, source_record_id, archive_class, source_state, archive_reason_code, archived_by_id,
                 retention_policy_id, restore_policy, restore_until_at, legal_hold, checksum_sha256, search_text)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
            [
                input.source_module.clone().into(),
                input.source_record_id.clone().into(),
                input.archive_class.clone().into(),
                input.source_state.clone().into(),
                input.archive_reason_code.clone().into(),
                input.archived_by_id.into(),
                retention_policy_id.into(),
                input.restore_policy.clone().into(),
                input.restore_until_at.clone().into(),
                checksum_sha256.into(),
                input.search_text.clone().into(),
            ],
        ))
        .await?;
    let archive_item_id = item_insert.last_insert_id() as i64;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO archive_payloads
            (archive_item_id, payload_json_compressed, workflow_history_json, attachment_manifest_json, config_version_refs_json, payload_size_bytes)
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            archive_item_id.into(),
            payload_bytes.into(),
            input.workflow_history_json.into(),
            input.attachment_manifest_json.into(),
            input.config_version_refs_json.into(),
            payload_size_bytes.into(),
        ],
    ))
    .await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO archive_actions
            (archive_item_id, action, action_by_id, reason_note, result_status)
         VALUES (?, 'archive', ?, ?, 'success')",
        [
            archive_item_id.into(),
            input.archived_by_id.into(),
            input.archive_reason_code.into(),
        ],
    ))
    .await?;

    Ok(archive_item_id)
}

async fn lookup_retention_policy_id(
    pool: &SqlitePool,
    source_module: &str,
    archive_class: &str,
) -> Result<Option<i64>> {
    let module_row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id
             FROM retention_policies
             WHERE module_code = ?
               AND archive_class = ?
             LIMIT 1",
            [source_module.into(), archive_class.into()],
        ))
        .await?;

    if let Some(row) = module_row {
        return Ok(Some(row.try_get::<i64>("", "id")?));
    }

    let global_row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id
             FROM retention_policies
             WHERE module_code = 'global'
               AND archive_class = ?
             LIMIT 1",
            [archive_class.into()],
        ))
        .await?;

    if let Some(row) = global_row {
        return Ok(Some(row.try_get::<i64>("", "id")?));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn archive_record_creates_archive_item_payload_and_action_rows() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts
                (sync_id, username, display_name, is_active, is_admin, force_password_change, created_at, updated_at)
             VALUES (?, ?, ?, 1, 0, 0, ?, ?)",
            [
                uuid::Uuid::new_v4().to_string().into(),
                "archive-user".into(),
                "Archive User".into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("insert user");

        let user_id = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'archive-user'".to_string(),
            ))
            .await
            .expect("query user")
            .expect("user row")
            .try_get::<i64>("", "id")
            .expect("user id");

        let archive_item_id = archive_record(
            &db,
            ArchiveInput {
                source_module: "wo".to_string(),
                source_record_id: "WO-1001".to_string(),
                archive_class: "operational_history".to_string(),
                source_state: Some("closed".to_string()),
                archive_reason_code: "completed".to_string(),
                archived_by_id: Some(user_id),
                restore_policy: "admin_only".to_string(),
                restore_until_at: None,
                payload_json: serde_json::json!({
                    "code": "WO-1001",
                    "status": "closed"
                }),
                workflow_history_json: Some("[{\"from\":\"in_progress\",\"to\":\"closed\"}]".to_string()),
                attachment_manifest_json: None,
                config_version_refs_json: None,
                search_text: Some("WO-1001 closed".to_string()),
            },
        )
        .await
        .expect("archive record");

        let archive_items_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS cnt FROM archive_items WHERE id = {archive_item_id}"),
            ))
            .await
            .expect("count archive items")
            .expect("archive item row")
            .try_get::<i64>("", "cnt")
            .expect("archive items count");
        assert_eq!(archive_items_count, 1);

        let payload_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS cnt FROM archive_payloads WHERE archive_item_id = {archive_item_id}"),
            ))
            .await
            .expect("count archive payloads")
            .expect("archive payload row")
            .try_get::<i64>("", "cnt")
            .expect("archive payload count");
        assert_eq!(payload_count, 1);

        let action_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS cnt FROM archive_actions
                     WHERE archive_item_id = {archive_item_id}
                       AND action = 'archive'"
                ),
            ))
            .await
            .expect("count archive actions")
            .expect("archive action row")
            .try_get::<i64>("", "cnt")
            .expect("archive action count");
        assert_eq!(action_count, 1);
    }
}
