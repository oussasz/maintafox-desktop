//! Step 6 of a study: the decision log. A decision is recorded with the study evidence,
//! approved (or rejected) by someone holding `ram.manage`, then applied: a draft PM plan
//! version with the new interval, or a work order, created through `rcm_loop` so the PM side and
//! the work order keep the link to the decision. The effect compares the MTBF of the study scope
//! before the decision with the MTBF since it was applied.
//!
//! Four-eyes: the person who proposed a decision does not approve it and cannot name themselves its
//! approver — unless the decision's site opted out (per-site rule
//! `reliability.decisions.allow_self_approval`, `settings::site`, changed under step-up with a
//! reason and audited). The site is the planning site of the decision's equipment (or of every
//! equipment of its study); when it cannot be resolved to one site, four-eyes applies. A decision
//! approved by its own proposer is flagged `self_approved` wherever it is listed, and the same rule
//! is checked when a decision arrives by synchronisation (`inbound_self_approval_check`).

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::reliability::rcm_loop::{
    create_pm_revision_from_source_in, create_wo_from_source_in, PmRevisionFromSourceInput, WoFromSourceInput,
    SOURCE_RELIABILITY_DECISION,
};
use crate::reliability::study::analysis::{life_analysis, meter_counts_hours, operating_ratios};
use crate::reliability::study::clock::parse_instant_opt;
use crate::reliability::study::domain::{
    ApproverOption, CreateDecisionInput, DecideDecisionInput, DecisionEffect, DecisionEvidence, LifeAnalysisRequest,
    StudyDecision, DECISION_MODIFICATION_WO, DECISION_ONE_OFF_WO, DECISION_PM_INTERVAL, STATUS_APPLIED,
    STATUS_APPROVED, STATUS_PROPOSED, STATUS_REJECTED,
};
use crate::reliability::study::pm_interval::PlanTrigger;
use crate::reliability::study::records::{failures_since, study_clocks, BASIS_OPERATING};
use crate::reliability::study::store::{get, get_study, opt_f64, opt_i64, opt_str, STUDY_ACTIVE};

/// An effect is measured once the scope has run at least this multiple of the MTBF before, or
/// had two failures, since the decision was applied.
const EFFECT_MIN_MTBF_MULTIPLE: f64 = 1.0;
const EFFECT_MIN_FAILURES: i64 = 2;

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

/// Per-site rule (`settings::site`): `true` lets the proposer of a reliability decision approve it.
/// No row at a site = four-eyes.
pub const SELF_APPROVAL_SETTING: &str = "reliability.decisions.allow_self_approval";

/// Whether a decision's proposer may approve it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfApprovalRule {
    /// The decision's site opted out of four-eyes.
    Allowed,
    /// Four-eyes (the site's rule, or no rule set there).
    FourEyes,
    /// The equipment has no planning site, or the decision spans several sites: four-eyes.
    SiteUnresolved,
}

/// The planning site of a decision: of its equipment, else of every equipment of its study.
/// `None` when an equipment has no site or they belong to different sites.
async fn decision_site<C: ConnectionTrait>(db: &C, study_id: i64, equipment_id: Option<i64>) -> AppResult<Option<i64>> {
    let rows = match equipment_id {
        Some(eq) => {
            db.query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT installed_at_node_id AS node FROM equipment WHERE id = ?",
                [eq.into()],
            ))
            .await?
        }
        None => {
            db.query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT e.installed_at_node_id AS node FROM reliability_study_assets a \
                   JOIN equipment e ON e.id = a.equipment_id WHERE a.study_id = ?",
                [study_id.into()],
            ))
            .await?
        }
    };
    if rows.is_empty() {
        return Ok(None);
    }
    let mut site: Option<i64> = None;
    for r in &rows {
        let Some(node) = get::<Option<i64>>(r, "node")? else {
            return Ok(None);
        };
        let Some(s) = crate::planning::weekly_schedule::planning_site_of_node(db, node).await? else {
            return Ok(None);
        };
        if site.is_some_and(|known| known != s) {
            return Ok(None);
        }
        site = Some(s);
    }
    Ok(site)
}

/// Rule of a site already resolved (`None`: unresolved).
async fn site_rule<C: ConnectionTrait>(db: &C, site: Option<i64>) -> AppResult<SelfApprovalRule> {
    let Some(site) = site else {
        return Ok(SelfApprovalRule::SiteUnresolved);
    };
    Ok(
        if crate::settings::site::site_setting(db, site, SELF_APPROVAL_SETTING).await? == Some(true) {
            SelfApprovalRule::Allowed
        } else {
            SelfApprovalRule::FourEyes
        },
    )
}

/// The stricter of two rules: the proposer may approve only when both allow it.
fn strictest(a: SelfApprovalRule, b: SelfApprovalRule) -> SelfApprovalRule {
    use SelfApprovalRule::{Allowed, FourEyes, SiteUnresolved};
    match (a, b) {
        (Allowed, Allowed) => Allowed,
        (SiteUnresolved, _) | (_, SiteUnresolved) => SiteUnresolved,
        _ => FourEyes,
    }
}

/// Rule for a recorded decision: the strictest of the site stored when it was proposed and the
/// site its equipment belongs to now (moving equipment to an opted-out site never lifts
/// four-eyes on a decision already proposed). A decision recorded before the site was stored
/// is judged on its current site.
async fn recorded_decision_rule<C: ConnectionTrait>(
    db: &C,
    stored_site: Option<i64>,
    study_id: i64,
    equipment_id: Option<i64>,
    has_stored_site: bool,
) -> AppResult<SelfApprovalRule> {
    let live = self_approval_rule(db, study_id, equipment_id).await?;
    if !has_stored_site {
        return Ok(live);
    }
    Ok(strictest(site_rule(db, stored_site).await?, live))
}

/// Four-eyes or the site's opt-out, for a decision of `study_id` on `equipment_id`.
pub async fn self_approval_rule<C: ConnectionTrait>(
    db: &C,
    study_id: i64,
    equipment_id: Option<i64>,
) -> AppResult<SelfApprovalRule> {
    site_rule(db, decision_site(db, study_id, equipment_id).await?).await
}

/// Someone other than `actor_id` who may approve reliability decisions.
async fn second_approver_exists(db: &DatabaseConnection, actor_id: i64) -> AppResult<bool> {
    Ok(list_approvers(db).await?.iter().any(|o| o.user_id != actor_id))
}

