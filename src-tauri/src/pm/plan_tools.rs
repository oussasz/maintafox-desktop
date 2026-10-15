//! Planner tools around a PM plan: scope pickers and fan-out preview, the asset meters a usage
//! plan counts on (lifetime values, readings with rollover / replacement), and the revision of
//! a plan from a finding.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::assets::meters::{self, MeterReadingKind, RecordMeterReadingPayload};
use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    CreatePmPlanVersionInput, PmAssetMeter, PmPlanVersion, PmScopeAsset, PmScopeOption, RecordPmMeterReadingInput,
};
use crate::pm::generation::resolve_targets;
use crate::pm::queries;

/// Values a plan can be scoped to: equipment families (published reference values), locations
/// (active organisation nodes), criticality groups (`equipment.criticality` lookup).
pub async fn list_pm_scope_options(db: &impl ConnectionTrait, scope_type: &str) -> AppResult<Vec<PmScopeOption>> {
    let sql = match scope_type {
        "family" => {
            "SELECT rv.id, rv.code, rv.label FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE rd.code = 'EQUIPMENT.FAMILY' AND rs.status = 'published' AND rv.is_active = 1
             ORDER BY COALESCE(rv.sort_order, 0), rv.label"
        }
        "location" => {
            "SELECT id, code, name AS label FROM org_nodes
             WHERE deleted_at IS NULL AND status = 'active'
             ORDER BY ancestor_path, code"
        }
        "criticality_group" => {
            "SELECT lv.id, lv.code, lv.label FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'equipment.criticality' AND lv.deleted_at IS NULL
             ORDER BY COALESCE(lv.sort_order, 0), lv.code"
        }
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "No scope options for asset scope type '{other}'."
            )]))
        }
    };
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PmScopeOption {
                id: r.try_get("", "id")?,
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
            })
        })
        .collect()
}

/// The assets a scope covers today (what the forecast fans out to).
pub async fn preview_pm_scope_assets(
    db: &impl ConnectionTrait,
    scope_type: &str,
    scope_id: i64,
) -> AppResult<Vec<PmScopeAsset>> {
    let targets = resolve_targets(db, scope_type, Some(scope_id), None).await?;
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let Some(id) = target.equipment_id else { continue };
        if let Some(row) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT asset_id_code, name FROM equipment WHERE id = ?",
                [id.into()],
            ))
            .await?
        {
            out.push(PmScopeAsset {
                equipment_id: id,
                asset_code: row.try_get("", "asset_id_code")?,
                asset_name: row.try_get("", "name")?,
            });
        }
    }
    Ok(out)
}

/// Active meters of an asset with their lifetime value (raw reading + rollover/replacement offset).
pub async fn list_pm_asset_meters(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Vec<PmAssetMeter>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, meter_type, unit, current_reading, current_reading + reading_offset AS lifetime,
                    rollover_value, last_read_at, is_primary
             FROM equipment_meters WHERE equipment_id = ? AND is_active = 1
             ORDER BY is_primary DESC, name",
            [equipment_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PmAssetMeter {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                meter_type: r.try_get("", "meter_type")?,
                unit: r.try_get("", "unit")?,
                current_reading: r.try_get("", "current_reading")?,
                lifetime_reading: r.try_get("", "lifetime")?,
                rollover_value: r.try_get("", "rollover_value")?,
                last_read_at: r.try_get("", "last_read_at")?,
                is_primary: r.try_get::<i64>("", "is_primary")? == 1,
            })
        })
        .collect()
}

pub fn parse_reading_kind(kind: Option<&str>) -> AppResult<Option<MeterReadingKind>> {
    match kind.map(str::trim).filter(|k| !k.is_empty()) {
        None => Ok(None),
        Some("normal") => Ok(Some(MeterReadingKind::Normal)),
        Some("rollover") => Ok(Some(MeterReadingKind::Rollover)),
        Some("replacement") => Ok(Some(MeterReadingKind::Replacement)),
        Some(other) => Err(AppError::ValidationFailed(vec![format!(
            "Unknown meter reading kind '{other}' (normal, rollover, replacement)."
        )])),
    }
}

