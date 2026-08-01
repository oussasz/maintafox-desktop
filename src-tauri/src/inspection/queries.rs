use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, ExecResult, QueryResult, Statement, TransactionTrait,
};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    InspectionCheckpointSyncPayload, InspectionRoundSyncPayload, InspectionTemplateSyncPayload,
    InspectionTemplateVersionSyncPayload, StageOutboxItemInput, SYNC_ENTITY_INSPECTION_CHECKPOINTS,
    SYNC_ENTITY_INSPECTION_ROUNDS, SYNC_ENTITY_INSPECTION_TEMPLATES, SYNC_ENTITY_INSPECTION_TEMPLATE_VERSIONS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    CreateInspectionTemplateInput, InspectionCheckpoint, InspectionCheckpointDraft, InspectionRound,
    InspectionTemplate, InspectionTemplateVersion, InspectionCheckpointsFilter, InspectionTemplateVersionsFilter,
    PublishInspectionTemplateVersionInput, ScheduleInspectionRoundInput,
};

const CHECK_TYPES: &[&str] = &["numeric", "boolean", "observation", "pass_fail"];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode inspection field '{field}': {err}"))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

#[derive(Serialize)]
struct CheckpointPackageEntry {
    sequence_order: i64,
    asset_id: Option<i64>,
    component_id: Option<i64>,
    checkpoint_code: String,
    check_type: String,
    measurement_unit: Option<String>,
    normal_min: Option<f64>,
    normal_max: Option<f64>,
    warning_min: Option<f64>,
    warning_max: Option<f64>,
    requires_photo: bool,
    requires_comment_on_exception: bool,
}

fn package_json_from_drafts(drafts: &[InspectionCheckpointDraft]) -> AppResult<String> {
    let mut v = Vec::with_capacity(drafts.len());
    for d in drafts {
        v.push(CheckpointPackageEntry {
            sequence_order: d.sequence_order,
            asset_id: d.asset_id,
            component_id: d.component_id,
            checkpoint_code: d.checkpoint_code.clone(),
            check_type: d.check_type.clone(),
            measurement_unit: d.measurement_unit.clone(),
            normal_min: d.normal_min,
            normal_max: d.normal_max,
            warning_min: d.warning_min,
            warning_max: d.warning_max,
            requires_photo: d.requires_photo.unwrap_or(false),
            requires_comment_on_exception: d.requires_comment_on_exception.unwrap_or(false),
        });
    }
    Ok(serde_json::to_string(&v)?)
}

fn validate_check_type(t: &str) -> AppResult<()> {
    if CHECK_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "check_type must be one of {:?}",
            CHECK_TYPES
        )]))
    }
}

fn validate_drafts(drafts: &[InspectionCheckpointDraft]) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    for d in drafts {
        if !seen.insert(d.sequence_order) {
            return Err(AppError::ValidationFailed(vec![format!(
                "Duplicate sequence_order {}.",
                d.sequence_order
            )]));
        }
        if d.checkpoint_code.trim().is_empty() {
            return Err(AppError::ValidationFailed(vec!["checkpoint_code required.".into()]));
        }
        validate_check_type(&d.check_type)?;
    }
    Ok(())
}