/// Why a proposer may not approve their own decision: the site could not be resolved, nobody else
/// can approve (how to fix it), or four-eyes.
async fn self_approval_refusal(db: &DatabaseConnection, rule: SelfApprovalRule, actor_id: i64) -> AppResult<AppError> {
    Ok(match rule {
        SelfApprovalRule::SiteUnresolved => invalid("DECISION_SELF_APPROVAL_SITE_UNKNOWN"),
        _ if !second_approver_exists(db, actor_id).await? => invalid("DECISION_NO_SECOND_APPROVER"),
        _ => invalid("DECISION_SELF_APPROVAL"),
    })
}

const DECISION_SELECT: &str = "SELECT d.*, e.asset_id_code AS asset_code, p.code AS plan_code,
        (SELECT n.display_no FROM pm_plan_version_numbers n WHERE n.id = v.id) AS linked_version_no, wo.code AS linked_wo_code,
        COALESCE(ap.display_name, ap.username) AS approver_name,
        COALESCE(pb.display_name, pb.username) AS proposed_by_name,
        COALESCE(db.display_name, db.username) AS decided_by_name
     FROM reliability_study_decisions d
     LEFT JOIN equipment e ON e.id = d.equipment_id
     LEFT JOIN pm_plans p ON p.id = COALESCE(d.linked_pm_plan_id, d.pm_plan_id)
     LEFT JOIN pm_plan_versions v ON v.id = d.linked_pm_plan_version_id
     LEFT JOIN work_orders wo ON wo.id = d.linked_work_order_id
     LEFT JOIN user_accounts ap ON ap.id = d.approver_id
     LEFT JOIN user_accounts pb ON pb.id = d.proposed_by_id
     LEFT JOIN user_accounts db ON db.id = d.decided_by_id";

fn map_decision(r: &QueryResult, effect: DecisionEffect) -> AppResult<StudyDecision> {
    let evidence_json: String = get(r, "evidence_json")?;
    let trig: Option<String> = get(r, "proposed_trigger_definition_json")?;
    let trig_v = trig.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    Ok(StudyDecision {
        id: get(r, "id")?,
        study_id: get(r, "study_id")?,
        title: get(r, "title")?,
        action_kind: get(r, "action_kind")?,
        equipment_id: get(r, "equipment_id")?,
        asset_code: get(r, "asset_code")?,
        pm_plan_id: get(r, "pm_plan_id")?,
        pm_plan_code: get(r, "plan_code")?,
        proposed_interval_hours: get(r, "proposed_interval_hours")?,
        proposed_trigger_interval_value: trig_v
            .as_ref()
            .and_then(|v| v.get("interval_value"))
            .and_then(serde_json::Value::as_f64),
        proposed_trigger_interval_unit: trig_v.as_ref().map(|v| {
            v.get("interval_unit")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("operating_hours")
                .to_string()
        }),
        wo_type_code: get(r, "wo_type_code")?,
        justification: get(r, "justification")?,
        fmeca_item_id: get(r, "fmeca_item_id")?,
        rcm_decision_id: get(r, "rcm_decision_id")?,
        evidence: serde_json::from_str(&evidence_json).unwrap_or_default(),
        status: get(r, "status")?,
        approver_id: get(r, "approver_id")?,
        approver_name: get(r, "approver_name")?,
        proposed_by_id: get(r, "proposed_by_id")?,
        decided_by_id: get(r, "decided_by_id")?,
        self_approved: {
            let status: String = get(r, "status")?;
            let proposer: Option<i64> = get(r, "proposed_by_id")?;
            let decider: Option<i64> = get(r, "decided_by_id")?;
            (status == STATUS_APPROVED || status == STATUS_APPLIED) && proposer.is_some() && proposer == decider
        },
        // Set by the readers (`with_site_rule`).
        site_allows_self_approval: false,
        proposed_by_name: get(r, "proposed_by_name")?,
        proposed_at: get(r, "proposed_at")?,
        decided_by_name: get(r, "decided_by_name")?,
        decided_at: get(r, "decided_at")?,
        decision_note: get(r, "decision_note")?,
        applied_at: get(r, "applied_at")?,
        linked_pm_plan_id: get(r, "linked_pm_plan_id")?,
        linked_pm_plan_version_id: get(r, "linked_pm_plan_version_id")?,
        linked_pm_version_no: get(r, "linked_version_no")?,
        linked_work_order_id: get(r, "linked_work_order_id")?,
        linked_work_order_code: get(r, "linked_wo_code")?,
        effect,
        sync_refused_at: get(r, "sync_refused_at")?,
        sync_refused_reason: get(r, "sync_refused_reason")?,
        row_version: get(r, "row_version")?,
    })
}

async fn effect_of(db: &DatabaseConnection, r: &QueryResult) -> AppResult<DecisionEffect> {
    let status: String = get(r, "status")?;
    let applied_at: Option<String> = get(r, "applied_at")?;
    let before: Option<f64> = get(r, "baseline_mtbf_hours")?;
    let not = |state: &str| DecisionEffect {
        state: state.into(),
        mtbf_before: before,
        mtbf_after: None,
        after_is_lower_bound: false,
        change: None,
        failures_after: 0,
        hours_after: 0.0,
    };
    let Some(applied) = parse_instant_opt(applied_at.as_deref()).filter(|_| status == STATUS_APPLIED) else {
        return Ok(not("not_applied"));
    };
    let study = get_study(db, get(r, "study_id")?).await?;
    let (_, _, clocks) = study_clocks(db, &study).await?;
    let eq: Option<i64> = get(r, "equipment_id")?;
    let ids: Vec<i64> = match eq {
        Some(e) => vec![e],
        None => study.assets.iter().map(|a| a.equipment_id).collect(),
    };
    let (failures, hours) = failures_since(db, &ids, &clocks, study.failure_mode_id, applied).await?;
    let enough_time = before.is_some_and(|b| hours >= b * EFFECT_MIN_MTBF_MULTIPLE);
    if failures < EFFECT_MIN_FAILURES && !enough_time {
        return Ok(DecisionEffect {
            failures_after: failures,
            hours_after: hours,
            ..not("not_yet_measured")
        });
    }
    let after = hours / failures.max(1) as f64;
    Ok(DecisionEffect {
        state: "measured".into(),
        mtbf_before: before,
        mtbf_after: Some(after),
        after_is_lower_bound: failures < EFFECT_MIN_FAILURES,
        change: before.filter(|b| *b > 0.0).map(|b| after / b - 1.0),
        failures_after: failures,
        hours_after: hours,
    })
}

