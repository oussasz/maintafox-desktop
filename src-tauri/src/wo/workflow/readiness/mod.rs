//! Readiness rule engine.

pub mod rule;
pub mod rules;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::wo::workflow::readiness::rule::{
    ReadinessCheck, ReadinessOutcome, ReadinessPhase, RuleCategory, RuleSeverity,
};
use crate::wo::workflow::readiness::rules::all_ready_gate_rules;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessCheckDto {
    pub code: String,
    pub category: String,
    pub severity: String,
    pub blocking: bool,
    pub phase: String,
    pub outcome: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessReport {
    pub wo_id: i64,
    pub can_mark_ready: bool,
    pub blocking_failed: i64,
    pub recommended_failed: i64,
    pub checks: Vec<ReadinessCheckDto>,
}

#[derive(Debug, Clone)]
pub struct WoReadinessContext {
    pub wo_id: i64,
    pub equipment_id: Option<i64>,
    pub title: String,
    pub type_id: i64,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub primary_responsible_id: Option<i64>,
    pub assigned_group_id: Option<i64>,
    pub requires_permit: bool,
    pub planning_approved_at: Option<String>,
    pub approval_policy_required: bool,
    pub mandatory_task_count: i64,
    /// Material readiness snapshot (loaded with context).
    pub planned_parts_count: i64,
    pub materials_ready: bool,
    pub materials_ready_pct: f64,
    pub materials_reserved_pct: f64,
    pub materials_missing_parts: i64,
    pub materials_expected_arrival: Option<String>,
}

impl WoReadinessContext {
    pub async fn load(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Self> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT wo.equipment_id, wo.title, wo.type_id, wo.planned_start, wo.planned_end, \
                        wo.primary_responsible_id, wo.assigned_group_id, wo.requires_permit, \
                        wo.planning_approved_at \
                 FROM work_orders wo WHERE wo.id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: wo_id.to_string(),
            })?;

        let mandatory_task_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM work_order_tasks \
                 WHERE work_order_id = ? AND is_mandatory = 1",
                [wo_id.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<i64>("", "c").ok())
            .unwrap_or(0);

        let approval_policy_required = load_approval_policy(db).await?;
        let materials = load_material_readiness_snapshot(db, wo_id).await?;

        Ok(Self {
            wo_id,
            equipment_id: row.try_get("", "equipment_id").ok().flatten(),
            title: row.try_get("", "title").unwrap_or_default(),
            type_id: row.try_get("", "type_id").unwrap_or(0),
            planned_start: row.try_get("", "planned_start").ok().flatten(),
            planned_end: row.try_get("", "planned_end").ok().flatten(),
            primary_responsible_id: row.try_get("", "primary_responsible_id").ok().flatten(),
            assigned_group_id: row.try_get("", "assigned_group_id").ok().flatten(),
            requires_permit: row
                .try_get::<i64>("", "requires_permit")
                .map(|v| v != 0)
                .unwrap_or(false),
            planning_approved_at: row.try_get("", "planning_approved_at").ok().flatten(),
            approval_policy_required,
            mandatory_task_count,
            planned_parts_count: materials.0,
            materials_ready: materials.1,
            materials_ready_pct: materials.2,
            materials_reserved_pct: materials.3,
            materials_missing_parts: materials.4,
            materials_expected_arrival: materials.5,
        })
    }
}

