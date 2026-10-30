//! Study step 4 — RCM decision (SAE JA1011 logic of `rcm_loop`, RCM backend reused), one FMECA
//! row at a time.
//!
//! Answers are pre-filled from the FMECA row and the life analysis, each with its basis:
//! - evident or hidden: the detection level of the plant scale (`hidden` flag);
//! - safety / environmental, operational: the RCM consequence class of the severity level;
//! - scheduled restoration / discard feasible: a wear-out pattern of the fitted life (β lower
//!   bound > 1) for the study mode; ruled out when the fit shows no wear-out;
//! - on-condition feasible: a P-F interval stated by the analyst (task every P-F / 2);
//! - failure finding and combination of tasks: left to the analyst.
//!
//! The decision is recorded in `rcm_decisions` (linked to the FMECA item) and sent to the study
//! decision log by the analyst.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reliability::advanced_rams::domain::{CreateRcmStudyInput, UpsertRcmDecisionInput};
use crate::reliability::advanced_rams::queries as rams_queries;
use crate::reliability::rcm_loop::{
    apply_rcm_decision_logic, get_rcm_decision_logic, recommend_tactic, ApplyRcmLogicInput, RcmAnswers,
    RcmRecommendation,
};
use crate::reliability::study::analysis::life_analysis;
use crate::reliability::study::domain::{LifeAnalysisRequest, ReliabilityStudy};
use crate::reliability::study::fmeca::{rows_to_review, study_fmeca_analysis_id};
use crate::reliability::study::life::PATTERN_WEAR_OUT;
use crate::reliability::study::scales::{load_scales, RatingScales};
use crate::reliability::study::store::{get, get_study, opt_f64, STUDY_ACTIVE};

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

