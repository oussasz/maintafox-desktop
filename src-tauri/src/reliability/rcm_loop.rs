//! RCM / FMECA → PM loop (PRD §6.10.4–6.10.5, Phase 6.5).
//!
//! - **RCM decision logic** ([`recommend_tactic`]): the RCM decision diagram (SAE JA1011 /
//!   Moubray RCM II). Consequence first (hidden, safety/environmental, operational,
//!   non-operational), then the proactive tasks in order of preference: on-condition, scheduled
//!   restoration, scheduled discard, failure finding (hidden failures), combination (safety),
//!   then the default action (redesign when compulsory, otherwise run to failure). A Weibull
//!   shape β ≤ 1 (no wear-out) rules out age-based tasks. Every answer is kept in the decision
//!   path so the choice can be audited and challenged later.
//! - **Actions with traceability**: an FMECA item or an RCM decision creates a work order or a
//!   draft PM plan version; the source keeps the link and the PM version records its source.
//! - **Criticality number** (MIL-STD-1629A): mode criticality `Cm = β · α · λp · t`.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

// ── RCM decision logic (pure) ─────────────────────────────────────────────────────────────

pub const CONSEQUENCE_HIDDEN: &str = "hidden";
pub const CONSEQUENCE_SAFETY: &str = "safety_environmental";
pub const CONSEQUENCE_OPERATIONAL: &str = "operational";
pub const CONSEQUENCE_NON_OPERATIONAL: &str = "non_operational";

pub const TACTIC_CONDITION_BASED: &str = "condition_based";
pub const TACTIC_TIME_BASED: &str = "time_based";
pub const TACTIC_FAILURE_FINDING: &str = "failure_finding";
pub const TACTIC_RUN_TO_FAILURE: &str = "run_to_failure";
pub const TACTIC_REDESIGN: &str = "redesign";

/// Tactics that are carried out by preventive maintenance (a PM plan revision makes sense).
pub const PM_TACTICS: &[&str] = &[TACTIC_CONDITION_BASED, TACTIC_TIME_BASED, TACTIC_FAILURE_FINDING];

/// Answers to the RCM decision diagram for one failure mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RcmAnswers {
    /// Will the loss of function become evident to operators under normal circumstances?
    pub failure_evident: bool,
    /// Evident failures: could it hurt someone or breach an environmental standard?
    #[serde(default)]
    pub safety_or_environmental: bool,
    /// Evident, non-safety failures: does it affect output, quality, service or operating cost?
    #[serde(default)]
    pub operational_impact: bool,
    /// Hidden failures: would the multiple failure have safety or environmental consequences?
    #[serde(default)]
    pub multiple_failure_affects_safety: bool,
    /// Is an on-condition task technically feasible (detectable P-F interval) and worth doing?
    pub on_condition_feasible: bool,
    /// Is a scheduled restoration task feasible (identifiable wear-out age) and worth doing?
    pub scheduled_restoration_feasible: bool,
    /// Is a scheduled discard task feasible and worth doing?
    pub scheduled_discard_feasible: bool,
    /// Hidden failures: is a failure-finding task feasible and worth doing?
    #[serde(default)]
    pub failure_finding_feasible: bool,
    /// Safety failures: does a combination of tasks reduce the risk to a tolerable level?
    #[serde(default)]
    pub combination_feasible: bool,
    /// Weibull shape of the failure mode when known; β ≤ 1 means no wear-out.
    #[serde(default)]
    pub weibull_beta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RcmRecommendation {
    pub consequence_category: String,
    pub recommended_tactic: String,
    /// Redesign is compulsory (safety/environmental risk with no adequate task).
    pub redesign_compulsory: bool,
    /// Ordered decision path (`step:answer` codes, translated in the UI).
    pub path: Vec<String>,
}

fn step(path: &mut Vec<String>, code: &str, yes: bool) {
    path.push(format!("{code}:{}", if yes { "yes" } else { "no" }));
}