pub async fn get_decision(db: &DatabaseConnection, id: i64) -> AppResult<StudyDecision> {
    let r = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{DECISION_SELECT} WHERE d.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study_decision".into(),
            id: id.to_string(),
        })?;
    let effect = effect_of(db, &r).await?;
    with_site_rule(db, map_decision(&r, effect)?).await
}

/// Fills `site_allows_self_approval` from the decision's rule (stored and current site).
async fn with_site_rule(db: &DatabaseConnection, mut d: StudyDecision) -> AppResult<StudyDecision> {
    let stored = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT site_node_id, site_recorded FROM reliability_study_decisions WHERE id = ?",
            [d.id.into()],
        ))
        .await?;
    let (site, recorded) = match stored {
        Some(r) => (
            get::<Option<i64>>(&r, "site_node_id")?,
            get::<i64>(&r, "site_recorded")? != 0,
        ),
        None => (None, false),
    };
    d.site_allows_self_approval =
        recorded_decision_rule(db, site, d.study_id, d.equipment_id, recorded).await? == SelfApprovalRule::Allowed;
    Ok(d)
}

pub async fn list_decisions(db: &DatabaseConnection, study_id: i64) -> AppResult<Vec<StudyDecision>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{DECISION_SELECT} WHERE d.study_id = ? ORDER BY d.created_at DESC, d.id DESC"),
            [study_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let effect = effect_of(db, r).await?;
        out.push(with_site_rule(db, map_decision(r, effect)?).await?);
    }
    Ok(out)
}

/// Active users who hold `ram.manage` through a tenant-wide role assignment (or are admins).
pub async fn list_approvers(db: &DatabaseConnection) -> AppResult<Vec<ApproverOption>> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT ua.id, COALESCE(ua.display_name, ua.username) AS name
             FROM user_accounts ua
             JOIN user_scope_assignments usa ON usa.user_id = ua.id
             JOIN role_permissions rp ON rp.role_id = usa.role_id
             JOIN permissions p ON p.id = rp.permission_id
             WHERE ua.is_active = 1 AND ua.deleted_at IS NULL
               AND usa.deleted_at IS NULL AND usa.scope_type = 'tenant'
               AND (usa.valid_from IS NULL OR usa.valid_from <= ?)
               AND (usa.valid_to IS NULL OR usa.valid_to >= ?)
               AND usa.is_emergency = 0
               AND p.name = ?
             ORDER BY name ASC",
            [
                now.clone().into(),
                now.into(),
                crate::rbac::permissions::RAM_MANAGE.into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(ApproverOption {
                user_id: get(r, "id")?,
                display_name: get(r, "name")?,
            })
        })
        .collect()
}

