//! Asset criticality with a method (ISO 14224 Annex / SAP PM ABC indicator / Maximo priority).
//!
//! Method `RISK_5X5_V1` — consequence × likelihood risk matrix:
//! - five consequence categories, each scored 1 (negligible) to 5 (catastrophic): safety,
//!   environment, production, quality, maintenance cost;
//! - consequence = the highest of the five; likelihood of failure scored 1 (rare) to 5 (frequent);
//! - risk = consequence × likelihood (1–25);
//! - class A if risk ≥ 15, or if safety or environment is scored 5 (a catastrophic HSE consequence
//!   makes the asset critical whatever its failure likelihood); B if risk ≥ 8; C if risk ≥ 4;
//!   D otherwise.
//!
//! The thresholds live in [`RISK_5X5_V1`] only; the UI reads them through
//! `get_asset_criticality_method` and asks `preview_asset_criticality` for the result, so the
//! rule is written once. Every assessment is kept (`equipment_criticality_assessments`) with the
//! method code, and the resulting class is written to the asset in the same transaction.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::assets::identity::{
    self, normalize_criticality_to_canon, resolve_criticality_code, resolve_criticality_ref_id,
};
use crate::errors::{issue_params, org_validation_failed, AppError, AppResult};

/// Thresholds of a risk-matrix criticality method.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct CriticalityMethod {
    pub code: &'static str,
    pub score_min: i64,
    pub score_max: i64,
    /// Minimum risk score for each class.
    pub class_a_min_risk: i64,
    pub class_b_min_risk: i64,
    pub class_c_min_risk: i64,
    /// A safety or environment score at or above this value makes the asset class A.
    pub hse_override_score: i64,
}

pub const RISK_5X5_V1: CriticalityMethod = CriticalityMethod {
    code: "RISK_5X5_V1",
    score_min: 1,
    score_max: 5,
    class_a_min_risk: 15,
    class_b_min_risk: 8,
    class_c_min_risk: 4,
    hse_override_score: 5,
};

/// Longest rationale kept with an assessment.
pub const RATIONALE_MAX_CHARS: usize = 2000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct CriticalityScores {
    pub safety: i64,
    pub environment: i64,
    pub production: i64,
    pub quality: i64,
    pub cost: i64,
    pub likelihood: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CriticalityResult {
    pub method_code: String,
    pub consequence: i64,
    pub risk_score: i64,
    pub resulting_class: String,
    /// True when the class comes from the safety / environment override, not the risk score.
    pub hse_override: bool,
}

#[derive(Debug, Deserialize)]
pub struct AssessCriticalityPayload {
    pub asset_id: i64,
    pub scores: CriticalityScores,
    pub rationale: Option<String>,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalityAssessment {
    pub id: i64,
    pub asset_id: i64,
    pub method_code: String,
    pub safety_score: i64,
    pub environment_score: i64,
    pub production_score: i64,
    pub quality_score: i64,
    pub cost_score: i64,
    pub likelihood_score: i64,
    pub risk_score: i64,
    pub resulting_class: String,
    pub previous_class: Option<String>,
    pub rationale: Option<String>,
    pub assessed_by_name: Option<String>,
    pub assessed_at: String,
}

fn validate_scores(method: &CriticalityMethod, scores: &CriticalityScores) -> AppResult<()> {
    let named = [
        ("safety", scores.safety),
        ("environment", scores.environment),
        ("production", scores.production),
        ("quality", scores.quality),
        ("cost", scores.cost),
        ("likelihood", scores.likelihood),
    ];
    for (name, value) in named {
        if value < method.score_min || value > method.score_max {
            return Err(org_validation_failed(
                "ASSET_CRITICALITY_SCORE_OUT_OF_RANGE",
                format!(
                    "Score '{name}' must be between {} and {}.",
                    method.score_min, method.score_max
                ),
                issue_params(&[
                    ("field", name.to_string()),
                    ("min", method.score_min.to_string()),
                    ("max", method.score_max.to_string()),
                ]),
            ));
        }
    }
    Ok(())
}

/// Applies the method to a set of scores.
pub fn evaluate(method: &CriticalityMethod, scores: &CriticalityScores) -> AppResult<CriticalityResult> {
    validate_scores(method, scores)?;
    let consequence = [
        scores.safety,
        scores.environment,
        scores.production,
        scores.quality,
        scores.cost,
    ]
    .into_iter()
    .max()
    .unwrap_or(method.score_min);
    let risk_score = consequence * scores.likelihood;
    let hse_override = scores.safety >= method.hse_override_score || scores.environment >= method.hse_override_score;
    let class = if risk_score >= method.class_a_min_risk || hse_override {
        "A"
    } else if risk_score >= method.class_b_min_risk {
        "B"
    } else if risk_score >= method.class_c_min_risk {
        "C"
    } else {
        "D"
    };
    Ok(CriticalityResult {
        method_code: method.code.to_string(),
        consequence,
        risk_score,
        resulting_class: class.to_string(),
        hse_override: hse_override && risk_score < method.class_a_min_risk,
    })
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "criticality assessment decode failed for column '{column}': {e}"
    ))
}

