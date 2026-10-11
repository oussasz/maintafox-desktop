//! Hand-written outbox staging for intervention requests and PM (moved to the generic capture in
//! their Phase 2.4 domain steps; inventory and media already use it).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    InterventionRequestSyncPayload, PmOccurrenceSyncPayload, PmPlanSyncPayload, PmPlanVersionSyncPayload,
    StageOutboxItemInput, SYNC_ENTITY_INTERVENTION_REQUESTS, SYNC_ENTITY_PM_OCCURRENCES, SYNC_ENTITY_PM_PLANS,
    SYNC_ENTITY_PM_PLAN_VERSIONS,
};
use crate::sync::identity::{parent_entity_sync_id, require_entity_sync_id, require_parent_sync_id};
use crate::sync::queries::stage_outbox_item;

async fn lookup_entity_sync_id(db: &impl ConnectionTrait, table: &str, id: i64) -> AppResult<Option<String>> {
    parent_entity_sync_id(db, table, id).await.map(Some)
}

pub async fn stage_intervention_request_sync(db: &impl ConnectionTrait, di_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT ir.entity_sync_id, ir.code, ir.title, ir.description, ir.status, ir.origin_type,
                    ir.row_version, ir.updated_at, ir.equipment_id, ir.org_node_id, ir.submitter_id
             FROM intervention_requests ir WHERE ir.id = ?",
            [di_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid = require_entity_sync_id(
        "intervention_requests",
        di_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("intervention_requests.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let equipment_id: i64 = row.try_get("", "equipment_id").unwrap_or(0);
    let org_node_id: i64 = row.try_get("", "org_node_id").unwrap_or(0);
    let submitter_id: i64 = row.try_get("", "submitter_id").unwrap_or(0);
    let payload = InterventionRequestSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        title: row.try_get("", "title").unwrap_or_default(),
        description: row.try_get("", "description").unwrap_or_default(),
        status: row.try_get("", "status").unwrap_or_default(),
        origin_type: row.try_get("", "origin_type").unwrap_or_default(),
        equipment_entity_sync_id: if equipment_id > 0 {
            lookup_entity_sync_id(db, "equipment", equipment_id).await?
        } else {
            None
        },
        org_node_entity_sync_id: if org_node_id > 0 {
            lookup_entity_sync_id(db, "org_nodes", org_node_id).await?
        } else {
            None
        },
        submitter_entity_sync_id: if submitter_id > 0 {
            lookup_entity_sync_id(db, "user_accounts", submitter_id).await?
        } else {
            None
        },
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_INTERVENTION_REQUESTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_plan_sync(db: &impl ConnectionTrait, plan_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT p.entity_sync_id, p.code, p.title, p.description, p.strategy_type,
                    p.asset_scope_type, p.lifecycle_status, p.is_active, p.row_version, p.updated_at,
                    p.assigned_team_id
             FROM pm_plans p WHERE p.id = ?",
            [plan_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid = require_entity_sync_id(
        "pm_plans",
        plan_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("pm_plans.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let team_id: Option<i64> = row.try_get("", "assigned_team_id").ok();
    let payload = PmPlanSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        title: row.try_get("", "title").unwrap_or_default(),
        description: row.try_get("", "description").ok(),
        strategy_type: row.try_get("", "strategy_type").unwrap_or_default(),
        asset_scope_type: row.try_get("", "asset_scope_type").unwrap_or_default(),
        assigned_team_entity_sync_id: match team_id {
            Some(id) => lookup_entity_sync_id(db, "teams", id).await?,
            None => None,
        },
        lifecycle_status: row.try_get("", "lifecycle_status").unwrap_or_else(|_| "draft".into()),
        is_active: is_active != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_PM_PLANS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_plan_version_sync(db: &impl ConnectionTrait, version_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT v.entity_sync_id, v.version_no, v.status, v.effective_from,
                    v.trigger_definition_json, v.row_version, v.updated_at,
                    p.entity_sync_id AS pm_plan_entity_sync_id
             FROM pm_plan_versions v
             JOIN pm_plans p ON p.id = v.pm_plan_id
             WHERE v.id = ?",
            [version_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let plan_esid: String = require_parent_sync_id(&row, "pm_plan_entity_sync_id")?;
    let esid = require_entity_sync_id(
        "pm_plan_versions",
        version_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("pm_plan_versions.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = PmPlanVersionSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        pm_plan_entity_sync_id: plan_esid,
        version_no: row.try_get("", "version_no").unwrap_or(1),
        status: row.try_get("", "status").unwrap_or_else(|_| "draft".into()),
        effective_from: row.try_get("", "effective_from").ok(),
        trigger_definition_json: row.try_get("", "trigger_definition_json").ok(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_PM_PLAN_VERSIONS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_occurrence_sync(db: &impl ConnectionTrait, occurrence_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT o.entity_sync_id, o.due_basis, o.status, o.due_at, o.row_version,
                    p.entity_sync_id AS pm_plan_entity_sync_id,
                    v.entity_sync_id AS pm_plan_version_entity_sync_id,
                    w.entity_sync_id AS linked_work_order_entity_sync_id
             FROM pm_occurrences o
             JOIN pm_plans p ON p.id = o.pm_plan_id
             JOIN pm_plan_versions v ON v.id = o.plan_version_id
             LEFT JOIN work_orders w ON w.id = o.linked_work_order_id
             WHERE o.id = ?",
            [occurrence_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let plan_esid: String = require_parent_sync_id(&row, "pm_plan_entity_sync_id")?;
    let ver_esid: String = require_parent_sync_id(&row, "pm_plan_version_entity_sync_id")?;
    let esid = require_entity_sync_id(
        "pm_occurrences",
        occurrence_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("pm_occurrences.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = PmOccurrenceSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        pm_plan_entity_sync_id: plan_esid,
        pm_plan_version_entity_sync_id: ver_esid,
        due_basis: row.try_get("", "due_basis").unwrap_or_else(|_| "calendar".into()),
        status: row.try_get("", "status").unwrap_or_else(|_| "generated".into()),
        due_at: row.try_get("", "due_at").ok(),
        linked_work_order_entity_sync_id: row.try_get("", "linked_work_order_entity_sync_id").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_PM_OCCURRENCES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}