pub async fn create_decision(
    db: &DatabaseConnection,
    input: CreateDecisionInput,
    actor_id: i64,
) -> AppResult<StudyDecision> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(invalid("DECISION_TITLE_REQUIRED"));
    }
    let study = get_study(db, input.study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    let kind = input.action_kind.as_str();
    if ![DECISION_PM_INTERVAL, DECISION_ONE_OFF_WO, DECISION_MODIFICATION_WO].contains(&kind) {
        return Err(invalid("DECISION_KIND_INVALID"));
    }
    let (analysis, built) = life_analysis(
        db,
        LifeAnalysisRequest {
            study_id: input.study_id,
            planned_cost: input.planned_cost,
            failure_cost: input.failure_cost,
            pm_plan_id: input.pm_plan_id,
        },
    )
    .await?;
    let operating = analysis.effective_time_basis == BASIS_OPERATING;
    let in_study = |id: i64| study.assets.iter().any(|a| a.equipment_id == id);

    let (equipment_id, pm_plan_id, interval, trigger_json, wo_type) = match kind {
        DECISION_PM_INTERVAL => {
            let plan_id = input.pm_plan_id.ok_or_else(|| invalid("DECISION_PM_PLAN_REQUIRED"))?;
            let plan = analysis
                .pm_plans
                .iter()
                .find(|p| p.pm_plan_id == plan_id)
                .ok_or_else(|| invalid("DECISION_PM_PLAN_NOT_IN_STUDY"))?;
            let hours = input
                .proposed_interval_hours
                .filter(|h| h.is_finite() && *h > 0.0)
                .ok_or_else(|| invalid("DECISION_INTERVAL_REQUIRED"))?;
            let trig_row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT p.strategy_type, COALESCE(cv.trigger_definition_json,
                        (SELECT v.trigger_definition_json FROM pm_plan_versions v WHERE v.pm_plan_id = p.id
                         ORDER BY v.version_no DESC, v.entity_sync_id DESC LIMIT 1)) AS trig
                     FROM pm_plans p LEFT JOIN pm_plan_versions cv ON cv.id = p.current_version_id WHERE p.id = ?",
                    [plan_id.into()],
                ))
                .await?
                .ok_or_else(|| invalid("DECISION_PM_PLAN_NOT_IN_STUDY"))?;
            let strategy: String = get(&trig_row, "strategy_type")?;
            let trig: String =
                get::<Option<String>>(&trig_row, "trig")?.ok_or_else(|| invalid("DECISION_PM_PLAN_NO_VERSION"))?;
            let parsed = PlanTrigger::parse(&strategy, &trig, meter_counts_hours(db, &trig).await?)
                .map_err(|n| invalid(&format!("DECISION_PM_INTERVAL_{}", n.to_uppercase())))?;
            let ratio = operating_ratios(&built).get(&plan.equipment_id).copied();
            let (json, _, _) = parsed
                .with_interval_hours(hours, operating, ratio)
                .map_err(|n| invalid(&format!("DECISION_PM_INTERVAL_{}", n.to_uppercase())))?;
            (Some(plan.equipment_id), Some(plan_id), Some(hours), Some(json), None)
        }
        _ => {
            let eq = input
                .equipment_id
                .or_else(|| (study.assets.len() == 1).then(|| study.assets[0].equipment_id))
                .ok_or_else(|| invalid("DECISION_EQUIPMENT_REQUIRED"))?;
            if !in_study(eq) {
                return Err(invalid("DECISION_EQUIPMENT_NOT_IN_STUDY"));
            }
            let wo_type = input
                .wo_type_code
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| invalid("DECISION_WO_TYPE_REQUIRED"))?;
            if crate::wo::types::resolve_work_order_type_id_by_code(db, &wo_type)
                .await
                .is_err()
            {
                return Err(invalid("DECISION_WO_TYPE_UNKNOWN"));
            }
            (Some(eq), None, None, None, Some(wo_type))
        }
    };
    // Links to the FMECA row / RCM decision of this study (steps 3 and 4).
    let mut fmeca_mode_label = None;
    let mut rpn = None;
    let mut rcm_tactic = None;
    if let Some(item) = input.fmeca_item_id {
        let analysis = crate::reliability::study::fmeca::study_fmeca_analysis_id(db, study.id).await?;
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT i.rpn, m.label, i.review_required FROM fmeca_items i
                 LEFT JOIN reference_values m ON m.id = i.failure_mode_id
                 WHERE i.id = ? AND i.analysis_id = ?",
                [item.into(), analysis.unwrap_or(-1).into()],
            ))
            .await?
            .ok_or_else(|| invalid("DECISION_FMECA_ITEM_NOT_IN_STUDY"))?;
        if get::<i64>(&row, "review_required")? != 0 {
            // A decision never rests on ratings nobody chose (migration 221).
            return Err(invalid("FMECA_ITEM_REVIEW_REQUIRED"));
        }
        rpn = get::<Option<i64>>(&row, "rpn")?;
        fmeca_mode_label = get::<Option<String>>(&row, "label")?;
    }
    if let Some(rcm) = input.rcm_decision_id {
        let rcm_study = crate::reliability::study::rcm::study_rcm_study_id(db, study.id).await?;
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT selected_tactic, fmeca_item_id FROM rcm_decisions WHERE id = ? AND study_id = ?",
                [rcm.into(), rcm_study.unwrap_or(-1).into()],
            ))
            .await?
            .ok_or_else(|| invalid("DECISION_RCM_NOT_IN_STUDY"))?;
        rcm_tactic = get::<Option<String>>(&row, "selected_tactic")?;
        let item: Option<i64> = get(&row, "fmeca_item_id")?;
        if input.fmeca_item_id.is_some() && item != input.fmeca_item_id {
            return Err(invalid("DECISION_RCM_NOT_IN_STUDY"));
        }
    }
    // The site the decision applies to, from the equipment it was resolved to (a PM plan's
    // equipment, or the work order's): stored with the decision.
    let decision_site_id = decision_site(db, input.study_id, equipment_id).await?;
    if let Some(a) = input.approver_id {
        if a == actor_id {
            match site_rule(db, decision_site_id).await? {
                SelfApprovalRule::Allowed => {}
                SelfApprovalRule::FourEyes => return Err(invalid("DECISION_SELF_APPROVER")),
                SelfApprovalRule::SiteUnresolved => return Err(invalid("DECISION_SELF_APPROVAL_SITE_UNKNOWN")),
            }
        }
        if !list_approvers(db).await?.iter().any(|o| o.user_id == a) {
            return Err(invalid("DECISION_APPROVER_INVALID"));
        }
    }

    let fit = analysis.fit.as_ref();
    let cost = analysis.cost.as_ref();
    let current = analysis
        .selected_pm_plan_id
        .and_then(|id| analysis.pm_plans.iter().find(|p| p.pm_plan_id == id))
        .and_then(|p| p.interval_hours);
    let evidence = DecisionEvidence {
        time_basis: analysis.effective_time_basis.clone(),
        n_failures: analysis.n_failures,
        n_suspensions: analysis.n_suspensions,
        beta: fit.map(|f| f.beta),
        eta: fit.map(|f| f.eta),
        beta_lower: fit.map(|f| f.beta_lower),
        beta_upper: fit.map(|f| f.beta_upper),
        b10: fit.map(|f| f.b10),
        mean_life: fit.map(|f| f.mean_life),
        rho: fit.and_then(|f| f.rho),
        pattern: fit.map(|f| f.pattern.clone()),
        optimal_interval: cost.and_then(|c| c.proposed).and_then(|p| p.interval),
        current_interval: current,
        proposed_cost_rate: cost.and_then(|c| c.proposed).map(|p| p.cost_rate),
        current_cost_rate: cost.and_then(|c| c.current).map(|p| p.cost_rate),
        run_to_failure_cost_rate: cost.map(|c| c.run_to_failure.cost_rate),
        annual_saving_vs_current: cost.and_then(|c| c.annual_saving_vs_current),
        planned_cost: cost.map(|c| c.planned_cost),
        failure_cost: cost.map(|c| c.failure_cost),
        currency_code: analysis.cost_currency_code.clone(),
        operating_ratio: analysis.operating_ratio,
        study_period_start: study.period_start.clone(),
        study_period_end: study.period_end.clone(),
        fmeca_mode_label,
        rpn,
        rcm_tactic,
    };
    // MTBF of the scope before the decision: study hours / failures used (all study equipment,
    // or the equipment the decision acts on).
    let baseline_failures = built.failures_used;
    let baseline_hours = built.total_hours;
    let baseline_mtbf =
        (baseline_failures > 0 && baseline_hours > 0.0).then(|| baseline_hours / baseline_failures as f64);

    let ts = Utc::now().to_rfc3339();
    let sync_id = crate::sync::identity::new_entity_sync_id();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reliability_study_decisions (entity_sync_id, study_id, title, action_kind, equipment_id,
            pm_plan_id, proposed_interval_hours, proposed_trigger_definition_json, wo_type_code, justification,
            evidence_json, baseline_mtbf_hours, baseline_failures, baseline_exposure_hours, status, approver_id,
            proposed_by_id, proposed_at, fmeca_item_id, rcm_decision_id, site_node_id, site_recorded,
            row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'proposed', ?, ?, ?, ?, ?, ?, 1, 1, ?, ?)",
        [
            sync_id.clone().into(),
            input.study_id.into(),
            title.into(),
            kind.into(),
            opt_i64(equipment_id),
            opt_i64(pm_plan_id),
            opt_f64(interval),
            opt_str(trigger_json),
            opt_str(wo_type),
            input.justification.unwrap_or_default().trim().to_string().into(),
            serde_json::to_string(&evidence)?.into(),
            opt_f64(baseline_mtbf),
            baseline_failures.into(),
            baseline_hours.into(),
            opt_i64(input.approver_id),
            actor_id.into(),
            ts.clone().into(),
            opt_i64(input.fmeca_item_id),
            opt_i64(input.rcm_decision_id),
            opt_i64(decision_site_id),
            ts.clone().into(),
            ts.into(),
        ],
    ))
    .await?;
    let id: i64 = get(
        &db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reliability_study_decisions WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("decision insert missing".into()))?,
        "id",
    )?;
    get_decision(db, id).await
}