/// Walks the RCM decision diagram for one failure mode.
#[must_use]
pub fn recommend_tactic(a: &RcmAnswers) -> RcmRecommendation {
    let mut path = Vec::new();
    step(&mut path, "evident", a.failure_evident);
    let consequence = if !a.failure_evident {
        CONSEQUENCE_HIDDEN
    } else {
        step(&mut path, "safety_environmental", a.safety_or_environmental);
        if a.safety_or_environmental {
            CONSEQUENCE_SAFETY
        } else {
            step(&mut path, "operational", a.operational_impact);
            if a.operational_impact {
                CONSEQUENCE_OPERATIONAL
            } else {
                CONSEQUENCE_NON_OPERATIONAL
            }
        }
    };

    // Age-based tasks need wear-out: a fitted β ≤ 1 rules them out whatever was answered.
    let no_wear_out = a.weibull_beta.is_some_and(|b| b.is_finite() && b <= 1.0);
    if no_wear_out {
        path.push("no_wear_out:beta_le_1".into());
    }
    let restoration = a.scheduled_restoration_feasible && !no_wear_out;
    let discard = a.scheduled_discard_feasible && !no_wear_out;

    let done = |tactic: &str, compulsory: bool, path: Vec<String>| RcmRecommendation {
        consequence_category: consequence.to_string(),
        recommended_tactic: tactic.to_string(),
        redesign_compulsory: compulsory,
        path,
    };

    step(&mut path, "on_condition", a.on_condition_feasible);
    if a.on_condition_feasible {
        return done(TACTIC_CONDITION_BASED, false, path);
    }
    step(&mut path, "scheduled_restoration", restoration);
    if restoration {
        return done(TACTIC_TIME_BASED, false, path);
    }
    step(&mut path, "scheduled_discard", discard);
    if discard {
        return done(TACTIC_TIME_BASED, false, path);
    }
    match consequence {
        CONSEQUENCE_HIDDEN => {
            step(&mut path, "failure_finding", a.failure_finding_feasible);
            if a.failure_finding_feasible {
                return done(TACTIC_FAILURE_FINDING, false, path);
            }
            step(&mut path, "multiple_failure_safety", a.multiple_failure_affects_safety);
            if a.multiple_failure_affects_safety {
                return done(TACTIC_REDESIGN, true, path);
            }
            done(TACTIC_RUN_TO_FAILURE, false, path)
        }
        CONSEQUENCE_SAFETY => {
            step(&mut path, "combination", a.combination_feasible);
            if a.combination_feasible {
                return done(TACTIC_TIME_BASED, false, path);
            }
            done(TACTIC_REDESIGN, true, path)
        }
        _ => done(TACTIC_RUN_TO_FAILURE, false, path),
    }
}

// ── Criticality (pure) ────────────────────────────────────────────────────────────────────

/// MIL-STD-1629A failure mode criticality `Cm = β · α · λp · t`.
pub fn mode_criticality(beta: f64, alpha: f64, lambda_per_hour: f64, operating_hours: f64) -> AppResult<f64> {
    let checks = [
        (beta, "effect probability β", 0.0, 1.0),
        (alpha, "failure mode ratio α", 0.0, 1.0),
    ];
    for (v, name, lo, hi) in checks {
        if !v.is_finite() || v < lo || v > hi {
            return Err(AppError::ValidationFailed(vec![format!(
                "{name} must be between {lo} and {hi}."
            )]));
        }
    }
    if !lambda_per_hour.is_finite() || lambda_per_hour < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "failure rate λp must be a non-negative number of failures per hour.".into(),
        ]));
    }
    if !operating_hours.is_finite() || operating_hours <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "operating time t must be positive (hours).".into(),
        ]));
    }
    Ok(beta * alpha * lambda_per_hour * operating_hours)
}