fn get<T: sea_orm::TryGetable>(row: &QueryResult, column: &str) -> AppResult<T> {
    row.try_get::<T>("", column).map_err(|e| decode_err(column, e))
}

fn map_assessment(row: &QueryResult) -> AppResult<CriticalityAssessment> {
    Ok(CriticalityAssessment {
        id: get(row, "id")?,
        asset_id: get(row, "equipment_id")?,
        method_code: get(row, "method_code")?,
        safety_score: get(row, "safety_score")?,
        environment_score: get(row, "environment_score")?,
        production_score: get(row, "production_score")?,
        quality_score: get(row, "quality_score")?,
        cost_score: get(row, "cost_score")?,
        likelihood_score: get(row, "likelihood_score")?,
        risk_score: get(row, "risk_score")?,
        resulting_class: get(row, "resulting_class")?,
        previous_class: get(row, "previous_class")?,
        rationale: get(row, "rationale")?,
        assessed_by_name: get(row, "assessed_by_name")?,
        assessed_at: get(row, "assessed_at")?,
    })
}

const ASSESSMENT_SELECT: &str = "SELECT a.id, a.equipment_id, a.method_code, a.safety_score, \
    a.environment_score, a.production_score, a.quality_score, a.cost_score, a.likelihood_score, \
    a.risk_score, a.resulting_class, a.previous_class, a.rationale, a.assessed_at, \
    COALESCE(u.display_name, u.username) AS assessed_by_name \
    FROM equipment_criticality_assessments a \
    LEFT JOIN user_accounts u ON u.id = a.assessed_by_id";

/// Assessments of one asset, newest first.
pub async fn list_assessments(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<Vec<CriticalityAssessment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ASSESSMENT_SELECT} WHERE a.equipment_id = ? ORDER BY a.assessed_at DESC, a.id DESC"),
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_assessment).collect()
}