async fn set_status(
    db: &impl ConnectionTrait,
    input: &DecideDecisionInput,
    from: &str,
    to: &str,
    actor_id: i64,
) -> AppResult<()> {
    let res = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_study_decisions SET status = ?, decided_by_id = ?, decided_at = ?, decision_note = ?,
                row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND status = ? AND row_version = ?
               AND (? <> 'approved' OR sync_refused_at IS NULL)",
            [
                to.into(),
                actor_id.into(),
                Utc::now().to_rfc3339().into(),
                opt_str(
                    input
                        .note
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                ),
                Utc::now().to_rfc3339().into(),
                input.decision_id.into(),
                from.into(),
                input.expected_row_version.into(),
                // Never approved once refused at synchronisation (AU-76b, review N1).
                to.into(),
            ],
        ))
        .await?;
    if res.rows_affected() == 0 {
        return Err(invalid("DECISION_STALE"));
    }
    Ok(())
}

/// Who proposed a decision, who is designated to decide it, and where it applies.
async fn decision_parties<C: ConnectionTrait>(db: &C, decision_id: i64) -> AppResult<DecisionParties> {
    let r = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, title, status, approver_id, proposed_by_id, study_id, equipment_id, \
                    site_node_id, site_recorded \
             FROM reliability_study_decisions WHERE id = ?",
            [decision_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study_decision".into(),
            id: decision_id.to_string(),
        })?;
    Ok(DecisionParties {
        sync_id: get(&r, "entity_sync_id")?,
        title: get(&r, "title")?,
        status: get(&r, "status")?,
        approver: get(&r, "approver_id")?,
        proposer: get(&r, "proposed_by_id")?,
        study_id: get(&r, "study_id")?,
        equipment_id: get(&r, "equipment_id")?,
        site: get(&r, "site_node_id")?,
        site_recorded: get::<i64>(&r, "site_recorded")? != 0,
    })
}

/// Refuses anyone but the designated approver (when one is set); returns who proposed the decision
/// and where it applies.
async fn designated_approver_check(
    db: &DatabaseConnection,
    decision_id: i64,
    actor_id: i64,
) -> AppResult<DecisionParties> {
    let parties = decision_parties(db, decision_id).await?;
    if parties.approver.is_some_and(|a| a != actor_id) {
        return Err(invalid("DECISION_NOT_DESIGNATED_APPROVER"));
    }
    Ok(parties)
}

/// Who proposed a decision, and where it applies (for the site's four-eyes rule).
struct DecisionParties {
    sync_id: Option<String>,
    title: String,
    status: String,
    /// The person designated to decide it, when one was named.
    approver: Option<i64>,
    proposer: Option<i64>,
    study_id: i64,
    equipment_id: Option<i64>,
    /// Site stored when the decision was proposed (`site_recorded`), `None` if unresolved then.
    site: Option<i64>,
    site_recorded: bool,
}

pub async fn approve_decision(
    db: &DatabaseConnection,
    input: DecideDecisionInput,
    actor_id: i64,
) -> AppResult<StudyDecision> {
    // Separation of duties (as for purchase approvals): the proposer never approves their own
    // decision. Rejecting it stays open to them (it withdraws the proposal).
    let parties = designated_approver_check(db, input.decision_id, actor_id).await?;
    if parties.proposer == Some(actor_id) {
        let rule = recorded_decision_rule(
            db,
            parties.site,
            parties.study_id,
            parties.equipment_id,
            parties.site_recorded,
        )
        .await?;
        if rule != SelfApprovalRule::Allowed {
            return Err(self_approval_refusal(db, rule, actor_id).await?);
        }
    }
    // The approval, its seal and its audit event: one transaction (AU-76b, CMMS Advisor Q7); a
    // decision refused at synchronisation is checked inside it, and the update is guarded too.
    let txn = db.begin().await?;
    refuse_if_refused_at_sync(&txn, input.decision_id).await?;
    set_status(&txn, &input, STATUS_PROPOSED, STATUS_APPROVED, actor_id).await?;
    let seal = crate::reliability::study::decision_sync::local_approval_seal(&txn, input.decision_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study_decision".into(),
            id: input.decision_id.to_string(),
        })?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reliability_study_decisions SET approval_seal = ? WHERE id = ?",
        [seal.clone().into(), input.decision_id.into()],
    ))
    .await?;
    let mut details = audit_refs(&txn, input.decision_id).await?;
    details.insert("summary".into(), "Reliability decision approved".into());
    details.insert("title".into(), parties.title.clone().into());
    details.insert("approval_seal".into(), seal.into());
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: APPROVED_EVENT.to_string(),
            target_type: Some("reliability_study_decisions".to_string()),
            target_id: Some(audit_target(&parties, input.decision_id)),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::Value::Object(details)),
        },
    )
    .await?;
    txn.commit().await?;
    get_decision(db, input.decision_id).await
}

/// Audit action of an approval; its details carry the approval seal (CMMS Advisor Q7).
pub const APPROVED_EVENT: &str = "reliability.decision.approved";

/// The target of a decision's audit event: its sync id, the same on every PC.
fn audit_target(parties: &DecisionParties, decision_id: i64) -> String {
    parties
        .sync_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| decision_id.to_string())
}

