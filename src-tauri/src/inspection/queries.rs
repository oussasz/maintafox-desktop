use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, ExecResult, QueryResult, Statement, TransactionTrait};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

use super::domain::{
    CreateInspectionTemplateInput, InspectionCheckpoint, InspectionCheckpointDraft, InspectionCheckpointsFilter,
    InspectionRound, InspectionTemplate, InspectionTemplateVersion, InspectionTemplateVersionsFilter,
    PublishInspectionTemplateVersionInput, ScheduleInspectionRoundInput,
};

pub(crate) const CHECK_TYPES: &[&str] = &["numeric", "boolean", "observation", "pass_fail"];

/// Checkpoint columns; the unit symbol is read from `MEASURE.UNITS` (`measurement_unit`).
pub(crate) const CHECKPOINT_COLUMNS: &str = concat!(
    "id, entity_sync_id, template_version_id, sequence_order, asset_id, component_id, checkpoint_code, \
     description, check_type, measurement_unit_id, ",
    crate::reference::units::unit_symbol_of!("measurement_unit_id"),
    " AS measurement_unit, normal_min, normal_max, warning_min, warning_max, requires_photo, \
     requires_comment_on_exception, row_version"
);

pub(crate) const ROUND_COLUMNS: &str = "id, entity_sync_id, template_id, template_version_id, scheduled_at,      assigned_to_id, status, started_at, completed_at, reviewed_at, reviewed_by_id, review_note,      completion_percentage, row_version, schedule_id, due_basis, window_end_at, assigned_team_id,      due_meter_value, missed_at";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode inspection field '{field}': {err}"))
}

fn now_rfc3339() -> String {
    crate::clock::now().to_rfc3339()
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
    description: Option<String>,
    check_type: String,
    measurement_unit_id: Option<i64>,
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
            description: trimmed(d.description.as_deref()),
            check_type: d.check_type.clone(),
            measurement_unit_id: d.measurement_unit_id,
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

fn gate(code: &str, detail: String) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {detail}")])
}

fn validate_check_type(code: &str, t: &str) -> AppResult<()> {
    if CHECK_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(gate(
            "GATE_INSP_CHECK_TYPE_INVALID",
            format!("{code}: check_type must be one of {CHECK_TYPES:?}."),
        ))
    }
}

/// Checkpoints of a template version. Each failure names the checkpoint (position and code) and
/// carries a stable `GATE_INSP_CHECK_*` code, so the template editor can point at the row.
fn validate_drafts(drafts: &[InspectionCheckpointDraft]) -> AppResult<()> {
    if drafts.is_empty() {
        return Err(gate(
            "GATE_INSP_TEMPLATE_NO_CHECK",
            "a checklist needs at least one checkpoint.".into(),
        ));
    }
    let mut seen_orders = std::collections::HashSet::new();
    let mut seen_codes = std::collections::HashSet::new();
    for (i, d) in drafts.iter().enumerate() {
        let position = i + 1;
        if !seen_orders.insert(d.sequence_order) {
            return Err(gate(
                "GATE_INSP_CHECK_SEQUENCE_DUPLICATE",
                format!("checkpoint {position}: sequence {} is used twice.", d.sequence_order),
            ));
        }
        let code = d.checkpoint_code.trim();
        if code.is_empty() {
            return Err(gate(
                "GATE_INSP_CHECK_CODE_REQUIRED",
                format!("checkpoint {position}: the code is required."),
            ));
        }
        // History and trends follow a checkpoint by its code across versions.
        if !seen_codes.insert(code.to_uppercase()) {
            return Err(gate(
                "GATE_INSP_CHECK_CODE_DUPLICATE",
                format!("checkpoint {position}: the code {code} is used twice."),
            ));
        }
        validate_check_type(code, &d.check_type)?;
        validate_limits(d)?;
    }
    Ok(())
}