/// Records an assessment and writes the resulting class to the asset (one transaction).
pub async fn assess_asset_criticality(
    db: &DatabaseConnection,
    payload: AssessCriticalityPayload,
    actor_id: i64,
) -> AppResult<(CriticalityAssessment, identity::Asset)> {
    let result = evaluate(&RISK_5X5_V1, &payload.scores)?;
    let rationale = payload
        .rationale
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    if let Some(ref r) = rationale {
        if r.chars().count() > RATIONALE_MAX_CHARS {
            return Err(org_validation_failed(
                "ASSET_CRITICALITY_RATIONALE_TOO_LONG",
                format!("The rationale is limited to {RATIONALE_MAX_CHARS} characters."),
                issue_params(&[("max", RATIONALE_MAX_CHARS.to_string())]),
            ));
        }
    }

    let txn = db.begin().await?;
    let current = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.row_version, COALESCE(rs_crit.code, lv.code) AS criticality_code \
             FROM equipment e \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id \
             WHERE e.id = ? AND e.deleted_at IS NULL",
            [payload.asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: payload.asset_id.to_string(),
        })?;
    let row_version: i64 = get(&current, "row_version")?;
    if row_version != payload.expected_row_version {
        return Err(org_validation_failed(
            "ASSET_STALE_VERSION",
            "This asset was changed by someone else. Reload it and try again.",
            issue_params(&[]),
        ));
    }
    let previous_class = get::<Option<String>>(&current, "criticality_code")?
        .and_then(|code| normalize_criticality_to_canon(&code).ok());

    let lookup_id = resolve_criticality_code(&txn, &result.resulting_class).await?;
    let ref_id = resolve_criticality_ref_id(&txn, &result.resulting_class).await?;
    let now = crate::clock::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment SET criticality_value_id = ?, equipment_criticality_ref_id = ?, \
         updated_at = ?, row_version = row_version + 1 WHERE id = ? AND row_version = ?",
        [
            lookup_id.into(),
            ref_id.into(),
            now.clone().into(),
            payload.asset_id.into(),
            row_version.into(),
        ],
    ))
    .await?;

    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_criticality_assessments \
         (entity_sync_id, equipment_id, method_code, safety_score, environment_score, \
          production_score, quality_score, cost_score, likelihood_score, risk_score, \
          resulting_class, previous_class, rationale, assessed_by_id, assessed_at, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            entity_sync_id.clone().into(),
            payload.asset_id.into(),
            result.method_code.clone().into(),
            payload.scores.safety.into(),
            payload.scores.environment.into(),
            payload.scores.production.into(),
            payload.scores.quality.into(),
            payload.scores.cost.into(),
            payload.scores.likelihood.into(),
            result.risk_score.into(),
            result.resulting_class.clone().into(),
            previous_class.into(),
            rationale.into(),
            actor_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ASSESSMENT_SELECT} WHERE a.entity_sync_id = ?"),
            [entity_sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("criticality assessment missing after insert")))?;
    let assessment = map_assessment(&row)?;
    let asset_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {} {} WHERE e.id = ?",
                identity::ASSET_SELECT,
                identity::ASSET_FROM
            ),
            [payload.asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: payload.asset_id.to_string(),
        })?;
    let asset = identity::map_asset(&asset_row)?;
    txn.commit().await?;
    tracing::info!(asset_id = payload.asset_id, class = %assessment.resulting_class, "asset criticality assessed");
    Ok((assessment, asset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(s: i64, e: i64, p: i64, q: i64, c: i64, l: i64) -> CriticalityScores {
        CriticalityScores {
            safety: s,
            environment: e,
            production: p,
            quality: q,
            cost: c,
            likelihood: l,
        }
    }

    fn class_of(s: CriticalityScores) -> String {
        evaluate(&RISK_5X5_V1, &s).unwrap().resulting_class
    }

    #[test]
    fn risk_is_highest_consequence_times_likelihood() {
        let r = evaluate(&RISK_5X5_V1, &scores(1, 2, 4, 1, 3, 3)).unwrap();
        assert_eq!(r.consequence, 4);
        assert_eq!(r.risk_score, 12);
        assert_eq!(r.resulting_class, "B");
        assert!(!r.hse_override);
    }

    #[test]
    fn class_boundaries_follow_the_thresholds() {
        assert_eq!(class_of(scores(1, 1, 5, 1, 1, 3)), "A"); // risk 15
        assert_eq!(class_of(scores(1, 1, 2, 1, 1, 5)), "B"); // risk 10
        assert_eq!(class_of(scores(1, 1, 4, 1, 1, 2)), "B"); // risk 8
        assert_eq!(class_of(scores(1, 1, 2, 1, 1, 2)), "C"); // risk 4
        assert_eq!(class_of(scores(1, 1, 3, 1, 1, 1)), "D"); // risk 3
    }

    #[test]
    fn catastrophic_safety_or_environment_is_always_class_a() {
        let r = evaluate(&RISK_5X5_V1, &scores(5, 1, 1, 1, 1, 1)).unwrap();
        assert_eq!(r.risk_score, 5);
        assert_eq!(r.resulting_class, "A");
        assert!(r.hse_override);
        assert_eq!(class_of(scores(1, 5, 1, 1, 1, 1)), "A");
    }

    #[test]
    fn scores_outside_the_scale_are_refused() {
        assert!(evaluate(&RISK_5X5_V1, &scores(0, 1, 1, 1, 1, 1)).is_err());
        assert!(evaluate(&RISK_5X5_V1, &scores(1, 1, 1, 1, 1, 6)).is_err());
    }
}