/// Why a question has its pre-filled answer.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnswerBasis {
    /// `RcmAnswers` field.
    pub question: String,
    /// `detection` | `severity` | `weibull_wear_out` | `weibull_no_wear_out` | `no_fit`
    /// | `pf_interval` | `pf_unknown` | `analyst`.
    pub basis: String,
    pub score: Option<i64>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedRcm {
    pub decision_id: i64,
    pub row_version: i64,
    pub selected_tactic: String,
    pub recommended_tactic: Option<String>,
    pub justification: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RcmRow {
    pub item_id: i64,
    pub component_label: Option<String>,
    pub mode_code: Option<String>,
    pub mode_label: Option<String>,
    pub functional_failure: String,
    pub severity: i64,
    pub occurrence: i64,
    pub detectability: i64,
    pub rpn: i64,
    /// Saved answers, else the pre-filled ones.
    pub answers: RcmAnswers,
    pub prefilled: RcmAnswers,
    pub bases: Vec<AnswerBasis>,
    pub recommendation: RcmRecommendation,
    pub saved: Option<SavedRcm>,
    pub pf_interval_hours: Option<f64>,
    /// On-condition task interval (P-F / 2).
    pub inspection_interval_hours: Option<f64>,
    /// Cost-optimal replacement age when the row is the study mode (step 2).
    pub replacement_interval_hours: Option<f64>,
    pub beta: Option<f64>,
    pub pattern: Option<String>,
    pub decision_id: Option<i64>,
    pub decision_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RcmWorksheet {
    pub study_id: i64,
    pub rcm_study_id: Option<i64>,
    pub rows: Vec<RcmRow>,
    pub editable: bool,
    pub time_basis: String,
    /// FMECA rows still to review (ratings nobody chose): not offered for an RCM decision.
    pub rows_to_review: i64,
    /// Set while the plant's FMECA grids must be reviewed (`scales::RatingScales::review`): the
    /// consequences and ratings read here come from the Maintafox grids standing in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grids_review: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveRcmInput {
    pub study_id: i64,
    pub fmeca_item_id: i64,
    pub answers: RcmAnswers,
    #[serde(default)]
    pub selected_tactic: Option<String>,
    #[serde(default)]
    pub justification: Option<String>,
    #[serde(default)]
    pub pf_interval_hours: Option<f64>,
}

/// Pre-filled answers for an FMECA row (pure).
#[must_use]
pub fn prefill_answers(
    scales: &RatingScales,
    severity: i64,
    detectability: i64,
    fit: Option<(f64, &str)>,
    pf_interval_hours: Option<f64>,
) -> (RcmAnswers, Vec<AnswerBasis>) {
    let hidden = scales
        .detection_level(detectability)
        .and_then(|l| l.hidden)
        .unwrap_or(false);
    let consequence = scales
        .severity_level(severity)
        .and_then(|l| l.consequence.clone())
        .unwrap_or_default();
    let safety = consequence == "safety";
    let operational = consequence == "operational";
    let pf = pf_interval_hours.filter(|v| v.is_finite() && *v > 0.0);
    let (wear_out, fit_basis) = match fit {
        Some((_, pattern)) if pattern == PATTERN_WEAR_OUT => (true, "weibull_wear_out"),
        Some(_) => (false, "weibull_no_wear_out"),
        None => (false, "no_fit"),
    };
    let b = |q: &str, basis: &str, score: Option<i64>, value: Option<f64>| AnswerBasis {
        question: q.into(),
        basis: basis.into(),
        score,
        value,
    };
    let beta = fit.map(|f| f.0);
    let answers = RcmAnswers {
        failure_evident: !hidden,
        safety_or_environmental: safety,
        operational_impact: operational,
        multiple_failure_affects_safety: hidden && safety,
        on_condition_feasible: pf.is_some(),
        scheduled_restoration_feasible: wear_out,
        scheduled_discard_feasible: wear_out,
        failure_finding_feasible: false,
        combination_feasible: false,
        weibull_beta: beta,
    };
    let bases = vec![
        b("failure_evident", "detection", Some(detectability), None),
        b("safety_or_environmental", "severity", Some(severity), None),
        b("operational_impact", "severity", Some(severity), None),
        b("multiple_failure_affects_safety", "severity", Some(severity), None),
        b(
            "on_condition_feasible",
            if pf.is_some() { "pf_interval" } else { "pf_unknown" },
            None,
            pf,
        ),
        b("scheduled_restoration_feasible", fit_basis, None, beta),
        b("scheduled_discard_feasible", fit_basis, None, beta),
        b("failure_finding_feasible", "analyst", None, None),
        b("combination_feasible", "analyst", None, None),
    ];
    (answers, bases)
}

pub async fn study_rcm_study_id(db: &impl ConnectionTrait, study_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rcm_study_id FROM reliability_studies WHERE id = ?",
            [study_id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| get::<Option<i64>>(&r, "rcm_study_id"))
        .transpose()?
        .flatten())
}

struct StudyFit {
    beta: f64,
    pattern: String,
    replacement: Option<f64>,
}

async fn study_fit(db: &DatabaseConnection, study: &ReliabilityStudy) -> AppResult<(Option<StudyFit>, String)> {
    let (a, _) = life_analysis(
        db,
        LifeAnalysisRequest {
            study_id: study.id,
            ..LifeAnalysisRequest::default()
        },
    )
    .await?;
    let basis = a.effective_time_basis.clone();
    Ok((
        a.fit.map(|f| StudyFit {
            beta: f.beta,
            pattern: f.pattern,
            replacement: a.cost.and_then(|c| c.proposed).and_then(|p| p.interval),
        }),
        basis,
    ))
}

pub async fn get_worksheet(db: &DatabaseConnection, study_id: i64) -> AppResult<RcmWorksheet> {
    let study = get_study(db, study_id).await?;
    let scales = load_scales(db).await?;
    let (fit, time_basis) = study_fit(db, &study).await?;
    let mut rows = Vec::new();
    if let Some(analysis_id) = study_fmeca_analysis_id(db, study_id).await? {
        let items = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT i.id, i.component_label, i.failure_mode_id, m.code AS mode_code, m.label AS mode_label,
                        i.functional_failure, i.severity, i.occurrence, i.detectability, i.rpn,
                        (SELECT d.id FROM rcm_decisions d WHERE d.fmeca_item_id = i.id ORDER BY d.id DESC LIMIT 1) AS rcm_id,
                        (SELECT d.pf_interval_hours FROM rcm_decisions d WHERE d.fmeca_item_id = i.id
                          ORDER BY d.id DESC LIMIT 1) AS pf,
                        (SELECT x.id FROM reliability_study_decisions x WHERE x.fmeca_item_id = i.id
                          ORDER BY x.created_at DESC, x.id DESC LIMIT 1) AS decision_id,
                        (SELECT x.status FROM reliability_study_decisions x WHERE x.fmeca_item_id = i.id
                          ORDER BY x.created_at DESC, x.id DESC LIMIT 1) AS decision_status
                 FROM fmeca_items i LEFT JOIN reference_values m ON m.id = i.failure_mode_id
                 WHERE i.analysis_id = ? AND i.review_required = 0 ORDER BY i.rpn DESC, i.id ASC",
                [analysis_id.into()],
            ))
            .await?;
        for r in &items {
            let mode: Option<i64> = get(r, "failure_mode_id")?;
            let is_study_mode = match (mode, study.failure_mode_id) {
                (Some(m), Some(s)) => {
                    m == s
                        || db
                            .query_one(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                "SELECT 1 FROM reference_values WHERE id = ? AND parent_id = ?",
                                [m.into(), s.into()],
                            ))
                            .await?
                            .is_some()
                }
                _ => false,
            };
            let mode_fit = fit.as_ref().filter(|_| is_study_mode);
            let severity: i64 = get(r, "severity")?;
            let detectability: i64 = get(r, "detectability")?;
            let pf: Option<f64> = get(r, "pf")?;
            let (prefilled, bases) = prefill_answers(
                &scales,
                severity,
                detectability,
                mode_fit.map(|f| (f.beta, f.pattern.as_str())),
                pf,
            );
            let rcm_id: Option<i64> = get(r, "rcm_id")?;
            let saved = match rcm_id {
                Some(id) => {
                    let logic = get_rcm_decision_logic(db, id).await?;
                    Some((
                        logic.answers.clone(),
                        SavedRcm {
                            decision_id: id,
                            row_version: logic.row_version,
                            selected_tactic: logic.selected_tactic,
                            recommended_tactic: logic.recommended_tactic,
                            justification: logic.justification,
                            path: logic.path,
                        },
                    ))
                }
                None => None,
            };
            let answers = saved
                .as_ref()
                .and_then(|(a, s)| a.clone().filter(|_| s.recommended_tactic.is_some()))
                .unwrap_or_else(|| prefilled.clone());
            let recommendation = recommend_tactic(&answers);
            rows.push(RcmRow {
                item_id: get(r, "id")?,
                component_label: get(r, "component_label")?,
                mode_code: get(r, "mode_code")?,
                mode_label: get(r, "mode_label")?,
                functional_failure: get(r, "functional_failure")?,
                severity,
                occurrence: get(r, "occurrence")?,
                detectability,
                rpn: get(r, "rpn")?,
                answers,
                prefilled,
                bases,
                recommendation,
                saved: saved.map(|(_, s)| s),
                pf_interval_hours: pf,
                inspection_interval_hours: pf.filter(|v| *v > 0.0).map(|v| v / 2.0),
                replacement_interval_hours: mode_fit.and_then(|f| f.replacement),
                beta: mode_fit.map(|f| f.beta),
                pattern: mode_fit.map(|f| f.pattern.clone()),
                decision_id: get(r, "decision_id")?,
                decision_status: get(r, "decision_status")?,
            });
        }
    }
    let rows_to_review = rows_to_review(db, study_id).await?;
    Ok(RcmWorksheet {
        study_id,
        rcm_study_id: study_rcm_study_id(db, study_id).await?,
        rows,
        editable: study.status == STUDY_ACTIVE,
        time_basis,
        rows_to_review,
        grids_review: scales.review.clone(),
    })
}