/// Numeric limits: each band is ordered and the warning band contains the normal band, so a
/// reading is classified the same way whatever side of the range it falls on.
fn validate_limits(d: &InspectionCheckpointDraft) -> AppResult<()> {
    let code = d.checkpoint_code.trim();
    let limits = [d.normal_min, d.normal_max, d.warning_min, d.warning_max];
    if d.check_type != "numeric" {
        if limits.iter().any(Option::is_some) {
            return Err(gate(
                "GATE_INSP_CHECK_LIMITS_NOT_NUMERIC",
                format!("{code}: limits are only allowed on numeric checkpoints."),
            ));
        }
        return Ok(());
    }
    if limits.into_iter().flatten().any(|v| !v.is_finite()) {
        return Err(gate(
            "GATE_INSP_CHECK_LIMITS_NOT_FINITE",
            format!("{code}: limits must be finite numbers."),
        ));
    }
    let ordered = |lo: Option<f64>, hi: Option<f64>| lo.zip(hi).is_none_or(|(l, h)| l <= h);
    if !ordered(d.normal_min, d.normal_max) || !ordered(d.warning_min, d.warning_max) {
        return Err(gate(
            "GATE_INSP_CHECK_LIMITS_ORDER",
            format!("{code}: a minimum is greater than its maximum."),
        ));
    }
    let wider_low = d.warning_min.is_none_or(|w| d.normal_min.is_some_and(|n| w <= n));
    let wider_high = d.warning_max.is_none_or(|w| d.normal_max.is_some_and(|n| w >= n));
    if !wider_low || !wider_high {
        return Err(gate(
            "GATE_INSP_CHECK_WARNING_BAND",
            format!("{code}: the warning band must contain the normal band."),
        ));
    }
    Ok(())
}

/// Inserts the checkpoints of a template version (create and publish share it).
async fn insert_checkpoints(
    txn: &impl ConnectionTrait,
    version_id: i64,
    drafts: &[InspectionCheckpointDraft],
) -> AppResult<Vec<InspectionCheckpoint>> {
    let mut out = Vec::with_capacity(drafts.len());
    for d in drafts {
        if let Some(unit_id) = d.measurement_unit_id {
            crate::reference::units::ensure_unit(
                txn,
                unit_id,
                &format!("checkpoint {}: measurement_unit_id", d.checkpoint_code.trim()),
                true,
            )
            .await?;
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_checkpoints (entity_sync_id, template_version_id, sequence_order, asset_id, \
             component_id, checkpoint_code, description, check_type, measurement_unit_id, normal_min, normal_max, \
             warning_min, warning_max, requires_photo, requires_comment_on_exception, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                Uuid::new_v4().to_string().into(),
                version_id.into(),
                d.sequence_order.into(),
                opt_i64(d.asset_id),
                opt_i64(d.component_id),
                d.checkpoint_code.trim().into(),
                opt_string(trimmed(d.description.as_deref())),
                d.check_type.clone().into(),
                opt_i64(d.measurement_unit_id),
                opt_f64(d.normal_min),
                opt_f64(d.normal_max),
                opt_f64(d.warning_min),
                opt_f64(d.warning_max),
                i64::from(d.requires_photo.unwrap_or(false)).into(),
                i64::from(d.requires_comment_on_exception.unwrap_or(false)).into(),
            ],
        ))
        .await?;
        let row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT {CHECKPOINT_COLUMNS} FROM inspection_checkpoints WHERE id = last_insert_rowid()"),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("inserted checkpoint not found")))?;
        out.push(map_checkpoint(&row)?);
    }
    Ok(out)
}

