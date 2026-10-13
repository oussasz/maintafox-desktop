use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sha2::{Digest, Sha256};

use crate::errors::AppResult;

#[derive(Debug, Clone)]
pub struct AuditEventInput {
    pub action_code: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub actor_id: Option<i64>,
    pub auth_context: String,
    pub result: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub retention_class: String,
    pub details_json: Option<serde_json::Value>,
}

/// Serialises the audit writers of this process: each event takes the next position of the
/// device's chain (`audit::chain`). The unique index on `(chain_origin, chain_seq)` makes a write
/// that still races (another process on the same database) fail instead of forking the chain.
static CHAIN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Writes and chains an audit event in its own transaction. Persistence failures are returned
/// (and logged) so callers can decide whether to block.
pub async fn write_audit_event(pool: &sea_orm::DatabaseConnection, input: AuditEventInput) -> AppResult<i64> {
    let _guard = CHAIN_LOCK.lock().await;
    let result = async {
        let txn = pool.begin().await?;
        let id = write_audit_event_in(&txn, input).await?;
        txn.commit().await?;
        Ok(id)
    }
    .await;
    if let Err(err) = &result {
        tracing::error!(error = %err, "CRITICAL: audit::write_audit_event failed");
    }
    result
}

/// Writes and chains an audit event inside the caller's transaction (the event is committed or
/// rolled back with the business change it records).
pub async fn write_audit_event_in(conn: &impl ConnectionTrait, input: AuditEventInput) -> AppResult<i64> {
    let details_json = input.details_json.map(|value| value.to_string());
    let sync_id = crate::sync::identity::new_entity_sync_id();
    let (origin, seq, prev_hash) = crate::audit::chain::next_position(conn).await?;
    let happened_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let insert = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO audit_events
                (entity_sync_id, action_code, target_type, target_id, actor_id, auth_context, result,
                 before_hash, after_hash, happened_at, retention_class, details_json,
                 chain_origin, chain_seq, chain_prev_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                sync_id.clone().into(),
                input.action_code.into(),
                input.target_type.into(),
                input.target_id.into(),
                input.actor_id.into(),
                input.auth_context.into(),
                input.result.into(),
                input.before_hash.into(),
                input.after_hash.into(),
                happened_at.into(),
                input.retention_class.into(),
                details_json.into(),
                origin.into(),
                seq.into(),
                prev_hash.clone().into(),
            ],
        ))
        .await?;
    crate::audit::chain::seal(conn, &sync_id, prev_hash.as_deref()).await?;
    Ok(insert.last_insert_id() as i64)
}

pub fn compute_hash(value: &serde_json::Value) -> String {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    let digest = Sha256::digest(serialized.as_bytes());
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn write_audit_event_propagates_error_on_insert_failure() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE audit_events".to_string(),
        ))
        .await
        .expect("drop audit_events");

        let result = write_audit_event(
            &db,
            AuditEventInput {
                action_code: "test.action".to_string(),
                target_type: Some("test".to_string()),
                target_id: Some("1".to_string()),
                actor_id: Some(1),
                auth_context: "password".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({"ok": true})),
            },
        )
        .await;

        assert!(result.is_err(), "write_audit_event must propagate insert errors");
    }

    #[test]
    fn compute_hash_is_stable_and_64_chars() {
        let value = serde_json::json!({
            "action_code": "rbac.role_assigned",
            "actor_id": 5,
            "result": "success"
        });

        let h1 = compute_hash(&value);
        let h2 = compute_hash(&value);

        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