async fn ensure_rcm_study(db: &DatabaseConnection, study: &ReliabilityStudy, actor_id: i32) -> AppResult<i64> {
    if let Some(id) = study_rcm_study_id(db, study.id).await? {
        return Ok(id);
    }
    let first = study.assets.first().ok_or_else(|| invalid("STUDY_NO_ASSETS"))?;
    let rcm = rams_queries::create_rcm_study(
        db,
        Some(actor_id),
        CreateRcmStudyInput {
            equipment_id: first.equipment_id,
            title: format!("MBF · {}", study.title),
            status: None,
        },
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reliability_studies SET rcm_study_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [rcm.id.into(), crate::clock::now().to_rfc3339().into(), study.id.into()],
    ))
    .await?;
    Ok(rcm.id)
}

pub async fn save_decision(db: &DatabaseConnection, input: SaveRcmInput, actor_id: i32) -> AppResult<RcmWorksheet> {
    let study = get_study(db, input.study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    let analysis = study_fmeca_analysis_id(db, study.id)
        .await?
        .ok_or_else(|| invalid("RCM_FMECA_ITEM_NOT_IN_STUDY"))?;
    let item = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT component_label, functional_failure, failure_mode_id, review_required FROM fmeca_items              WHERE id = ? AND analysis_id = ?",
            [input.fmeca_item_id.into(), analysis.into()],
        ))
        .await?
        .ok_or_else(|| invalid("RCM_FMECA_ITEM_NOT_IN_STUDY"))?;
    if get::<i64>(&item, "review_required")? != 0 {
        // Its ratings were never chosen by a person: rate it in the FMECA step first.
        return Err(invalid("FMECA_ITEM_REVIEW_REQUIRED"));
    }
    if let Some(pf) = input.pf_interval_hours {
        if !(pf.is_finite() && pf > 0.0) {
            return Err(invalid("RCM_PF_INTERVAL_INVALID"));
        }
    }
    let rcm_study = ensure_rcm_study(db, &study, actor_id).await?;
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_version FROM rcm_decisions WHERE fmeca_item_id = ? AND study_id = ? ORDER BY id DESC LIMIT 1",
            [input.fmeca_item_id.into(), rcm_study.into()],
        ))
        .await?;
    let recommended = recommend_tactic(&input.answers).recommended_tactic;
    let (decision_id, row_version) = match existing {
        Some(r) => (get::<i64>(&r, "id")?, get::<i64>(&r, "row_version")?),
        None => {
            let d = rams_queries::upsert_rcm_decision(
                db,
                UpsertRcmDecisionInput {
                    id: None,
                    study_id: rcm_study,
                    expected_row_version: None,
                    function_description: get::<Option<String>>(&item, "component_label")?,
                    functional_failure: Some(get::<String>(&item, "functional_failure")?),
                    failure_mode_id: get::<Option<i64>>(&item, "failure_mode_id")?,
                    consequence_category: None,
                    selected_tactic: recommended.clone(),
                    justification: None,
                    review_due_at: None,
                    linked_pm_plan_id: None,
                },
            )
            .await?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE rcm_decisions SET fmeca_item_id = ? WHERE id = ?",
                [input.fmeca_item_id.into(), d.id.into()],
            ))
            .await?;
            (d.id, d.row_version)
        }
    };
    let logic = apply_rcm_decision_logic(
        db,
        ApplyRcmLogicInput {
            decision_id,
            expected_row_version: row_version,
            answers: input.answers,
            selected_tactic: input.selected_tactic,
            justification: input.justification,
        },
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE rcm_decisions SET pf_interval_hours = ? WHERE id = ?",
        [opt_f64(input.pf_interval_hours), logic.decision_id.into()],
    ))
    .await?;
    get_worksheet(db, input.study_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reliability::study::scales::default_scales;

    #[test]
    fn mockup_bearing_prefill_leads_to_condition_based_then_age_replacement() {
        let s = default_scales();
        // G 8 (stop > 4 h, operational), D 5 (periodic inspection, evident), wear-out β = 3.
        let (a, bases) = prefill_answers(&s, 8, 5, Some((3.0, PATTERN_WEAR_OUT)), Some(400.0));
        assert!(a.failure_evident && !a.safety_or_environmental && a.operational_impact);
        assert!(a.on_condition_feasible && a.scheduled_restoration_feasible);
        assert_eq!(a.weibull_beta, Some(3.0));
        assert_eq!(recommend_tactic(&a).recommended_tactic, "condition_based");
        assert_eq!(
            bases
                .iter()
                .find(|b| b.question == "on_condition_feasible")
                .map(|b| b.value),
            Some(Some(400.0))
        );
        // Without a P-F interval the wear-out pattern makes scheduled restoration the task.
        let (a, _) = prefill_answers(&s, 8, 5, Some((3.0, PATTERN_WEAR_OUT)), None);
        assert_eq!(recommend_tactic(&a).recommended_tactic, "time_based");
        // Random failures, no P-F: nothing proactive for an operational consequence.
        let (a, b) = prefill_answers(&s, 8, 5, Some((1.0, "random")), None);
        assert_eq!(recommend_tactic(&a).recommended_tactic, "run_to_failure");
        assert!(b.iter().any(|x| x.basis == "weibull_no_wear_out"));
    }

    #[test]
    fn hidden_safety_failure_calls_for_redesign_without_a_task() {
        let s = default_scales();
        let (a, _) = prefill_answers(&s, 10, 9, None, None);
        assert!(!a.failure_evident && a.multiple_failure_affects_safety);
        let rec = recommend_tactic(&a);
        assert_eq!(rec.consequence_category, "hidden");
        assert_eq!(rec.recommended_tactic, "redesign");
        assert!(rec.redesign_compulsory);
    }
}