fn trimmed(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
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
        org_scope_id: row
            .try_get("", "org_scope_id")
            .map_err(|e| decode_err("org_scope_id", e))?,
        route_scope: row
            .try_get("", "route_scope")
            .map_err(|e| decode_err("route_scope", e))?,
        estimated_duration_minutes: row
            .try_get("", "estimated_duration_minutes")
            .map_err(|e| decode_err("estimated_duration_minutes", e))?,
        is_active: is_active_i != 0,
        current_version_id: row
            .try_get("", "current_version_id")
            .map_err(|e| decode_err("current_version_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
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
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| decode_err("template_id", e))?,
        version_no: row.try_get("", "version_no").map_err(|e| decode_err("version_no", e))?,
        effective_from: row
            .try_get("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        checkpoint_package_json: row
            .try_get("", "checkpoint_package_json")
            .map_err(|e| decode_err("checkpoint_package_json", e))?,
        tolerance_rules_json: row
            .try_get("", "tolerance_rules_json")
            .map_err(|e| decode_err("tolerance_rules_json", e))?,
        escalation_rules_json: row
            .try_get("", "escalation_rules_json")
            .map_err(|e| decode_err("escalation_rules_json", e))?,
        requires_review: rr != 0,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_checkpoint(row: &sea_orm::QueryResult) -> AppResult<InspectionCheckpoint> {
    let rp: i64 = row
        .try_get("", "requires_photo")
        .map_err(|e| decode_err("requires_photo", e))?;
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
        sequence_order: row
            .try_get("", "sequence_order")
            .map_err(|e| decode_err("sequence_order", e))?,
        asset_id: row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?,
        component_id: row
            .try_get("", "component_id")
            .map_err(|e| decode_err("component_id", e))?,
        checkpoint_code: row
            .try_get("", "checkpoint_code")
            .map_err(|e| decode_err("checkpoint_code", e))?,
        description: row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?,
        check_type: row.try_get("", "check_type").map_err(|e| decode_err("check_type", e))?,
        measurement_unit_id: row
            .try_get("", "measurement_unit_id")
            .map_err(|e| decode_err("measurement_unit_id", e))?,
        measurement_unit: row
            .try_get("", "measurement_unit")
            .map_err(|e| decode_err("measurement_unit", e))?,
        normal_min: row.try_get("", "normal_min").map_err(|e| decode_err("normal_min", e))?,
        normal_max: row.try_get("", "normal_max").map_err(|e| decode_err("normal_max", e))?,
        warning_min: row
            .try_get("", "warning_min")
            .map_err(|e| decode_err("warning_min", e))?,
        warning_max: row
            .try_get("", "warning_max")
            .map_err(|e| decode_err("warning_max", e))?,
        requires_photo: rp != 0,
        requires_comment_on_exception: rc != 0,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

pub(crate) fn map_round(row: &sea_orm::QueryResult) -> AppResult<InspectionRound> {
    Ok(InspectionRound {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| decode_err("template_id", e))?,
        template_version_id: row
            .try_get("", "template_version_id")
            .map_err(|e| decode_err("template_version_id", e))?,
        scheduled_at: row
            .try_get("", "scheduled_at")
            .map_err(|e| decode_err("scheduled_at", e))?,
        assigned_to_id: row
            .try_get("", "assigned_to_id")
            .map_err(|e| decode_err("assigned_to_id", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        started_at: row.try_get("", "started_at").map_err(|e| decode_err("started_at", e))?,
        completed_at: row
            .try_get("", "completed_at")
            .map_err(|e| decode_err("completed_at", e))?,
        reviewed_at: row
            .try_get("", "reviewed_at")
            .map_err(|e| decode_err("reviewed_at", e))?,
        reviewed_by_id: row
            .try_get("", "reviewed_by_id")
            .map_err(|e| decode_err("reviewed_by_id", e))?,
        review_note: row
            .try_get("", "review_note")
            .map_err(|e| decode_err("review_note", e))?,
        completion_percentage: row
            .try_get("", "completion_percentage")
            .map_err(|e| decode_err("completion_percentage", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        schedule_id: row
            .try_get("", "schedule_id")
            .map_err(|e| decode_err("schedule_id", e))?,
        due_basis: row.try_get("", "due_basis").map_err(|e| decode_err("due_basis", e))?,
        window_end_at: row
            .try_get("", "window_end_at")
            .map_err(|e| decode_err("window_end_at", e))?,
        assigned_team_id: row
            .try_get("", "assigned_team_id")
            .map_err(|e| decode_err("assigned_team_id", e))?,
        due_meter_value: row
            .try_get("", "due_meter_value")
            .map_err(|e| decode_err("due_meter_value", e))?,
        missed_at: row.try_get("", "missed_at").map_err(|e| decode_err("missed_at", e))?,
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
    db: &impl ConnectionTrait,
    filter: InspectionCheckpointsFilter,
) -> AppResult<Vec<InspectionCheckpoint>> {
    let tid = filter.template_version_id.ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "GATE_INSP_LIST_FILTER_REQUIRED: template_version_id is required for listing checkpoints.".into(),
        ])
    })?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CHECKPOINT_COLUMNS} FROM inspection_checkpoints WHERE template_version_id = ? ORDER BY sequence_order ASC"),
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
            format!("SELECT {ROUND_COLUMNS} FROM inspection_rounds ORDER BY id DESC LIMIT 500"),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_round(&r)?);
    }
    Ok(out)
}

async fn get_template(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<InspectionTemplate>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, org_scope_id, route_scope, estimated_duration_minutes, \
             is_active, current_version_id, row_version FROM inspection_templates WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_template(&r)).transpose()
}

pub async fn create_inspection_template(
    db: &DatabaseConnection,
    input: CreateInspectionTemplateInput,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_TEMPLATE_CODE_NAME_REQUIRED: code and name are required.".into(),
        ]));
    }
    validate_drafts(&input.checkpoints)?;
    let pkg = package_json_from_drafts(&input.checkpoints)?;
    let is_active = input.is_active.unwrap_or(true);
    let effective = now_rfc3339();
    let template_sync = Uuid::new_v4().to_string();
    let version_sync = Uuid::new_v4().to_string();

    let txn = db.begin().await?;
    let _: ExecResult = txn
        .execute(Statement::from_sql_and_values(
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

    let _: ExecResult = txn
        .execute(Statement::from_sql_and_values(
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

    let checkpoints_out = insert_checkpoints(&txn, version_id, &input.checkpoints).await?;

    let _: ExecResult = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_templates SET current_version_id = ? WHERE id = ?",
            [version_id.into(), template_id.into()],
        ))
        .await?;
    let template = get_template(&txn, template_id).await?.expect("row");
    let version_row = txn
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

    txn.commit().await?;
    Ok((template, version, checkpoints_out))
}

pub async fn publish_inspection_template_version(
    db: &DatabaseConnection,
    input: PublishInspectionTemplateVersionInput,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    validate_drafts(&input.checkpoints)?;
    super::results::parse_escalation(input.escalation_rules_json.as_deref())?;
    let pkg = package_json_from_drafts(&input.checkpoints)?;
    let current = get_template(db, input.template_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplate".into(),
            id: input.template_id.to_string(),
        })?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on inspection_templates.".into(),
        ]));
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
    let _: ExecResult = txn
        .execute(Statement::from_sql_and_values(
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

    let checkpoints_out = insert_checkpoints(&txn, version_id, &input.checkpoints).await?;

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
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Concurrent update on inspection_templates.".into(),
        ]));
    }
    let template = get_template(&txn, input.template_id).await?.expect("row");
    let version_row = txn
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

    txn.commit().await?;
    Ok((template, version, checkpoints_out))
}

