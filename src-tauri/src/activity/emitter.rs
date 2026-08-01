use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::activity::{Result, SqlitePool};

#[derive(Debug, Clone)]
pub struct ActivityEventInput {
    pub event_class: String,
    pub event_code: String,
    pub source_module: String,
    pub source_record_type: Option<String>,
    pub source_record_id: Option<String>,
    pub entity_scope_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub severity: String,
    pub summary_json: Option<serde_json::Value>,
    pub correlation_id: Option<String>,
    pub visibility_scope: String,
}

pub async fn emit_activity_event(pool: &SqlitePool, input: ActivityEventInput) -> Result<()> {
    if let Err(err) = emit_activity_event_inner(pool, input).await {
        tracing::error!(
            error = %err,
            "activity::emit_activity_event fire-and-log failure"
        );
    }
    Ok(())
}

async fn emit_activity_event_inner(pool: &SqlitePool, input: ActivityEventInput) -> Result<()> {
    let summary_json = input.summary_json.map(|value| value.to_string());

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO activity_events
            (event_class, event_code, source_module, source_record_type, source_record_id,
             entity_scope_id, actor_id, happened_at, severity, summary_json, correlation_id, visibility_scope)
         VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, ?, ?, ?)",
        [
            input.event_class.into(),
            input.event_code.into(),
            input.source_module.into(),
            input.source_record_type.into(),
            input.source_record_id.into(),
            input.entity_scope_id.into(),
            input.actor_id.into(),
            input.severity.into(),
            summary_json.into(),
            input.correlation_id.into(),
            input.visibility_scope.into(),
        ],
    ))
    .await?;

    Ok(())
}

pub async fn emit_wo_event(
    pool: &SqlitePool,
    wo_id: i64,
    event_code: &str,
    actor_id: Option<i64>,
    summary_json: Option<serde_json::Value>,
    corr_id: Option<String>,
) -> Result<()> {
    emit_activity_event(
        pool,
        ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: event_code.to_string(),
            source_module: "wo".to_string(),
            source_record_type: Some("work_order".to_string()),
            source_record_id: Some(wo_id.to_string()),
            entity_scope_id: None,
            actor_id,
            severity: "info".to_string(),
            summary_json,
            correlation_id: corr_id,
            visibility_scope: "entity".to_string(),
        },
    )
    .await
}

pub async fn emit_di_event(
    pool: &SqlitePool,
    di_id: i64,
    event_code: &str,
    actor_id: Option<i64>,
    summary_json: Option<serde_json::Value>,
    corr_id: Option<String>,
) -> Result<()> {
    emit_activity_event(
        pool,
        ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: event_code.to_string(),
            source_module: "di".to_string(),
            source_record_type: Some("intervention_request".to_string()),
            source_record_id: Some(di_id.to_string()),
            entity_scope_id: None,
            actor_id,
            severity: "info".to_string(),
            summary_json,
            correlation_id: corr_id,
            visibility_scope: "entity".to_string(),
        },
    )
    .await
}

pub async fn emit_rbac_event(
    pool: &SqlitePool,
    actor_id: Option<i64>,
    event_code: &str,
    summary_json: Option<serde_json::Value>,
) -> Result<()> {
    emit_activity_event(
        pool,
        ActivityEventInput {
            event_class: "security".to_string(),
            event_code: event_code.to_string(),
            source_module: "rbac".to_string(),
            source_record_type: Some("role_assignment".to_string()),
            source_record_id: None,
            entity_scope_id: None,
            actor_id,
            severity: "info".to_string(),
            summary_json,
            correlation_id: None,
            visibility_scope: "global".to_string(),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn emit_activity_event_returns_ok_even_when_insert_fails() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE activity_events".to_string(),
        ))
        .await
        .expect("drop activity_events");

        let result = emit_activity_event(
            &db,
            ActivityEventInput {
                event_class: "operational".to_string(),
                event_code: "wo.closed".to_string(),
                source_module: "wo".to_string(),
                source_record_type: Some("work_order".to_string()),
                source_record_id: Some("1".to_string()),
                entity_scope_id: None,
                actor_id: Some(1),
                severity: "info".to_string(),
                summary_json: Some(serde_json::json!({"test": true})),
                correlation_id: Some("corr-1".to_string()),
                visibility_scope: "global".to_string(),
            },
        )
        .await;

        assert!(
            result.is_ok(),
            "emit_activity_event must swallow insert failures and return Ok(())"
        );
    }
}