/// The decision, its study, its proposer and its designated approver named by sync id in an audit
/// event's details: the event travels, and local ids mean nothing on another PC (review of #179).
async fn audit_refs<C: ConnectionTrait>(
    db: &C,
    decision_id: i64,
) -> AppResult<serde_json::Map<String, serde_json::Value>> {
    let r = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.entity_sync_id AS decision_sync_id, s.entity_sync_id AS study_sync_id, \
                    pb.entity_sync_id AS proposed_by_sync_id, ap.entity_sync_id AS designated_approver_sync_id \
               FROM reliability_study_decisions d \
               LEFT JOIN reliability_studies s ON s.id = d.study_id \
               LEFT JOIN user_accounts pb ON pb.id = d.proposed_by_id \
               LEFT JOIN user_accounts ap ON ap.id = d.approver_id \
              WHERE d.id = ?",
            [decision_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study_decision".into(),
            id: decision_id.to_string(),
        })?;
    let mut out = serde_json::Map::new();
    for key in [
        "decision_sync_id",
        "study_sync_id",
        "proposed_by_sync_id",
        "designated_approver_sync_id",
    ] {
        let v: Option<String> = get(&r, key)?;
        out.insert(key.into(), v.map_or(serde_json::Value::Null, serde_json::Value::String));
    }
    Ok(out)
}

/// A decision refused at synchronisation (here, or on another PC and received with its mark) is
/// never approved or applied, on any PC (AU-76b, CMMS Advisor Q5).
async fn refuse_if_refused_at_sync<C: ConnectionTrait>(db: &C, decision_id: i64) -> AppResult<()> {
    let refused = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS r FROM reliability_study_decisions WHERE id = ? AND sync_refused_at IS NOT NULL",
            [decision_id.into()],
        ))
        .await?
        .is_some();
    if refused {
        return Err(AppError::ValidationFailed(vec![
            "GATE_REL_DECISION_SYNC_REFUSED: This decision was refused at synchronisation (its approval is not valid on a PC): it can no longer be approved or applied. Propose and approve a new decision."
                .into(),
        ]));
    }
    Ok(())
}

/// The right checked to reject a decision: anyone else needs `ram.manage`; its proposer withdraws
/// with `ram.analyze` (the right they proposed with) or, without it, with `ram.manage` (D10).
pub fn reject_right(is_proposer: bool, holds_analyze: bool) -> &'static str {
    if is_proposer && holds_analyze {
        crate::rbac::permissions::RAM_ANALYZE
    } else {
        crate::rbac::permissions::RAM_MANAGE
    }
}

/// Whether `actor_id` proposed the decision: their rejection is a withdrawal (owner decision D10).
pub async fn is_proposer(db: &DatabaseConnection, decision_id: i64, actor_id: i64) -> AppResult<bool> {
    Ok(decision_parties(db, decision_id).await?.proposer == Some(actor_id))
}

/// Rejects a proposed decision, or withdraws it when the actor proposed it.
///
/// Owner decision D10: the proposer may withdraw their own proposal at any time before it is
/// decided, even when another person was designated to approve it (a withdrawal approves nothing).
/// Anyone else rejects only as the designated approver, when one is named. Once approved, rejected
/// or applied, the decision can no longer be withdrawn or rejected. Both are audited with the
/// reason, in the same transaction.
pub async fn reject_decision(
    db: &DatabaseConnection,
    input: DecideDecisionInput,
    actor_id: i64,
) -> AppResult<StudyDecision> {
    let note = input.note.as_deref().map(str::trim).unwrap_or_default().to_string();
    if note.is_empty() {
        return Err(invalid("DECISION_REJECT_REASON_REQUIRED"));
    }
    let parties = decision_parties(db, input.decision_id).await?;
    let withdrawal = parties.proposer == Some(actor_id);
    if !withdrawal && parties.approver.is_some_and(|a| a != actor_id) {
        return Err(invalid("DECISION_NOT_DESIGNATED_APPROVER"));
    }
    let txn = db.begin().await?;
    if let Err(e) = set_status(&txn, &input, STATUS_PROPOSED, STATUS_REJECTED, actor_id).await {
        let status = decision_parties(&txn, input.decision_id).await?.status;
        return Err(if status == STATUS_PROPOSED {
            e
        } else {
            AppError::ValidationFailed(vec![
                "GATE_REL_DECISION_ALREADY_DECIDED: The decision has already been approved, rejected or applied: it can no longer be withdrawn or rejected."
                    .into(),
            ])
        });
    }
    // Sync ids, not local ids: the event travels (review of #179).
    let mut details = audit_refs(&txn, input.decision_id).await?;
    details.insert(
        "summary".into(),
        if withdrawal {
            "Reliability decision withdrawn by its proposer"
        } else {
            "Reliability decision rejected"
        }
        .into(),
    );
    details.insert("title".into(), parties.title.clone().into());
    details.insert("reason".into(), note.clone().into());
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: if withdrawal {
                "reliability.decision.withdrawn"
            } else {
                "reliability.decision.rejected"
            }
            .to_string(),
            target_type: Some("reliability_study_decisions".to_string()),
            target_id: Some(audit_target(&parties, input.decision_id)),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::Value::Object(details)),
        },
    )
    .await?;
    txn.commit().await?;
    get_decision(db, input.decision_id).await
}

