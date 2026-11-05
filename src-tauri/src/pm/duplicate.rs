//! "Dupliquer ce plan" (owner audit L6-11, CMMS Advisor decision).
//!
//! A new plan, version 1 in draft, copied from the source plan's current published version (else
//! its latest draft) onto a target asset or scope; a planner publishes it through the normal PM
//! versioning. Everything is written in one transaction with an audit entry
//! "copié de <code> v<n>" on the new plan.
//!
//! - **Copied**: strategy and trigger (interval, unit, tolerance), task package (order, mandatory
//!   flag, result type, limits — the package as written), part lines, tools, skills, estimated
//!   duration, crew size, assigned crew, description, shutdown and permit flags.
//! - **Not copied**: occurrences, generated work orders, executions, readings (`start_reading`),
//!   costs (estimated costs included), version history, approvals and publish state (the new plan
//!   is a draft), the criticality (it follows the target asset), the audit trail.
//! - **Meter**: a trigger on a chosen asset meter keeps a meter only when the person picks a meter
//!   of the target asset with the same unit; otherwise the meter is left to choose and publishing
//!   is refused until it is (`GATE_PM_METER_REQUIRED`, `queries::ensure_meter_chosen`).
//! - **Right**: `pm.create` at the target's site ([`target_anchor`], checked by the command).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};

use crate::auth::scope::Anchor;
use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, DuplicatePmPlanInput, DuplicatePmPlanResult, PmPlan, PmPlanVersion,
};
use crate::pm::queries;

fn refused(message: String) -> AppError {
    AppError::ValidationFailed(vec![message])
}

fn target_unknown() -> AppError {
    refused("GATE_PM_COPY_TARGET_UNKNOWN: The target asset or scope of the copy does not exist.".to_string())
}

/// The target scope of the copy: the requested one, else the source plan's.
pub fn resolve_target(source: &PmPlan, input: &DuplicatePmPlanInput) -> AppResult<(String, i64)> {
    let scope_type = input
        .target_scope_type
        .as_deref()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| source.asset_scope_type.clone());
    let scope_id = input
        .target_scope_id
        .or(if scope_type == source.asset_scope_type {
            source.asset_scope_id
        } else {
            None
        })
        .ok_or_else(target_unknown)?;
    Ok((scope_type, scope_id))
}

/// The org node the copy is checked at: the target asset's installation node, the target
/// location itself; a family or criticality group spans the plant (plant-wide right only).
/// Refuses a target that does not exist.
pub async fn target_anchor(db: &impl ConnectionTrait, scope_type: &str, scope_id: i64) -> AppResult<Anchor> {
    let (sql, plant_wide) = match scope_type {
        "equipment" => (
            "SELECT installed_at_node_id AS node FROM equipment WHERE id = ? AND deleted_at IS NULL",
            false,
        ),
        "location" => (
            "SELECT id AS node FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            false,
        ),
        "family" => ("SELECT NULL AS node FROM reference_values WHERE id = ?", true),
        "criticality_group" => (
            "SELECT NULL AS node FROM lookup_values WHERE id = ? AND deleted_at IS NULL",
            true,
        ),
        other => {
            return Err(refused(format!(
                "GATE_PM_SCOPE_TYPE_INVALID: Unsupported PM asset scope type '{other}'."
            )))
        }
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [scope_id.into()],
        ))
        .await?
        .ok_or_else(target_unknown)?;
    if plant_wide {
        return Ok(Anchor::Plant);
    }
    Ok(Anchor::of(row.try_get::<Option<i64>>("", "node")?))
}

/// The version copied: the current published version, else the latest draft.
async fn source_version(db: &DatabaseConnection, source: &PmPlan) -> AppResult<PmPlanVersion> {
    let versions = queries::list_pm_plan_versions(db, source.id).await?;
    let current = source
        .current_version_id
        .and_then(|id| versions.iter().find(|v| v.id == id && v.status == "published"));
    current
        .or_else(|| {
            versions
                .iter()
                .filter(|v| v.status == "draft")
                .max_by_key(|v| (v.version_no, v.id))
        })
        .cloned()
        .ok_or_else(|| {
            refused(
                "GATE_PM_NO_VERSION_TO_COPY: The plan has neither a published version nor a draft to copy.".to_string(),
            )
        })
}

/// The meter spec of a trigger, mutable (see `queries::meter_spec_of`).
fn meter_spec_mut<'a>(
    strategy_type: &str,
    trigger: &'a mut serde_json::Map<String, Value>,
) -> Option<&'a mut serde_json::Map<String, Value>> {
    match strategy_type {
        "meter" => Some(trigger),
        "fixed" | "floating" => trigger.get_mut("meter").and_then(Value::as_object_mut),
        _ => None,
    }
}

/// Unit of a meter (`None`: the meter has no unit, or does not exist).
async fn meter_unit(db: &impl ConnectionTrait, meter_id: i64) -> AppResult<Option<i64>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_value_id FROM equipment_meters WHERE id = ?",
            [meter_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<Option<i64>>("", "unit_value_id"))
        .transpose()?
        .flatten())
}