async fn stage_inspection_template(db: &DatabaseConnection, row: &InspectionTemplate) -> AppResult<()> {
    let payload = InspectionTemplateSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        code: row.code.clone(),
        name: row.name.clone(),
        is_active: row.is_active,
        current_version_id: row.current_version_id,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_templates:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_TEMPLATES.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn stage_inspection_template_version(db: &DatabaseConnection, row: &InspectionTemplateVersion) -> AppResult<()> {
    let payload = InspectionTemplateVersionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        template_id: row.template_id,
        version_no: row.version_no,
        effective_from: row.effective_from.clone(),
        checkpoint_package_json: row.checkpoint_package_json.clone(),
        requires_review: row.requires_review,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "inspection_template_versions:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_INSPECTION_TEMPLATE_VERSIONS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn stage_inspection_checkpoint(db: &DatabaseConnection, row: &InspectionCheckpoint) -> AppResult<()> {
    let payload = InspectionCheckpointSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        template_version_id: row.template_version_id,
        sequence_order: row.sequence_order,
        checkpoint_code: row.checkpoint_code.clone(),
        check_type: row.check_type.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_checkpoints:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_CHECKPOINTS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub(crate) async fn stage_inspection_round(db: &DatabaseConnection, row: &InspectionRound) -> AppResult<()> {
    let payload = InspectionRoundSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        template_version_id: row.template_version_id,
        scheduled_at: row.scheduled_at.clone(),
        status: row.status.clone(),
        assigned_to_id: row.assigned_to_id,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_rounds:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_ROUNDS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

fn map_template(row: &sea_orm::QueryResult) -> AppResult<InspectionTemplate> {
    let is_active_i: i64 = row.try_get("", "is_active").map_err(|e| decode_err("is_active", e))?;
    Ok(InspectionTemplate {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
        org_scope_id: row.try_get("", "org_scope_id").ok(),
        route_scope: row.try_get("", "route_scope").ok(),
        estimated_duration_minutes: row.try_get("", "estimated_duration_minutes").ok(),
        is_active: is_active_i != 0,
        current_version_id: row.try_get("", "current_version_id").ok(),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_version(row: &sea_orm::QueryResult) -> AppResult<InspectionTemplateVersion> {
    let rr: i64 = row
        .try_get("", "requires_review")
        .map_err(|e| decode_err("requires_review", e))?;
    Ok(InspectionTemplateVersion {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        template_id: row.try_get("", "template_id").map_err(|e| decode_err("template_id", e))?,
        version_no: row.try_get("", "version_no").map_err(|e| decode_err("version_no", e))?,
        effective_from: row.try_get("", "effective_from").ok(),
        checkpoint_package_json: row
            .try_get("", "checkpoint_package_json")
            .map_err(|e| decode_err("checkpoint_package_json", e))?,
        tolerance_rules_json: row.try_get("", "tolerance_rules_json").ok(),
        escalation_rules_json: row.try_get("", "escalation_rules_json").ok(),
        requires_review: rr != 0,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_checkpoint(row: &sea_orm::QueryResult) -> AppResult<InspectionCheckpoint> {
    let rp: i64 = row.try_get("", "requires_photo").map_err(|e| decode_err("requires_photo", e))?;
    let rc: i64 = row
        .try_get("", "requires_comment_on_exception")
        .map_err(|e| decode_err("requires_comment_on_exception", e))?;
    Ok(InspectionCheckpoint {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        template_version_id: row
            .try_get("", "template_version_id")
            .map_err(|e| decode_err("template_version_id", e))?,
        sequence_order: row.try_get("", "sequence_order").map_err(|e| decode_err("sequence_order", e))?,
        asset_id: row.try_get("", "asset_id").ok(),
        component_id: row.try_get("", "component_id").ok(),
        checkpoint_code: row
            .try_get("", "checkpoint_code")
            .map_err(|e| decode_err("checkpoint_code", e))?,
        check_type: row.try_get("", "check_type").map_err(|e| decode_err("check_type", e))?,
        measurement_unit: row.try_get("", "measurement_unit").ok(),
        normal_min: row.try_get("", "normal_min").ok(),
        normal_max: row.try_get("", "normal_max").ok(),
        warning_min: row.try_get("", "warning_min").ok(),
        warning_max: row.try_get("", "warning_max").ok(),
        requires_photo: rp != 0,
        requires_comment_on_exception: rc != 0,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_round(row: &sea_orm::QueryResult) -> AppResult<InspectionRound> {
    Ok(InspectionRound {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        template_id: row.try_get("", "template_id").map_err(|e| decode_err("template_id", e))?,
        template_version_id: row
            .try_get("", "template_version_id")
            .map_err(|e| decode_err("template_version_id", e))?,
        scheduled_at: row.try_get("", "scheduled_at").ok(),
        assigned_to_id: row.try_get("", "assigned_to_id").ok(),
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn list_inspection_templates(db: &DatabaseConnection) -> AppResult<Vec<InspectionTemplate>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, org_scope_id, route_scope, estimated_duration_minutes, \
             is_active, current_version_id, row_version \
             FROM inspection_templates ORDER BY code ASC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_template(&r)?);
    }
    Ok(out)
}

pub async fn list_inspection_template_versions(
    db: &DatabaseConnection,
    filter: InspectionTemplateVersionsFilter,
) -> AppResult<Vec<InspectionTemplateVersion>> {
    let (sql, params): (String, Vec<sea_orm::Value>) = if let Some(tid) = filter.template_id {
        (
            "SELECT id, entity_sync_id, template_id, version_no, effective_from, checkpoint_package_json, \
             tolerance_rules_json, escalation_rules_json, requires_review, row_version \
             FROM inspection_template_versions WHERE template_id = ? ORDER BY version_no ASC"
                .to_string(),
            vec![tid.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, template_id, version_no, effective_from, checkpoint_package_json, \
             tolerance_rules_json, escalation_rules_json, requires_review, row_version \
             FROM inspection_template_versions ORDER BY template_id ASC, version_no ASC"
                .to_string(),
            vec![],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_version(&r)?);
    }
    Ok(out)
}

pub async fn list_inspection_checkpoints(
    db: &DatabaseConnection,
    filter: InspectionCheckpointsFilter,
) -> AppResult<Vec<InspectionCheckpoint>> {
    let tid = filter.template_version_id.ok_or_else(|| {
        AppError::ValidationFailed(vec!["template_version_id is required for listing checkpoints.".into()])
    })?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_version_id, sequence_order, asset_id, component_id, \
             checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, warning_max, \
             requires_photo, requires_comment_on_exception, row_version \
             FROM inspection_checkpoints WHERE template_version_id = ? ORDER BY sequence_order ASC",
            [tid.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_checkpoint(&r)?);
    }
    Ok(out)
}

pub async fn list_inspection_rounds(db: &DatabaseConnection) -> AppResult<Vec<InspectionRound>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, template_version_id, scheduled_at, assigned_to_id, status, \
             row_version FROM inspection_rounds ORDER BY id DESC LIMIT 500"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_round(&r)?);
    }
    Ok(out)
}

async fn get_template(db: &DatabaseConnection, id: i64) -> AppResult<Option<InspectionTemplate>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, org_scope_id, route_scope, estimated_duration_minutes, \
             is_active, current_version_id, row_version FROM inspection_templates WHERE id = ?",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|r| map_template(&r)).transpose()?)
}

pub async fn create_inspection_template(
    db: &DatabaseConnection,
    input: CreateInspectionTemplateInput,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["code and name are required.".into()]));
    }
    validate_drafts(&input.checkpoints)?;
    let pkg = package_json_from_drafts(&input.checkpoints)?;
    let is_active = input.is_active.unwrap_or(true);
    let effective = now_rfc3339();
    let template_sync = Uuid::new_v4().to_string();
    let version_sync = Uuid::new_v4().to_string();

    let txn = db.begin().await?;
    let _: ExecResult = txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_templates (entity_sync_id, code, name, org_scope_id, route_scope, \
         estimated_duration_minutes, is_active, current_version_id, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, 1)",
        [
            template_sync.clone().into(),
            input.code.trim().into(),
            input.name.trim().into(),
            opt_i64(input.org_scope_id),
            opt_string(input.route_scope.clone()),
            opt_i64(input.estimated_duration_minutes),
            (if is_active { 1 } else { 0 }).into(),
        ],
    ))
    .await?;
    let tid_row: QueryResult = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let template_id: i64 = tid_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    let _: ExecResult = txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_template_versions (entity_sync_id, template_id, version_no, effective_from, \
         checkpoint_package_json, tolerance_rules_json, escalation_rules_json, requires_review, row_version) \
         VALUES (?, ?, 1, ?, ?, NULL, NULL, 0, 1)",
        [
            version_sync.clone().into(),
            template_id.into(),
            effective.into(),
            pkg.into(),
        ],
    ))
    .await?;
    let vid_row: QueryResult = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let version_id: i64 = vid_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    let mut checkpoints_out = Vec::new();
    for d in &input.checkpoints {
        let cs = Uuid::new_v4().to_string();
        let rp = if d.requires_photo.unwrap_or(false) { 1 } else { 0 };
        let rc = if d.requires_comment_on_exception.unwrap_or(false) {
            1
        } else {
            0
        };
        let _: ExecResult = txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_checkpoints (entity_sync_id, template_version_id, sequence_order, asset_id, \
             component_id, checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, \
             warning_max, requires_photo, requires_comment_on_exception, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                cs.into(),
                version_id.into(),
                d.sequence_order.into(),
                opt_i64(d.asset_id),
                opt_i64(d.component_id),
                d.checkpoint_code.trim().into(),
                d.check_type.clone().into(),
                opt_string(d.measurement_unit.clone()),
                opt_f64(d.normal_min),
                opt_f64(d.normal_max),
                opt_f64(d.warning_min),
                opt_f64(d.warning_max),
                rp.into(),
                rc.into(),
            ],
        ))
        .await?;
        let cid_row: QueryResult = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let cid: i64 = cid_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let row: QueryResult = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, entity_sync_id, template_version_id, sequence_order, asset_id, component_id, \
                 checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, warning_max, \
                 requires_photo, requires_comment_on_exception, row_version \
                 FROM inspection_checkpoints WHERE id = ?",
                [cid.into()],
            ))
            .await?
            .expect("row");
        checkpoints_out.push(map_checkpoint(&row)?);
    }

    let _: ExecResult = txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_templates SET current_version_id = ? WHERE id = ?",
        [version_id.into(), template_id.into()],
    ))
    .await?;
    txn.commit().await?;

    let template = get_template(db, template_id).await?.expect("row");
    let version_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, version_no, effective_from, checkpoint_package_json, \
             tolerance_rules_json, escalation_rules_json, requires_review, row_version \
             FROM inspection_template_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .expect("row");
    let version = map_version(&version_row)?;

    stage_inspection_template(db, &template).await?;
    stage_inspection_template_version(db, &version).await?;
    for c in &checkpoints_out {
        stage_inspection_checkpoint(db, c).await?;
    }
    Ok((template, version, checkpoints_out))
}