/// Carry out an approved decision in maintenance: a draft PM plan version with the proposed
/// trigger, or a work order on the decision's equipment. Both stay linked to the decision.
///
/// One transaction: the decision is claimed first (`approved` → `applied`, guarded by its status and
/// the row version the caller saw), then the PM version or work order is created and linked. A
/// second apply of the same decision (double click, two screens, two people) finds nothing to claim
/// and is refused; a failure leaves the decision approved with nothing created (audit finding AU-76).
pub async fn apply_decision(
    db: &DatabaseConnection,
    input: DecideDecisionInput,
    actor_id: i64,
) -> AppResult<StudyDecision> {
    let txn = db.begin().await?;
    refuse_if_refused_at_sync(&txn, input.decision_id).await?;
    let ts = Utc::now().to_rfc3339();
    let claimed = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_study_decisions SET status = ?, applied_by_id = ?, applied_at = ?,
                row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND status = ? AND row_version = ? AND sync_refused_at IS NULL",
            [
                STATUS_APPLIED.into(),
                actor_id.into(),
                ts.clone().into(),
                ts.into(),
                input.decision_id.into(),
                STATUS_APPROVED.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if claimed.rows_affected() == 0 {
        let status = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT status FROM reliability_study_decisions WHERE id = ?",
                [input.decision_id.into()],
            ))
            .await?
            .map(|r| get::<String>(&r, "status"))
            .transpose()?
            .ok_or_else(|| AppError::NotFound {
                entity: "reliability_study_decision".into(),
                id: input.decision_id.to_string(),
            })?;
        // Not approved (or already applied by someone else): nothing to apply. Approved, but at
        // another version than the one shown: reload first.
        return Err(invalid(if status == STATUS_APPROVED {
            "DECISION_STALE"
        } else {
            "DECISION_NOT_APPROVED"
        }));
    }
    // The claimed row: the version the caller saw (its row version matched).
    let r = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT action_kind, title, pm_plan_id, wo_type_code, proposed_trigger_definition_json \
             FROM reliability_study_decisions WHERE id = ?",
            [input.decision_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study_decision".into(),
            id: input.decision_id.to_string(),
        })?;
    let action_kind: String = get(&r, "action_kind")?;
    let title: String = get(&r, "title")?;
    if action_kind == DECISION_PM_INTERVAL {
        let plan_id = get::<Option<i64>>(&r, "pm_plan_id")?.ok_or_else(|| invalid("DECISION_PM_PLAN_REQUIRED"))?;
        let trig = get::<Option<String>>(&r, "proposed_trigger_definition_json")?
            .ok_or_else(|| invalid("DECISION_TRIGGER_MISSING"))?;
        create_pm_revision_from_source_in(
            &txn,
            PmRevisionFromSourceInput {
                source_type: SOURCE_RELIABILITY_DECISION.into(),
                source_id: input.decision_id,
                pm_plan_id: plan_id,
                trigger_definition_json: Some(trig),
                change_note: title,
            },
        )
        .await?;
    } else {
        let type_code =
            get::<Option<String>>(&r, "wo_type_code")?.ok_or_else(|| invalid("DECISION_WO_TYPE_REQUIRED"))?;
        create_wo_from_source_in(
            &txn,
            WoFromSourceInput {
                source_type: SOURCE_RELIABILITY_DECISION.into(),
                source_id: input.decision_id,
                type_code,
                title: Some(title),
            },
            actor_id,
        )
        .await?;
    }
    txn.commit().await?;
    get_decision(db, input.decision_id).await
}

/// What to do with a decision received by synchronisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InboundDecisionVerdict {
    /// A parent (study, equipment, site, person) is not applied here yet: wait.
    Defer(String),
    /// Refused: a sync conflict for a person to decide.
    Conflict(String),
    /// Refused: the decision is not approved (proposed or rejected) yet names a work order or a PM
    /// version created from it — only an approved decision is carried out (audit finding AU-69).
    LinkedWhileUnapproved(String),
}

/// Columns of a decision that point to what was created from it.
const DECISION_LINK_KEYS: [&str; 3] = [
    "linked_work_order_sync_id",
    "linked_pm_plan_version_sync_id",
    "linked_pm_plan_sync_id",
];

/// Local id of a row referenced by its sync id in a payload (`Err(())`: named but not here).
async fn payload_ref<C: ConnectionTrait>(
    conn: &C,
    payload: &serde_json::Map<String, serde_json::Value>,
    column: &str,
    table: &str,
) -> AppResult<Result<Option<i64>, ()>> {
    match payload.get(column) {
        Some(serde_json::Value::String(sync_id)) => Ok(conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT id FROM {table} WHERE entity_sync_id = ?"),
                [sync_id.clone().into()],
            ))
            .await?
            .map(|r| get::<i64>(&r, "id"))
            .transpose()?
            .map_or(Err(()), |id| Ok(Some(id)))),
        None | Some(serde_json::Value::Null) => Ok(Ok(None)),
        // Payloads name rows by sync id; anything else is not trusted as a local id.
        Some(_) => Ok(Err(())),
    }
}

const STATUS_RANK: [(&str, i64); 4] = [
    (STATUS_PROPOSED, 0),
    (STATUS_APPROVED, 1),
    (STATUS_APPLIED, 2),
    (STATUS_REJECTED, 1),
];

fn status_rank(status: &str) -> Option<i64> {
    STATUS_RANK.iter().find(|(s, _)| *s == status).map(|(_, r)| *r)
}

/// The local row of a received decision, its references expressed as sync ids.
struct LocalDecision {
    status: String,
    study: Option<String>,
    equipment: Option<String>,
    proposer: Option<String>,
    decider: Option<String>,
    study_id: i64,
    equipment_id: Option<i64>,
    site_node_id: Option<i64>,
    site_recorded: bool,
}

async fn local_decision<C: ConnectionTrait>(conn: &C, sync_id: &str) -> AppResult<Option<LocalDecision>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.status, s.entity_sync_id AS study, e.entity_sync_id AS equipment, \
                    pb.entity_sync_id AS proposer, db.entity_sync_id AS decider, \
                    d.study_id, d.equipment_id, d.site_node_id, d.site_recorded \
             FROM reliability_study_decisions d \
             LEFT JOIN reliability_studies s ON s.id = d.study_id \
             LEFT JOIN equipment e ON e.id = d.equipment_id \
             LEFT JOIN user_accounts pb ON pb.id = d.proposed_by_id \
             LEFT JOIN user_accounts db ON db.id = d.decided_by_id \
             WHERE d.entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?;
    row.map(|r| {
        Ok(LocalDecision {
            status: get(&r, "status")?,
            study: get(&r, "study")?,
            equipment: get(&r, "equipment")?,
            proposer: get(&r, "proposer")?,
            decider: get(&r, "decider")?,
            study_id: get(&r, "study_id")?,
            equipment_id: get(&r, "equipment_id")?,
            site_node_id: get(&r, "site_node_id")?,
            site_recorded: get::<i64>(&r, "site_recorded")? != 0,
        })
    })
    .transpose()
}