// ── Persistence ───────────────────────────────────────────────────────────────────────────

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("rcm/fmeca loop decode '{field}': {err}"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyRcmLogicInput {
    pub decision_id: i64,
    pub expected_row_version: i64,
    pub answers: RcmAnswers,
    /// Tactic retained; the recommendation when omitted. A different choice needs a justification.
    #[serde(default)]
    pub selected_tactic: Option<String>,
    #[serde(default)]
    pub justification: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RcmDecisionLogic {
    pub decision_id: i64,
    pub row_version: i64,
    pub selected_tactic: String,
    pub consequence_category: String,
    pub justification: String,
    pub recommended_tactic: Option<String>,
    pub answers: Option<RcmAnswers>,
    pub path: Vec<String>,
    pub linked_pm_plan_id: Option<i64>,
    pub linked_pm_plan_version_id: Option<i64>,
    pub pm_plan_code: Option<String>,
    pub pm_version_no: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
}

const LOGIC_SELECT: &str = "SELECT d.id, d.row_version, d.selected_tactic, d.consequence_category, d.justification, \
     d.recommended_tactic, d.decision_inputs_json, d.decision_path_json, d.linked_pm_plan_id, \
     d.linked_pm_plan_version_id, p.code AS pm_plan_code, v.version_no AS pm_version_no, \
     d.linked_work_order_id, wo.code AS work_order_code \
     FROM rcm_decisions d \
     LEFT JOIN pm_plan_versions v ON v.id = d.linked_pm_plan_version_id \
     LEFT JOIN pm_plans p ON p.id = COALESCE(v.pm_plan_id, d.linked_pm_plan_id) \
     LEFT JOIN work_orders wo ON wo.id = d.linked_work_order_id";

fn map_logic(r: &sea_orm::QueryResult) -> AppResult<RcmDecisionLogic> {
    let inputs: Option<String> = r
        .try_get("", "decision_inputs_json")
        .map_err(|e| decode_err("decision_inputs_json", e))?;
    let path: Option<String> = r
        .try_get("", "decision_path_json")
        .map_err(|e| decode_err("decision_path_json", e))?;
    Ok(RcmDecisionLogic {
        decision_id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        selected_tactic: r
            .try_get("", "selected_tactic")
            .map_err(|e| decode_err("selected_tactic", e))?,
        consequence_category: r
            .try_get("", "consequence_category")
            .map_err(|e| decode_err("consequence_category", e))?,
        justification: r
            .try_get("", "justification")
            .map_err(|e| decode_err("justification", e))?,
        recommended_tactic: r
            .try_get("", "recommended_tactic")
            .map_err(|e| decode_err("recommended_tactic", e))?,
        answers: inputs
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| decode_err("decision_inputs_json", e))?,
        path: path
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| decode_err("decision_path_json", e))?
            .unwrap_or_default(),
        linked_pm_plan_id: r
            .try_get("", "linked_pm_plan_id")
            .map_err(|e| decode_err("linked_pm_plan_id", e))?,
        linked_pm_plan_version_id: r
            .try_get("", "linked_pm_plan_version_id")
            .map_err(|e| decode_err("linked_pm_plan_version_id", e))?,
        pm_plan_code: r
            .try_get("", "pm_plan_code")
            .map_err(|e| decode_err("pm_plan_code", e))?,
        pm_version_no: r
            .try_get("", "pm_version_no")
            .map_err(|e| decode_err("pm_version_no", e))?,
        linked_work_order_id: r
            .try_get("", "linked_work_order_id")
            .map_err(|e| decode_err("linked_work_order_id", e))?,
        work_order_code: r
            .try_get("", "work_order_code")
            .map_err(|e| decode_err("work_order_code", e))?,
    })
}

pub async fn get_rcm_decision_logic(db: &impl ConnectionTrait, decision_id: i64) -> AppResult<RcmDecisionLogic> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LOGIC_SELECT} WHERE d.id = ?"),
            [decision_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "RcmDecision".into(),
            id: decision_id.to_string(),
        })?;
    map_logic(&row)
}

pub async fn list_rcm_decision_logic(db: &impl ConnectionTrait, study_id: i64) -> AppResult<Vec<RcmDecisionLogic>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LOGIC_SELECT} WHERE d.study_id = ? ORDER BY d.id"),
            [study_id.into()],
        ))
        .await?;
    rows.iter().map(map_logic).collect()
}