/// Returns (planned_parts_count, ready, ready_pct, reserved_pct, missing, expected_arrival).
async fn load_material_readiness_snapshot(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<(i64, bool, f64, f64, i64, Option<String>)> {
    let parts = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wop.quantity_planned AS planned_qty,
                    COALESCE(wop.quantity_reserved, 0.0) AS quantity_reserved,
                    COALESCE(sb_sum.available_qty, 0.0) AS available_qty
             FROM work_order_parts wop
             LEFT JOIN (
                SELECT article_id, SUM(available_qty) AS available_qty
                FROM stock_balances
                GROUP BY article_id
             ) sb_sum ON sb_sum.article_id = wop.article_id
             WHERE wop.work_order_id = ?
               AND wop.article_id IS NOT NULL
               AND COALESCE(wop.quantity_planned, 0) > 0",
            [wo_id.into()],
        ))
        .await
        .unwrap_or_default();

    let total = parts.len() as i64;
    if total == 0 {
        return Ok((0, true, 100.0, 0.0, 0, None));
    }

    let mut available_parts = 0i64;
    let mut reserved_parts = 0i64;
    let mut missing = 0i64;
    for part in &parts {
        let planned: f64 = part.try_get("", "planned_qty").unwrap_or(0.0);
        let available: f64 = part.try_get("", "available_qty").unwrap_or(0.0);
        let qty_reserved: f64 = part.try_get("", "quantity_reserved").unwrap_or(0.0);
        let covered = qty_reserved.max(available);
        if qty_reserved >= planned {
            reserved_parts += 1;
            available_parts += 1;
        } else if covered >= planned {
            available_parts += 1;
        } else {
            missing += 1;
        }
    }

    let ready_pct = (available_parts as f64 / total as f64) * 100.0;
    let reserved_pct = (reserved_parts as f64 / total as f64) * 100.0;

    let expected_arrival: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MIN(COALESCE(po.ordered_at, po.approved_at, po.created_at)) AS eta
             FROM purchase_orders po
             INNER JOIN purchase_order_lines pol ON pol.purchase_order_id = po.id
             WHERE pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND pol.demand_source_id = ?
               AND po.status NOT IN ('RECEIVED_CLOSED', 'CANCELLED')",
            [wo_id.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<String>>("", "eta").ok().flatten());

    Ok((
        total,
        missing == 0,
        ready_pct,
        reserved_pct,
        missing,
        expected_arrival,
    ))
}

async fn load_approval_policy(db: &impl ConnectionTrait) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings \
             WHERE setting_key = 'wo_planning_approval_required' LIMIT 1",
            [],
        ))
        .await?;
    let Some(r) = row else {
        return Ok(false);
    };
    let raw: String = r.try_get("", "setting_value_json").unwrap_or_default();
    let trimmed = raw.trim().trim_matches('"').to_ascii_lowercase();
    Ok(matches!(trimmed.as_str(), "true" | "1" | "yes"))
}

fn dto_from_check(c: ReadinessCheck) -> ReadinessCheckDto {
    ReadinessCheckDto {
        code: c.code.to_string(),
        category: match c.category {
            RuleCategory::Planning => "planning",
            RuleCategory::Schedule => "schedule",
            RuleCategory::Assignment => "assignment",
            RuleCategory::Safety => "safety",
            RuleCategory::Parts => "parts",
            RuleCategory::Governance => "governance",
        }
        .to_string(),
        severity: match c.severity {
            RuleSeverity::Blocking => "blocking",
            RuleSeverity::Recommended => "recommended",
        }
        .to_string(),
        blocking: c.blocking,
        phase: match c.phase {
            ReadinessPhase::ReadyGate => "ready_gate",
        }
        .to_string(),
        outcome: match c.outcome {
            ReadinessOutcome::Pass => "pass",
            ReadinessOutcome::Fail => "fail",
            ReadinessOutcome::Na => "na",
        }
        .to_string(),
        message: c.message,
    }
}

pub async fn evaluate_wo_readiness(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<ReadinessReport> {
    let ctx = WoReadinessContext::load(db, wo_id).await?;
    let rules = all_ready_gate_rules();
    let mut checks = Vec::with_capacity(rules.len());
    let mut blocking_failed = 0i64;
    let mut recommended_failed = 0i64;

    for rule in rules {
        let check = rule.validate(&ctx);
        if check.blocking && matches!(check.outcome, ReadinessOutcome::Fail) {
            blocking_failed += 1;
        }
        if !check.blocking && matches!(check.outcome, ReadinessOutcome::Fail) {
            recommended_failed += 1;
        }
        checks.push(dto_from_check(check));
    }

    Ok(ReadinessReport {
        wo_id,
        can_mark_ready: blocking_failed == 0,
        blocking_failed,
        recommended_failed,
        checks,
    })
}

pub async fn assert_ready_to_mark(db: &DatabaseConnection, wo_id: i64) -> AppResult<()> {
    let report = evaluate_wo_readiness(db, wo_id).await?;
    if report.can_mark_ready {
        return Ok(());
    }
    let mut errors: Vec<String> = vec![
        "Impossible de marquer prêt : des contrôles bloquants ont échoué.".into(),
    ];
    for c in report.checks {
        if c.blocking && c.outcome == "fail" {
            errors.push(format!(
                "[{}] {}",
                c.code,
                c.message.unwrap_or_else(|| "échec".into())
            ));
        }
    }
    Err(AppError::ValidationFailed(errors))
}