/// Four-eyes on a decision received by synchronisation (`before_upsert_checked`):
/// - the proposer and decider are taken from the payload, or from the local row when the payload
///   leaves them out;
/// - a decision still `proposed` never names its proposer as decider;
/// - an approval (or applied decision) by its own proposer is refused as a sync conflict unless
///   the decision's site allows it here — the strictest of the site stored with the decision and
///   its equipment's current site — or this device already holds exactly that approval (same
///   study, equipment, proposer and decider; status the same or further along);
/// - a site stored here that a new organisation structure replaced (deleted here, the received site
///   is the node replacing it, `org::validation::node_replaces_deleted_node`) gives way to the
///   received site;
/// - a study, equipment, site or person not applied here yet makes the row wait.
pub(crate) async fn inbound_self_approval_check<C: ConnectionTrait>(
    conn: &C,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundDecisionVerdict>> {
    let local = local_decision(conn, entity_sync_id).await?;
    // A field left out of the payload (older device, partial row) keeps the local value.
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| local.as_ref().map(|l| l.status.clone()))
        .unwrap_or_default();
    let status = status.as_str();
    let party = |key: &str, local_value: Option<&String>| -> Option<String> {
        match payload.get(key) {
            Some(serde_json::Value::String(v)) => Some(v.clone()),
            Some(_) => None,
            None => local_value.cloned(),
        }
    };
    let proposer = party("proposed_by_sync_id", local.as_ref().and_then(|l| l.proposer.as_ref()));
    let decider = party("decided_by_sync_id", local.as_ref().and_then(|l| l.decider.as_ref()));
    let title = payload
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    // A work order or PM version is created only from an approved decision (applying it). A received
    // decision still proposed, or rejected, that names one was carried out without approval (a
    // crafted call on an older version): refused, never applied here.
    if status == STATUS_PROPOSED || status == STATUS_REJECTED {
        let linked = DECISION_LINK_KEYS
            .iter()
            .any(|k| payload.get(*k).is_some_and(|v| !v.is_null()));
        if linked {
            return Ok(Some(InboundDecisionVerdict::LinkedWhileUnapproved(format!(
                "decision '{title}' is {status} but names a work order or preventive plan version created \
                 from it; only an approved decision is carried out"
            ))));
        }
    }
    let decided = status == STATUS_APPROVED || status == STATUS_APPLIED;
    if decided && decider.is_none() {
        return Ok(Some(InboundDecisionVerdict::Conflict(format!(
            "decision '{title}' is {status} but names nobody who approved it"
        ))));
    }
    let self_decided = proposer.is_some() && proposer == decider;
    if !self_decided {
        return Ok(None);
    }
    if status == STATUS_PROPOSED {
        return Ok(Some(InboundDecisionVerdict::Conflict(format!(
            "decision '{title}' is still proposed but names its proposer as the person who decided it"
        ))));
    }
    // Rejecting one's own proposal withdraws it: allowed, as on this device.
    if !decided {
        return Ok(None);
    }
    let ref_sync = |key: &str, local_value: Option<&String>| -> Option<String> {
        match payload.get(key) {
            Some(serde_json::Value::String(v)) => Some(v.clone()),
            Some(_) => None,
            None => local_value.cloned(),
        }
    };
    let study_sync = ref_sync("study_sync_id", local.as_ref().and_then(|l| l.study.as_ref()));
    let equipment_sync = ref_sync("equipment_sync_id", local.as_ref().and_then(|l| l.equipment.as_ref()));
    if let Some(l) = &local {
        let advances = matches!((status_rank(&l.status), status_rank(status)), (Some(a), Some(b)) if b >= a);
        let same_parties =
            l.study == study_sync && l.equipment == equipment_sync && l.proposer == proposer && l.decider == decider;
        let held_approval = (l.status == STATUS_APPROVED || l.status == STATUS_APPLIED) && l.proposer == l.decider;
        if held_approval && same_parties && advances {
            return Ok(None);
        }
    }
    // The study and equipment named by the payload, else the local ones.
    let study = match payload.get("study_sync_id") {
        None => local.as_ref().map(|l| l.study_id),
        Some(_) => match payload_ref(conn, payload, "study_sync_id", "reliability_studies").await? {
            Ok(id) => id,
            Err(()) => return Ok(Some(InboundDecisionVerdict::Defer("study not applied yet".into()))),
        },
    };
    let Some(study) = study else {
        return Ok(Some(InboundDecisionVerdict::Defer("study not applied yet".into())));
    };
    let equipment = match payload.get("equipment_sync_id") {
        None => local.as_ref().and_then(|l| l.equipment_id),
        Some(_) => match payload_ref(conn, payload, "equipment_sync_id", "equipment").await? {
            Ok(id) => id,
            Err(()) => return Ok(Some(InboundDecisionVerdict::Defer("equipment not applied yet".into()))),
        },
    };
    // The strictest of: the equipment's current site, the site the payload says was stored at
    // proposal, and the site stored here.
    let mut rule = self_approval_rule(conn, study, equipment).await?;
    let payload_recorded = payload
        .get("site_recorded")
        .and_then(|v| v.as_i64().or_else(|| v.as_bool().map(i64::from)))
        == Some(1);
    let mut payload_site = None;
    if payload_recorded {
        let Ok(site) = payload_ref(conn, payload, "site_node_sync_id", "org_nodes").await? else {
            return Ok(Some(InboundDecisionVerdict::Defer("site not applied yet".into())));
        };
        payload_site = site;
        rule = strictest(rule, site_rule(conn, site).await?);
    }
    if let Some(l) = local.as_ref().filter(|l| l.site_recorded) {
        // The site stored here is a node deleted here and the received site is the node replacing
        // it: a new organisation structure moved the decision (and the site's rules) there
        // (`org::validation::publish_model_with_remap`). The received site's rule is the one
        // judged; the deleted node no longer holds a rule.
        let replaced = match (l.site_node_id, payload_site) {
            (Some(held), Some(received)) if held != received => {
                if crate::org::validation::node_copy_awaits_activation(conn, Some(held), received).await? {
                    // The copy's structure is not activated here yet (the publish arrived in part).
                    return Ok(Some(InboundDecisionVerdict::Defer(
                        "the new organisation structure is not active here yet".into(),
                    )));
                }
                crate::org::validation::node_replaces_deleted_node(conn, held, received).await?
            }
            _ => false,
        };
        if !replaced {
            rule = strictest(rule, site_rule(conn, l.site_node_id).await?);
        }
    }
    Ok(match rule {
        SelfApprovalRule::Allowed => None,
        SelfApprovalRule::FourEyes => Some(InboundDecisionVerdict::Conflict(format!(
            "decision '{title}' was approved by the person who proposed it; its site requires a second \
             person (four-eyes)"
        ))),
        SelfApprovalRule::SiteUnresolved => Some(InboundDecisionVerdict::Conflict(format!(
            "decision '{title}' was approved by the person who proposed it; its site cannot be resolved \
             here, so four-eyes applies"
        ))),
    })
}