/// The trigger of the copy and whether its meter is left to choose. Readings (`start_reading`)
/// are never copied; a chosen asset meter is replaced by `target_meter_id` (an active meter of
/// the target asset with the same unit) or removed.
async fn copied_trigger(
    db: &impl ConnectionTrait,
    strategy_type: &str,
    source_trigger_json: &str,
    target: (&str, i64),
    target_meter_id: Option<i64>,
) -> AppResult<(String, bool)> {
    let mut trigger = match serde_json::from_str::<Value>(source_trigger_json) {
        Ok(Value::Object(obj)) => obj,
        _ => {
            return Err(refused(
                "GATE_PM_DEFINITION_INVALID: trigger_definition_json must be an object.".to_string(),
            ))
        }
    };
    let mut meter_to_choose = false;
    match meter_spec_mut(strategy_type, &mut trigger) {
        Some(spec) => {
            spec.remove("start_reading");
            let source_meter = spec.get("asset_meter_id").and_then(Value::as_i64).filter(|id| *id > 0);
            match (source_meter, target_meter_id) {
                (Some(source_meter), Some(meter_id)) => {
                    let (scope_type, equipment_id) = target;
                    let unit = meter_unit(db, source_meter).await?;
                    let fits = scope_type == "equipment"
                        && db
                            .query_one(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                "SELECT 1 AS ok FROM equipment_meters
                                 WHERE id = ? AND equipment_id = ? AND is_active = 1 AND unit_value_id IS ?",
                                [meter_id.into(), equipment_id.into(), unit.into()],
                            ))
                            .await?
                            .is_some();
                    if !fits {
                        return Err(refused(
                            "GATE_PM_COPY_METER_MISMATCH: The chosen meter is not an active meter of the target asset with the unit of the source meter."
                                .to_string(),
                        ));
                    }
                    spec.insert("asset_meter_id".to_string(), json!(meter_id));
                }
                (Some(_), None) => {
                    spec.remove("asset_meter_id");
                    meter_to_choose = true;
                }
                (None, Some(_)) => return Err(meter_not_expected()),
                (None, None) => {}
            }
        }
        None if target_meter_id.is_some() => return Err(meter_not_expected()),
        None => {}
    }
    Ok((Value::Object(trigger).to_string(), meter_to_choose))
}

fn meter_not_expected() -> AppError {
    refused("GATE_PM_COPY_METER_NOT_EXPECTED: This plan is not triggered by a chosen asset meter.".to_string())
}

/// Creates the copy (see the module docs). The caller has checked `pm.create` at
/// [`target_anchor`]; `actor_id` is the session user (audit).
pub async fn duplicate_pm_plan(
    db: &DatabaseConnection,
    input: DuplicatePmPlanInput,
    actor_id: Option<i64>,
) -> AppResult<DuplicatePmPlanResult> {
    let source = queries::get_pm_plan(db, input.source_plan_id).await?;
    let base = source_version(db, &source).await?;
    let (scope_type, scope_id) = resolve_target(&source, &input)?;
    target_anchor(db, &scope_type, scope_id).await?;

    let code = queries::normalize_plan_code(&input.code);
    let taken = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS taken FROM pm_plans WHERE code = ?",
            [code.clone().into()],
        ))
        .await?
        .is_some();
    if taken {
        return Err(refused(format!(
            "GATE_PM_PLAN_CODE_TAKEN: Another PM plan already uses the code '{code}'."
        )));
    }
    let (trigger_definition_json, meter_to_choose) = copied_trigger(
        db,
        &source.strategy_type,
        &base.trigger_definition_json,
        (&scope_type, scope_id),
        input.target_meter_id,
    )
    .await?;
    let copied_from = format!("copié de {} v{}", source.code, base.version_no);

    let tx = db.begin().await?;
    let plan_id = queries::create_pm_plan_in(
        &tx,
        CreatePmPlanInput {
            code: code.clone(),
            title: input.title,
            description: source.description.clone(),
            asset_scope_type: scope_type,
            asset_scope_id: Some(scope_id),
            strategy_type: source.strategy_type.clone(),
            criticality_value_id: None,
            assigned_team_id: source.assigned_team_id,
            requires_shutdown: source.requires_shutdown == 1,
            requires_permit: source.requires_permit == 1,
            is_active: None,
        },
    )
    .await?;
    let version = queries::create_pm_plan_version_in_with(
        &tx,
        plan_id,
        CreatePmPlanVersionInput {
            // The anchor of a calendar plan: today, as a revision draft (the planner sets it
            // before publishing); no date of the source plan is carried over.
            effective_from: crate::clock::now().format("%Y-%m-%dT00:00:00Z").to_string(),
            effective_to: None,
            trigger_definition_json,
            task_package_json: base.task_package_json.clone(),
            required_parts_json: crate::pm::wo_content::normalized_required_parts_json(
                base.required_parts_json.as_deref(),
            ),
            required_skills_json: base.required_skills_json.clone(),
            required_tools_json: base.required_tools_json.clone(),
            estimated_duration_hours: base.estimated_duration_hours,
            planned_crew_size: base.planned_crew_size,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: Some(copied_from.clone()),
        },
        meter_to_choose,
    )
    .await?;
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "pm.plan.duplicated".to_string(),
            target_type: Some("pm_plans".to_string()),
            target_id: Some(plan_id.to_string()),
            actor_id,
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(json!({
                "summary": copied_from,
                "plan_code": code,
                "source_plan_code": source.code,
                "source_version_no": base.version_no,
                "meter_to_choose": meter_to_choose,
            })),
        },
    )
    .await?;
    tx.commit().await?;

    Ok(DuplicatePmPlanResult {
        plan: queries::get_pm_plan(db, plan_id).await?,
        version,
        meter_to_choose,
    })
}