/// Runs the decision logic and records answers, path, consequence, recommendation and the
/// retained tactic. Choosing another tactic than the recommendation requires a justification.
pub async fn apply_rcm_decision_logic(
    db: &DatabaseConnection,
    input: ApplyRcmLogicInput,
) -> AppResult<RcmDecisionLogic> {
    let rec = recommend_tactic(&input.answers);
    let selected = input
        .selected_tactic
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&rec.recommended_tactic)
        .to_string();
    let allowed = [
        TACTIC_CONDITION_BASED,
        TACTIC_TIME_BASED,
        TACTIC_FAILURE_FINDING,
        TACTIC_RUN_TO_FAILURE,
        TACTIC_REDESIGN,
    ];
    if !allowed.contains(&selected.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unknown tactic '{selected}'."
        )]));
    }
    let justification = input.justification.as_deref().map(str::trim).unwrap_or("").to_string();
    if selected != rec.recommended_tactic && justification.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "The decision logic recommends '{}': a justification is required to retain '{selected}'.",
            rec.recommended_tactic
        )]));
    }
    if rec.redesign_compulsory && selected != TACTIC_REDESIGN {
        return Err(AppError::ValidationFailed(vec![
            "Redesign is compulsory: no task brings the safety or environmental risk to a tolerable level.".into(),
        ]));
    }
    let txn = db.begin().await?;
    let aff = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rcm_decisions SET consequence_category = ?, selected_tactic = ?, justification = ?, \
             recommended_tactic = ?, decision_inputs_json = ?, decision_path_json = ?, \
             row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
            [
                rec.consequence_category.clone().into(),
                selected.into(),
                justification.into(),
                rec.recommended_tactic.clone().into(),
                serde_json::to_string(&input.answers)?.into(),
                serde_json::to_string(&rec.path)?.into(),
                Utc::now().to_rfc3339().into(),
                input.decision_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "rcm_decisions update conflict (stale row_version or unknown decision).".into(),
        ]));
    }
    let out = get_rcm_decision_logic(&txn, input.decision_id).await?;
    txn.commit().await?;
    Ok(out)
}

// ── Actions: PM revision / work order from an FMECA item or an RCM decision ────────────────

pub const SOURCE_FMECA_ITEM: &str = "fmeca_item";
pub const SOURCE_RCM_DECISION: &str = "rcm_decision";
/// A decision of a reliability study (`reliability_study_decisions`).
pub const SOURCE_RELIABILITY_DECISION: &str = "reliability_study_decision";

/// Table that keeps the link back to the PM version / work order created from a source.
fn source_table(source_type: &str) -> &'static str {
    match source_type {
        SOURCE_FMECA_ITEM => "fmeca_items",
        SOURCE_RELIABILITY_DECISION => "reliability_study_decisions",
        _ => "rcm_decisions",
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PmRevisionFromSourceInput {
    /// `fmeca_item`, `rcm_decision` or `reliability_study_decision`.
    pub source_type: String,
    pub source_id: i64,
    pub pm_plan_id: i64,
    /// New trigger (e.g. a shorter interval); the current version's trigger when omitted.
    #[serde(default)]
    pub trigger_definition_json: Option<String>,
    /// Why the plan changes (kept as the version's change reason).
    pub change_note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAction {
    pub source_type: String,
    pub source_id: i64,
    pub pm_plan_version_id: Option<i64>,
    pub pm_version_no: Option<i64>,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
}

struct SourceContext {
    equipment_id: i64,
    label: String,
    /// RCM tactic, when the source is a decision.
    tactic: Option<String>,
}

async fn load_source(db: &impl ConnectionTrait, source_type: &str, source_id: i64) -> AppResult<SourceContext> {
    let (sql, label_col) = match source_type {
        SOURCE_FMECA_ITEM => (
            "SELECT a.equipment_id, i.functional_failure AS label, NULL AS tactic, i.review_required \
             FROM fmeca_items i JOIN fmeca_analyses a ON a.id = i.analysis_id WHERE i.id = ?",
            "functional_failure",
        ),
        SOURCE_RCM_DECISION => (
            "SELECT s.equipment_id, d.functional_failure AS label, d.selected_tactic AS tactic, 0 AS review_required FROM rcm_decisions d \
             JOIN rcm_studies s ON s.id = d.study_id WHERE d.id = ?",
            "functional_failure",
        ),
        // The decision acts on one equipment of its study (set when the decision is recorded).
        SOURCE_RELIABILITY_DECISION => (
            "SELECT d.equipment_id, d.title AS label, NULL AS tactic, 0 AS review_required FROM reliability_study_decisions d WHERE d.id = ? AND d.equipment_id IS NOT NULL",
            "title",
        ),
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "source_type must be '{SOURCE_FMECA_ITEM}', '{SOURCE_RCM_DECISION}' or '{SOURCE_RELIABILITY_DECISION}' (got '{other}')."
            )]))
        }
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [source_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: source_type.into(),
            id: source_id.to_string(),
        })?;
    if row
        .try_get::<i64>("", "review_required")
        .map_err(|e| decode_err("review_required", e))?
        != 0
    {
        // An FMECA row whose ratings nobody chose never drives a PM or work order (migration 221).
        return Err(AppError::ValidationFailed(vec!["FMECA_ITEM_REVIEW_REQUIRED".into()]));
    }
    Ok(SourceContext {
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        label: row.try_get("", "label").map_err(|e| decode_err(label_col, e))?,
        tactic: row.try_get("", "tactic").map_err(|e| decode_err("tactic", e))?,
    })
}

