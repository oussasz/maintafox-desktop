//! What a planned job asks of the technician named on it (SAP PM: qualifications of the work
//! centre / operation; Maximo: crafts and qualifications of the job plan), for every kind of job
//! in the backlog — not only work orders.
//!
//! - Work order: its own requirements, its type's and its permits' (`qualification::gate`), with
//!   the overrides recorded on it.
//! - PM occurrence: the work order it generated once there is one; before that, the requirements
//!   of the preventive work-order type it will be generated with and the skills its plan version
//!   lists.
//! - Request (DI) approved for planning: the requirements of the work-order type it becomes on
//!   conversion (its request type's maintenance nature).
//!
//! Overrides are recorded on work orders only: a PM occurrence or a request is refused until the
//! technician is qualified, or the work order exists and a supervisor overrides there.

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::qualification::gate::{self, JobRequirement, PersonQualificationCheck};
use crate::wo::types::MaintenanceNature;

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("planning qualification decode '{field}': {err}"))
}

/// The requirements of a job: a work order's (checked with its overrides) or a list.
enum JobRequirements {
    WorkOrder(i64),
    List(Vec<JobRequirement>),
}

fn skill_codes(json: Option<&str>) -> AppResult<Vec<String>> {
    let Some(raw) = json.filter(|s| !s.trim().is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<String>>(raw)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PM required_skills_json is not a list of codes: {e}")))
}

async fn job_requirements(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
) -> AppResult<Option<JobRequirements>> {
    match source_type {
        "work_order" => Ok(Some(JobRequirements::WorkOrder(source_id))),
        "pm_occurrence" => {
            let Some(row) = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT po.linked_work_order_id, pp.code AS plan_code, pv.required_skills_json \
                       FROM pm_occurrences po \
                       JOIN pm_plans pp ON pp.id = po.pm_plan_id \
                       LEFT JOIN pm_plan_versions pv ON pv.id = po.plan_version_id \
                      WHERE po.id = ?",
                    [source_id.into()],
                ))
                .await?
            else {
                return Ok(None);
            };
            let linked: Option<i64> = row
                .try_get("", "linked_work_order_id")
                .map_err(|e| decode_err("linked_work_order_id", e))?;
            if let Some(wo_id) = linked {
                return Ok(Some(JobRequirements::WorkOrder(wo_id)));
            }
            let plan_code: String = row.try_get("", "plan_code").map_err(|e| decode_err("plan_code", e))?;
            let skills_json: Option<String> = row
                .try_get("", "required_skills_json")
                .map_err(|e| decode_err("required_skills_json", e))?;
            // The type a PM generates its work orders with (time-based preventive).
            let wo_type =
                crate::wo::types::default_type_for_nature(db, MaintenanceNature::PreventivePredetermined).await?;
            let mut reqs = gate::work_order_type_requirements(db, wo_type.id).await?;
            reqs.extend(
                gate::skill_code_requirements(db, "pm_plan", Some(&plan_code), &skill_codes(skills_json.as_deref())?)
                    .await?,
            );
            Ok(Some(JobRequirements::List(reqs)))
        }
        "inspection_follow_up" => {
            let Some(row) = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT request_type, COALESCE(validated_urgency, reported_urgency) AS urgency, converted_to_wo_id \
                       FROM intervention_requests WHERE id = ?",
                    [source_id.into()],
                ))
                .await?
            else {
                return Ok(None);
            };
            let converted: Option<i64> = row
                .try_get("", "converted_to_wo_id")
                .map_err(|e| decode_err("converted_to_wo_id", e))?;
            if let Some(wo_id) = converted {
                return Ok(Some(JobRequirements::WorkOrder(wo_id)));
            }
            let request_type: String = row
                .try_get("", "request_type")
                .map_err(|e| decode_err("request_type", e))?;
            let urgency: Option<String> = row.try_get("", "urgency").map_err(|e| decode_err("urgency", e))?;
            // Only the emergency level changes the nature of an untyped request (as on conversion).
            let urgency = urgency
                .as_deref()
                .and_then(crate::reference::urgency_scale::UrgencyCode::parse)
                .unwrap_or(crate::reference::urgency_scale::UrgencyCode::Normal);
            let nature = crate::di::conversion::conversion_nature(db, &request_type, urgency).await?;
            let wo_type = crate::wo::types::default_type_for_nature(db, nature).await?;
            Ok(Some(JobRequirements::List(
                gate::work_order_type_requirements(db, wo_type.id).await?,
            )))
        }
        _ => Ok(None),
    }
}

/// The technician against what the job asks, valid on `work_day` (the slot's first day); `None`
/// when the job is unknown.
pub async fn check_assignee(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
    personnel_id: i64,
    work_day: NaiveDate,
) -> AppResult<Option<PersonQualificationCheck>> {
    match job_requirements(db, source_type, source_id).await? {
        None => Ok(None),
        Some(JobRequirements::WorkOrder(wo_id)) => {
            Ok(Some(gate::check_person_for_work_order(db, personnel_id, wo_id).await?))
        }
        Some(JobRequirements::List(reqs)) => Ok(Some(
            gate::check_person_for_requirements(db, personnel_id, reqs, work_day).await?,
        )),
    }
}