pub async fn schedule_inspection_round(
    db: &DatabaseConnection,
    input: ScheduleInspectionRoundInput,
) -> AppResult<InspectionRound> {
    let scheduled_at = match trimmed(input.scheduled_at.as_deref()) {
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map_err(|_| {
                    AppError::ValidationFailed(vec![
                        "GATE_INSP_DATE_INVALID: scheduled_at must be an RFC 3339 timestamp.".into(),
                    ])
                })?
                .with_timezone(&Utc)
                .to_rfc3339(),
        ),
        None => None,
    };
    let txn = db.begin().await?;
    let tpl = get_template(&txn, input.template_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplate".into(),
            id: input.template_id.to_string(),
        })?;
    let version_id = if let Some(v) = input.explicit_template_version_id {
        v
    } else {
        tpl.current_version_id.ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INSP_TEMPLATE_NO_VERSION: Template has no current_version_id; cannot schedule.".into(),
            ])
        })?
    };
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_template_versions WHERE id = ? AND template_id = ?",
            [version_id.into(), input.template_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_VERSION_MISMATCH: template_version_id does not belong to this template.".into(),
        ]));
    }
    let rs = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_rounds (entity_sync_id, template_id, template_version_id, scheduled_at, \
         assigned_to_id, assigned_team_id, status, row_version) VALUES (?, ?, ?, ?, ?, ?, 'scheduled', 1)",
        [
            rs.into(),
            input.template_id.into(),
            version_id.into(),
            opt_string(scheduled_at),
            opt_i64(input.assigned_to_id),
            opt_i64(input.assigned_team_id),
        ],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let rid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let out_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {ROUND_COLUMNS} FROM inspection_rounds WHERE id = ?"),
            [rid.into()],
        ))
        .await?
        .expect("row");
    let round = map_round(&out_row)?;
    txn.commit().await?;
    Ok(round)
}

pub async fn get_inspection_round_by_id(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<InspectionRound>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {ROUND_COLUMNS} FROM inspection_rounds WHERE id = ?"),
            [id.into()],
        ))
        .await?;
    row.map(|r| map_round(&r)).transpose()
}

pub async fn get_inspection_checkpoint_by_id(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<Option<InspectionCheckpoint>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CHECKPOINT_COLUMNS} FROM inspection_checkpoints WHERE id = ?"),
            [id.into()],
        ))
        .await?;
    row.map(|r| map_checkpoint(&r)).transpose()
}

pub async fn get_inspection_template_version_by_id(
    db: &impl ConnectionTrait,
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
    row.map(|r| map_version(&r)).transpose()
}