pub async fn publish_inspection_template_version(
    db: &DatabaseConnection,
    input: PublishInspectionTemplateVersionInput,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    validate_drafts(&input.checkpoints)?;
    let pkg = package_json_from_drafts(&input.checkpoints)?;
    let current = get_template(db, input.template_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplate".into(),
            id: input.template_id.to_string(),
        })?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_templates.".into()]));
    }
    let max_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_no), 0) AS m FROM inspection_template_versions WHERE template_id = ?",
            [input.template_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("max version")))?;
    let max_v: i64 = max_row.try_get("", "m").map_err(|e| decode_err("m", e))?;
    let next_no = max_v + 1;
    let effective = input.effective_from.unwrap_or_else(now_rfc3339);
    let rr = input.requires_review.unwrap_or(false);
    let version_sync = Uuid::new_v4().to_string();
    let new_tpl_rv = current.row_version + 1;

    let txn = db.begin().await?;
    let _: ExecResult = txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_template_versions (entity_sync_id, template_id, version_no, effective_from, \
         checkpoint_package_json, tolerance_rules_json, escalation_rules_json, requires_review, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            version_sync.into(),
            input.template_id.into(),
            next_no.into(),
            effective.into(),
            pkg.into(),
            opt_string(input.tolerance_rules_json.clone()),
            opt_string(input.escalation_rules_json.clone()),
            (if rr { 1 } else { 0 }).into(),
        ],
    ))
    .await?;
    let vid_row: QueryResult = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let version_id: i64 = vid_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    let mut checkpoints_out = Vec::new();
    for d in &input.checkpoints {
        let cs = Uuid::new_v4().to_string();
        let rp = if d.requires_photo.unwrap_or(false) { 1 } else { 0 };
        let rc = if d.requires_comment_on_exception.unwrap_or(false) {
            1
        } else {
            0
        };
        let _: ExecResult = txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_checkpoints (entity_sync_id, template_version_id, sequence_order, asset_id, \
             component_id, checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, \
             warning_max, requires_photo, requires_comment_on_exception, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                cs.into(),
                version_id.into(),
                d.sequence_order.into(),
                opt_i64(d.asset_id),
                opt_i64(d.component_id),
                d.checkpoint_code.trim().into(),
                d.check_type.clone().into(),
                opt_string(d.measurement_unit.clone()),
                opt_f64(d.normal_min),
                opt_f64(d.normal_max),
                opt_f64(d.warning_min),
                opt_f64(d.warning_max),
                rp.into(),
                rc.into(),
            ],
        ))
        .await?;
        let cid_row: QueryResult = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let cid: i64 = cid_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let row: QueryResult = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, entity_sync_id, template_version_id, sequence_order, asset_id, component_id, \
                 checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, warning_max, \
                 requires_photo, requires_comment_on_exception, row_version \
                 FROM inspection_checkpoints WHERE id = ?",
                [cid.into()],
            ))
            .await?
            .expect("row");
        checkpoints_out.push(map_checkpoint(&row)?);
    }

    let upd: ExecResult = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_templates SET current_version_id = ?, row_version = ? WHERE id = ? AND row_version = ?",
            [
                version_id.into(),
                new_tpl_rv.into(),
                input.template_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if upd.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec!["Concurrent update on inspection_templates.".into()]));
    }
    txn.commit().await?;

    let template = get_template(db, input.template_id).await?.expect("row");
    let version_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, version_no, effective_from, checkpoint_package_json, \
             tolerance_rules_json, escalation_rules_json, requires_review, row_version \
             FROM inspection_template_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .expect("row");
    let version = map_version(&version_row)?;

    stage_inspection_template(db, &template).await?;
    stage_inspection_template_version(db, &version).await?;
    for c in &checkpoints_out {
        stage_inspection_checkpoint(db, c).await?;
    }
    Ok((template, version, checkpoints_out))
}