/// Creates a draft PM plan version from the plan's latest version (new trigger when given),
/// records its source, and links the source to it. The version stays a draft: publishing it
/// goes through the normal PM review.
pub async fn create_pm_revision_from_source(
    db: &DatabaseConnection,
    input: PmRevisionFromSourceInput,
) -> AppResult<SourceAction> {
    let ctx = load_source(db, &input.source_type, input.source_id).await?;
    if let Some(tactic) = &ctx.tactic {
        if !PM_TACTICS.contains(&tactic.as_str()) {
            return Err(AppError::ValidationFailed(vec![format!(
                "A '{tactic}' decision is not carried out by preventive maintenance."
            )]));
        }
    }
    let note = input.change_note.trim();
    if note.is_empty() {
        return Err(AppError::ValidationFailed(vec!["A change note is required.".into()]));
    }
    let latest = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT trigger_definition_json, task_package_json, required_parts_json, required_skills_json, \
             required_tools_json, estimated_duration_hours, planned_crew_size, estimated_labor_cost, estimated_parts_cost, \
             estimated_service_cost FROM pm_plan_versions WHERE pm_plan_id = ? ORDER BY version_no DESC LIMIT 1",
            [input.pm_plan_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "The PM plan has no version yet: create its first version in the PM module.".into(),
            ])
        })?;
    let get_s = |c: &str| -> AppResult<Option<String>> { latest.try_get("", c).map_err(|e| decode_err(c, e)) };
    let get_f = |c: &str| -> AppResult<Option<f64>> { latest.try_get("", c).map_err(|e| decode_err(c, e)) };
    let trigger = match input
        .trigger_definition_json
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        Some(t) => t,
        None => get_s("trigger_definition_json")?.ok_or_else(|| decode_err("trigger_definition_json", "missing"))?,
    };
    let version = crate::pm::queries::create_pm_plan_version(
        db,
        input.pm_plan_id,
        crate::pm::domain::CreatePmPlanVersionInput {
            effective_from: Utc::now().to_rfc3339(),
            effective_to: None,
            trigger_definition_json: trigger,
            task_package_json: get_s("task_package_json")?,
            // Copied part lines go through the same rule as a revision made in the PM module
            // (`pm::plan_tools`): a flag-less line (stored before migration 231 or received from a
            // PC that has not run it) is read as required, so the copy is not refused.
            required_parts_json: crate::pm::wo_content::normalized_required_parts_json(
                get_s("required_parts_json")?.as_deref(),
            ),
            required_skills_json: get_s("required_skills_json")?,
            required_tools_json: get_s("required_tools_json")?,
            estimated_duration_hours: get_f("estimated_duration_hours")?,
            planned_crew_size: latest
                .try_get("", "planned_crew_size")
                .map_err(|e| decode_err("planned_crew_size", e))?,
            estimated_labor_cost: get_f("estimated_labor_cost")?,
            estimated_parts_cost: get_f("estimated_parts_cost")?,
            estimated_service_cost: get_f("estimated_service_cost")?,
            change_reason: Some(format!("{note} — {}: {}", input.source_type, ctx.label)),
        },
    )
    .await?;

    // The draft exists from here: link it in one transaction; on failure the error names it.
    let link = async {
        let txn = db.begin().await?;
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE pm_plan_versions SET source_type = ?, source_id = ? WHERE id = ?",
            [
                input.source_type.clone().into(),
                input.source_id.into(),
                version.id.into(),
            ],
        ))
        .await?;
        let table = source_table(&input.source_type);
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE {table} SET linked_pm_plan_id = ?, linked_pm_plan_version_id = ?, \
                 row_version = row_version + 1, updated_at = ? WHERE id = ?"
            ),
            [
                input.pm_plan_id.into(),
                version.id.into(),
                Utc::now().to_rfc3339().into(),
                input.source_id.into(),
            ],
        ))
        .await?;
        txn.commit().await?;
        AppResult::Ok(())
    };
    link.await.map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "Draft version {} was created but could not be linked to its source: {e}",
            version.version_no
        )])
    })?;
    Ok(SourceAction {
        source_type: input.source_type,
        source_id: input.source_id,
        pm_plan_version_id: Some(version.id),
        pm_version_no: Some(version.version_no),
        work_order_id: None,
        work_order_code: None,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoFromSourceInput {
    pub source_type: String,
    pub source_id: i64,
    /// Work order type code (e.g. `corrective`, `improvement`).
    pub type_code: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// Creates a work order on the source's equipment and links the source to it.
pub async fn create_wo_from_source(
    db: &DatabaseConnection,
    input: WoFromSourceInput,
    actor_id: i64,
) -> AppResult<SourceAction> {
    let ctx = load_source(db, &input.source_type, input.source_id).await?;
    if input.source_type == SOURCE_RCM_DECISION && ctx.tactic.as_deref() == Some(TACTIC_RUN_TO_FAILURE) {
        return Err(AppError::ValidationFailed(vec![
            "A run-to-failure decision does not call for a work order.".into(),
        ]));
    }
    let title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map_or_else(|| ctx.label.clone(), str::to_string);
    if title.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "A work order title is required.".into()
        ]));
    }
    let wo = crate::wo::queries::create_work_order(
        db,
        crate::wo::domain::WoCreateInput {
            type_code: input.type_code.clone(),
            equipment_id: Some(ctx.equipment_id),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: Some(format!("{}: {}", input.source_type, ctx.label)),
            entity_id: None,
            planner_id: None,
            urgency_code: None,
            subtype_code: None,
            title,
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor_id,
            requires_permit: None,
        },
    )
    .await?;
    let table = source_table(&input.source_type);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE {table} SET linked_work_order_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?"
        ),
        [wo.id.into(), Utc::now().to_rfc3339().into(), input.source_id.into()],
    ))
    .await
    .map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "Work order {} was created but could not be linked to its source: {e}",
            wo.code
        )])
    })?;
    Ok(SourceAction {
        source_type: input.source_type,
        source_id: input.source_id,
        pm_plan_version_id: None,
        pm_version_no: None,
        work_order_id: Some(wo.id),
        work_order_code: Some(wo.code),
    })
}