/// Records a reading from the PM workspace (manual source) and returns the meter afterwards.
pub async fn record_pm_meter_reading(
    db: &DatabaseConnection,
    input: RecordPmMeterReadingInput,
    actor_id: i32,
) -> AppResult<PmAssetMeter> {
    let kind = parse_reading_kind(input.kind.as_deref())?;
    meters::record_meter_reading_of_kind(
        db,
        RecordMeterReadingPayload {
            meter_id: input.meter_id,
            reading_value: input.reading_value,
            reading_at: input.reading_at,
            source_type: "MANUAL".to_string(),
            source_reference: None,
            quality_flag: None,
        },
        kind,
        actor_id,
    )
    .await?;
    let equipment_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM equipment_meters WHERE id = ?",
            [input.meter_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_meters".into(),
            id: input.meter_id.to_string(),
        })?
        .try_get("", "equipment_id")?;
    list_pm_asset_meters(db, equipment_id)
        .await?
        .into_iter()
        .find(|m| m.id == input.meter_id)
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_meters".into(),
            id: input.meter_id.to_string(),
        })
}

/// "Revise the plan" from a finding: a draft version copied from the plan's current version
/// (else its latest), with the finding as change reason and as source (`source_type =
/// 'pm_finding'`). The normal approval / publish flow applies. Asking twice for the same finding
/// returns the draft already created.
pub async fn create_pm_revision_from_finding(db: &DatabaseConnection, finding_id: i64) -> AppResult<PmPlanVersion> {
    let finding = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pf.description, po.pm_plan_id
             FROM pm_findings pf
             JOIN pm_executions pe ON pe.id = pf.pm_execution_id
             JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
             WHERE pf.id = ?",
            [finding_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_finding".into(),
            id: finding_id.to_string(),
        })?;
    let description: String = finding.try_get("", "description")?;
    let plan_id: i64 = finding.try_get("", "pm_plan_id")?;
    let plan = queries::get_pm_plan(db, plan_id).await?;
    if plan.lifecycle_status == "retired" {
        return Err(AppError::ValidationFailed(vec![
            "A retired PM plan cannot be revised.".to_string()
        ]));
    }
    if let Some(existing) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM pm_plan_versions
             WHERE pm_plan_id = ? AND source_type = 'pm_finding' AND source_id = ? AND status = 'draft'
             ORDER BY id DESC LIMIT 1",
            [plan_id.into(), finding_id.into()],
        ))
        .await?
    {
        let id: i64 = existing.try_get("", "id")?;
        return queries::list_pm_plan_versions(db, plan_id)
            .await?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("draft revision not found")));
    }
    let versions = queries::list_pm_plan_versions(db, plan_id).await?;
    let base = plan
        .current_version_id
        .and_then(|id| versions.iter().find(|v| v.id == id))
        .or_else(|| versions.iter().max_by_key(|v| v.version_no))
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "The plan has no version to revise; create a version first.".to_string()
            ])
        })?;
    let effective_from = Utc::now().format("%Y-%m-%dT00:00:00Z").to_string();
    let draft = queries::create_pm_plan_version(
        db,
        plan_id,
        CreatePmPlanVersionInput {
            effective_from,
            effective_to: None,
            trigger_definition_json: base.trigger_definition_json.clone(),
            task_package_json: base.task_package_json.clone(),
            required_parts_json: base.required_parts_json.clone(),
            required_skills_json: base.required_skills_json.clone(),
            required_tools_json: base.required_tools_json.clone(),
            estimated_duration_hours: base.estimated_duration_hours,
            planned_crew_size: base.planned_crew_size,
            estimated_labor_cost: base.estimated_labor_cost,
            estimated_parts_cost: base.estimated_parts_cost,
            estimated_service_cost: base.estimated_service_cost,
            change_reason: Some(description.trim().to_string()),
        },
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_plan_versions SET source_type = 'pm_finding', source_id = ? WHERE id = ?",
        [finding_id.into(), draft.id.into()],
    ))
    .await?;
    Ok(draft)
}