pub async fn schedule_inspection_round(
    db: &DatabaseConnection,
    input: ScheduleInspectionRoundInput,
) -> AppResult<InspectionRound> {
    let tpl = get_template(db, input.template_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplate".into(),
            id: input.template_id.to_string(),
        })?;
    let version_id = if let Some(v) = input.explicit_template_version_id {
        v
    } else {
        tpl.current_version_id.ok_or_else(|| {
            AppError::ValidationFailed(vec!["Template has no current_version_id; cannot schedule.".into()])
        })?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_template_versions WHERE id = ? AND template_id = ?",
            [version_id.into(), input.template_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "template_version_id does not belong to this template.".into(),
        ]));
    }
    let rs = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_rounds (entity_sync_id, template_id, template_version_id, scheduled_at, \
         assigned_to_id, status, row_version) VALUES (?, ?, ?, ?, ?, 'scheduled', 1)",
        [
            rs.into(),
            input.template_id.into(),
            version_id.into(),
            opt_string(input.scheduled_at.clone()),
            opt_i64(input.assigned_to_id),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let rid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let out_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, template_version_id, scheduled_at, assigned_to_id, status, \
             row_version FROM inspection_rounds WHERE id = ?",
            [rid.into()],
        ))
        .await?
        .expect("row");
    let round = map_round(&out_row)?;
    stage_inspection_round(db, &round).await?;
    Ok(round)
}

pub async fn get_inspection_round_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<InspectionRound>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, template_version_id, scheduled_at, assigned_to_id, status, \
             row_version FROM inspection_rounds WHERE id = ?",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|r| map_round(&r)).transpose()?)
}

pub async fn get_inspection_checkpoint_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<InspectionCheckpoint>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_version_id, sequence_order, asset_id, component_id, \
             checkpoint_code, check_type, measurement_unit, normal_min, normal_max, warning_min, warning_max, \
             requires_photo, requires_comment_on_exception, row_version \
             FROM inspection_checkpoints WHERE id = ?",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|r| map_checkpoint(&r)).transpose()?)
}

pub async fn get_inspection_template_version_by_id(
    db: &DatabaseConnection,
    id: i64,
) -> AppResult<Option<InspectionTemplateVersion>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, template_id, version_no, effective_from, checkpoint_package_json, \
             tolerance_rules_json, escalation_rules_json, requires_review, row_version \
             FROM inspection_template_versions WHERE id = ?",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|r| map_version(&r)).transpose()?)
}