// ── FMECA criticality ─────────────────────────────────────────────────────────────────────

/// α and λp observed in the failure history of the analysis equipment.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CriticalityEvidence {
    pub mode_failures: i64,
    pub total_failures: i64,
    /// α = mode failures / all eligible failures; `None` without failure history or failure mode.
    pub mode_ratio: Option<f64>,
    /// λp = eligible failures per exposure hour from the latest KPI snapshot of the equipment.
    pub failure_rate_per_hour: Option<f64>,
    pub kpi_period_end: Option<String>,
}

pub async fn fmeca_criticality_evidence(db: &impl ConnectionTrait, item_id: i64) -> AppResult<CriticalityEvidence> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.equipment_id, i.failure_mode_id, i.review_required FROM fmeca_items i \
             JOIN fmeca_analyses a ON a.id = i.analysis_id WHERE i.id = ?",
            [item_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "FmecaItem".into(),
            id: item_id.to_string(),
        })?;
    if row
        .try_get::<i64>("", "review_required")
        .map_err(|e| decode_err("review_required", e))?
        != 0
    {
        // Rate the row in the FMECA first: its criticality would rest on ratings nobody chose.
        return Err(AppError::ValidationFailed(vec!["FMECA_ITEM_REVIEW_REQUIRED".into()]));
    }
    let equipment_id: i64 = row
        .try_get("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
    let mode: Option<i64> = row
        .try_get("", "failure_mode_id")
        .map_err(|e| decode_err("failure_mode_id", e))?;
    let counts = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS total, \
             SUM(CASE WHEN ? IS NOT NULL AND failure_mode_id = ? THEN 1 ELSE 0 END) AS mode_n \
             FROM failure_events WHERE equipment_id = ? \
             AND COALESCE(json_extract(eligible_flags_json, '$.eligible_unplanned_mtbf'), 0) = 1",
            [mode.into(), mode.into(), equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failure counts")))?;
    let total: i64 = counts.try_get("", "total").map_err(|e| decode_err("total", e))?;
    let mode_n: Option<i64> = counts.try_get("", "mode_n").map_err(|e| decode_err("mode_n", e))?;
    let mode_n = mode_n.unwrap_or(0);
    let kpi = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT failure_rate, period_end FROM reliability_kpi_snapshots \
             WHERE equipment_id = ? AND failure_rate IS NOT NULL ORDER BY period_end DESC, id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    let (rate, period_end) = match kpi {
        Some(r) => (
            r.try_get::<Option<f64>>("", "failure_rate")
                .map_err(|e| decode_err("failure_rate", e))?,
            r.try_get::<Option<String>>("", "period_end")
                .map_err(|e| decode_err("period_end", e))?,
        ),
        None => (None, None),
    };
    Ok(CriticalityEvidence {
        mode_failures: mode_n,
        total_failures: total,
        mode_ratio: (mode.is_some() && total > 0).then(|| mode_n as f64 / total as f64),
        failure_rate_per_hour: rate,
        kpi_period_end: period_end,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct FmecaCriticalityInput {
    pub item_id: i64,
    pub expected_row_version: i64,
    /// β: conditional probability that the failure mode causes the effect (1, 0.5, 0.1, 0 …).
    pub effect_probability: f64,
    /// α; taken from the failure history when omitted.
    #[serde(default)]
    pub mode_ratio: Option<f64>,
    /// λp (failures per hour); taken from the latest KPI snapshot when omitted.
    #[serde(default)]
    pub failure_rate_per_hour: Option<f64>,
    pub operating_hours: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FmecaCriticality {
    pub item_id: i64,
    pub row_version: i64,
    pub effect_probability: f64,
    pub mode_ratio: f64,
    pub failure_rate_per_hour: f64,
    pub operating_hours: f64,
    pub criticality_number: f64,
    /// Σ Cm of the items of the same analysis (item criticality Cr), rows to review excluded.
    pub analysis_criticality: f64,
    /// Rows of the analysis still to review, not counted in Cr.
    pub analysis_rows_to_review: i64,
}

pub async fn compute_fmeca_criticality(
    db: &DatabaseConnection,
    input: FmecaCriticalityInput,
) -> AppResult<FmecaCriticality> {
    let evidence = fmeca_criticality_evidence(db, input.item_id).await?;
    let alpha = input.mode_ratio.or(evidence.mode_ratio).ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "No failure history for this failure mode: enter the failure mode ratio α.".into(),
        ])
    })?;
    let lambda = input
        .failure_rate_per_hour
        .or(evidence.failure_rate_per_hour)
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "No failure rate is known for this equipment: enter λp (failures per hour).".into(),
            ])
        })?;
    let cm = mode_criticality(input.effect_probability, alpha, lambda, input.operating_hours)?;
    let txn = db.begin().await?;
    let aff = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE fmeca_items SET effect_probability = ?, mode_ratio = ?, failure_rate_per_hour = ?, \
             operating_hours = ?, criticality_number = ?, row_version = row_version + 1, updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.effect_probability.into(),
                alpha.into(),
                lambda.into(),
                input.operating_hours.into(),
                cm.into(),
                Utc::now().to_rfc3339().into(),
                input.item_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "fmeca_items update conflict (stale row_version).".into(),
        ]));
    }
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT i.row_version, (SELECT COALESCE(SUM(j.criticality_number), 0.0) FROM fmeca_items j \
             WHERE j.analysis_id = i.analysis_id AND j.review_required = 0) AS cr, \
             (SELECT COUNT(*) FROM fmeca_items k WHERE k.analysis_id = i.analysis_id AND k.review_required = 1) \
             AS to_review FROM fmeca_items i WHERE i.id = ?",
            [input.item_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("fmeca item vanished")))?;
    let out = FmecaCriticality {
        item_id: input.item_id,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        effect_probability: input.effect_probability,
        mode_ratio: alpha,
        failure_rate_per_hour: lambda,
        operating_hours: input.operating_hours,
        criticality_number: cm,
        analysis_criticality: row.try_get("", "cr").map_err(|e| decode_err("cr", e))?,
        analysis_rows_to_review: row.try_get("", "to_review").map_err(|e| decode_err("to_review", e))?,
    };
    txn.commit().await?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> RcmAnswers {
        RcmAnswers {
            failure_evident: true,
            safety_or_environmental: false,
            operational_impact: true,
            multiple_failure_affects_safety: false,
            on_condition_feasible: false,
            scheduled_restoration_feasible: false,
            scheduled_discard_feasible: false,
            failure_finding_feasible: false,
            combination_feasible: false,
            weibull_beta: None,
        }
    }

    #[test]
    fn evident_operational_prefers_on_condition_then_age_tasks_then_run_to_failure() {
        let mut a = answers();
        a.on_condition_feasible = true;
        let r = recommend_tactic(&a);
        assert_eq!(r.consequence_category, CONSEQUENCE_OPERATIONAL);
        assert_eq!(r.recommended_tactic, TACTIC_CONDITION_BASED);
        assert_eq!(
            r.path,
            vec![
                "evident:yes",
                "safety_environmental:no",
                "operational:yes",
                "on_condition:yes"
            ]
        );

        a.on_condition_feasible = false;
        a.scheduled_restoration_feasible = true;
        assert_eq!(recommend_tactic(&a).recommended_tactic, TACTIC_TIME_BASED);

        a.scheduled_restoration_feasible = false;
        let r = recommend_tactic(&a);
        assert_eq!(r.recommended_tactic, TACTIC_RUN_TO_FAILURE);
        assert!(!r.redesign_compulsory);
    }

    #[test]
    fn no_wear_out_rules_out_age_based_tasks() {
        let mut a = answers();
        a.scheduled_restoration_feasible = true;
        a.scheduled_discard_feasible = true;
        a.weibull_beta = Some(0.8);
        let r = recommend_tactic(&a);
        assert_eq!(r.recommended_tactic, TACTIC_RUN_TO_FAILURE);
        assert!(r.path.contains(&"no_wear_out:beta_le_1".to_string()));
        a.weibull_beta = Some(2.4);
        assert_eq!(recommend_tactic(&a).recommended_tactic, TACTIC_TIME_BASED);
    }

    #[test]
    fn hidden_failures_go_to_failure_finding_or_redesign() {
        let mut a = answers();
        a.failure_evident = false;
        a.failure_finding_feasible = true;
        let r = recommend_tactic(&a);
        assert_eq!(r.consequence_category, CONSEQUENCE_HIDDEN);
        assert_eq!(r.recommended_tactic, TACTIC_FAILURE_FINDING);

        a.failure_finding_feasible = false;
        a.multiple_failure_affects_safety = true;
        let r = recommend_tactic(&a);
        assert_eq!(r.recommended_tactic, TACTIC_REDESIGN);
        assert!(r.redesign_compulsory);

        a.multiple_failure_affects_safety = false;
        assert_eq!(recommend_tactic(&a).recommended_tactic, TACTIC_RUN_TO_FAILURE);
    }

    #[test]
    fn safety_without_tasks_makes_redesign_compulsory() {
        let mut a = answers();
        a.safety_or_environmental = true;
        let r = recommend_tactic(&a);
        assert_eq!(r.consequence_category, CONSEQUENCE_SAFETY);
        assert_eq!(r.recommended_tactic, TACTIC_REDESIGN);
        assert!(r.redesign_compulsory);
        a.combination_feasible = true;
        assert_eq!(recommend_tactic(&a).recommended_tactic, TACTIC_TIME_BASED);
    }

    #[test]
    fn non_operational_consequence() {
        let mut a = answers();
        a.operational_impact = false;
        assert_eq!(recommend_tactic(&a).consequence_category, CONSEQUENCE_NON_OPERATIONAL);
    }

    #[test]
    fn mode_criticality_matches_mil_std_formula() {
        // β = 0.5, α = 0.3, λp = 2e-4 /h, t = 1000 h → Cm = 0.03.
        let cm = mode_criticality(0.5, 0.3, 2e-4, 1000.0).expect("cm");
        assert!((cm - 0.03).abs() < 1e-12);
        assert!(mode_criticality(1.5, 0.3, 2e-4, 1000.0).is_err());
        assert!(mode_criticality(0.5, -0.1, 2e-4, 1000.0).is_err());
        assert!(mode_criticality(0.5, 0.3, -1.0, 1000.0).is_err());
        assert!(mode_criticality(0.5, 0.3, 2e-4, 0.0).is_err());
    }
}
