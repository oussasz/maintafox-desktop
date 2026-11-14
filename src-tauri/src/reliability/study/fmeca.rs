//! Study step 3 — FMECA worksheet of the study equipment (FMECA backend: `fmeca_analyses` /
//! `fmeca_items`, reused).
//!
//! Proposed failure modes are the governed ISO 14224 modes (`WORK.FAILURE_MODES`) recorded on
//! the study equipment in the period, then those recorded on any equipment of the same classes.
//! The occurrence rating is proposed from data, with its basis:
//! - `weibull`: the mode of a single-mode study with a fitted life: renewal rate
//!   `operating hours per equipment-year / mean life`;
//! - `history`: failures of the mode per equipment-year over the study period;
//!
//! and mapped to the plant occurrence scale (`scales`).

use std::collections::{BTreeMap, HashMap};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reliability::advanced_rams::domain::{CreateFmecaAnalysisInput, UpsertFmecaItemInput};
use crate::reliability::advanced_rams::queries as fmeca_queries;
use crate::reliability::study::analysis::{life_analysis, HOURS_PER_YEAR};
use crate::reliability::study::clock::parse_instant;
use crate::reliability::study::domain::{LifeAnalysisRequest, ReliabilityStudy};
use crate::reliability::study::scales::{load_scales, RatingScales};
use crate::reliability::study::store::{get, get_study, opt_f64, opt_i64, opt_str, STUDY_ACTIVE};

pub const SOURCE_WEIBULL: &str = "weibull";
pub const SOURCE_HISTORY: &str = "history";
pub const SOURCE_ANALYST: &str = "analyst";

pub const DOMAIN_MODES: &str = "WORK.FAILURE_MODES";
pub const DOMAIN_CAUSES: &str = "WORK.FAILURE_CAUSES";
pub const DOMAIN_MECHANISMS: &str = "WORK.FAILURE_MECHANISMS";

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReferenceOption {
    pub id: i64,
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OccurrenceProposal {
    /// `weibull` | `history`.
    pub source: String,
    /// Failures per equipment-year.
    pub events_per_year: f64,
    pub score: i64,
    /// Failures of the mode on the study equipment in the period.
    pub failures: i64,
    pub equipment_years: f64,
    /// Weibull basis: mean life and operating hours per equipment-year.
    pub mean_life_hours: Option<f64>,
    pub hours_per_equipment_year: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModeProposal {
    pub failure_mode_id: i64,
    pub code: String,
    pub label: String,
    pub failures_in_period: i64,
    pub failures_on_class: i64,
    pub occurrence: OccurrenceProposal,
    /// Most frequent mechanism and cause recorded with this mode.
    pub mechanism: Option<ReferenceOption>,
    pub cause: Option<ReferenceOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FmecaRow {
    pub item_id: i64,
    pub row_version: i64,
    pub component_label: Option<String>,
    pub failure_mode_id: Option<i64>,
    pub mode_code: Option<String>,
    pub mode_label: Option<String>,
    pub functional_failure: String,
    pub failure_effect: String,
    pub failure_mechanism_id: Option<i64>,
    pub mechanism_label: Option<String>,
    pub failure_cause_id: Option<i64>,
    pub cause_label: Option<String>,
    pub severity: i64,
    pub occurrence: i64,
    pub detectability: i64,
    pub rpn: i64,
    pub current_control: String,
    pub recommended_action: String,
    /// `weibull` | `history` | `analyst`.
    pub occurrence_source: Option<String>,
    pub occurrence_events_per_year: Option<f64>,
    /// Occurrence the data propose today for this mode (may differ from the saved rating).
    pub occurrence_proposal: Option<OccurrenceProposal>,
    pub severity_class: i64,
    pub occurrence_class: i64,
    /// `unacceptable` | `reduce` | `monitor` | `acceptable`.
    pub zone: String,
    pub rcm_decision_id: Option<i64>,
    pub rcm_tactic: Option<String>,
    pub decision_id: Option<i64>,
    pub decision_status: Option<String>,
    /// Ratings nobody chose (former root-cause promotion placeholder, migration 221): listed, but
    /// not counted in the matrix, the RPN ranking or the RCM candidates until a person saves it.
    pub review_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatrixCell {
    pub severity_class: i64,
    pub occurrence_class: i64,
    pub count: i64,
    pub zone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FmecaWorksheet {
    pub study_id: i64,
    pub analysis_id: Option<i64>,
    pub rows: Vec<FmecaRow>,
    pub proposals: Vec<ModeProposal>,
    pub scales: RatingScales,
    pub matrix: Vec<MatrixCell>,
    pub rpn_threshold: i64,
    pub equipment_years: f64,
    pub class_names: Vec<String>,
    pub modes: Vec<ReferenceOption>,
    pub mechanisms: Vec<ReferenceOption>,
    pub causes: Vec<ReferenceOption>,
    pub editable: bool,
    /// Rows to review, left out of the matrix and the ranking.
    pub rows_to_review: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFmecaItemInput {
    pub study_id: i64,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub expected_row_version: Option<i64>,
    pub failure_mode_id: i64,
    #[serde(default)]
    pub component_label: Option<String>,
    #[serde(default)]
    pub functional_failure: Option<String>,
    #[serde(default)]
    pub failure_effect: Option<String>,
    #[serde(default)]
    pub failure_mechanism_id: Option<i64>,
    #[serde(default)]
    pub failure_cause_id: Option<i64>,
    pub severity: i64,
    pub occurrence: i64,
    pub detectability: i64,
    #[serde(default)]
    pub current_control: Option<String>,
    #[serde(default)]
    pub recommended_action: Option<String>,
    /// `weibull` | `history` | `analyst`: where the occurrence rating comes from.
    pub occurrence_source: String,
    #[serde(default)]
    pub occurrence_events_per_year: Option<f64>,
    /// Root-cause diagram cause this new row comes from (traceability; ignored on update).
    #[serde(default)]
    pub source_ram_ishikawa_diagram_id: Option<i64>,
    #[serde(default)]
    pub source_ishikawa_flow_node_id: Option<String>,
}

pub async fn reference_options(db: &impl ConnectionTrait, domain: &str) -> AppResult<Vec<ReferenceOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rv.label FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains d ON d.id = rs.domain_id
             WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1
             ORDER BY rv.sort_order ASC, rv.code ASC",
            [domain.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(ReferenceOption {
                id: get(r, "id")?,
                code: get(r, "code")?,
                label: get(r, "label")?,
            })
        })
        .collect()
}

async fn value_in_domain(db: &impl ConnectionTrait, id: i64, domain: &str) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains d ON d.id = rs.domain_id WHERE rv.id = ? AND UPPER(TRIM(d.code)) = ?",
            [id.into(), domain.into()],
        ))
        .await?
        .is_some())
}

fn ids_sql(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// Equipment-years of the study (equipment × calendar years of the period, capped at now).
fn equipment_years(study: &ReliabilityStudy) -> AppResult<f64> {
    let start = parse_instant(&study.period_start)?;
    let end = parse_instant(&study.period_end)?.min(crate::clock::now());
    let years = ((end - start).num_milliseconds() as f64 / 3_600_000.0 / HOURS_PER_YEAR).max(0.0);
    Ok(years * study.assets.len() as f64)
}

struct ModeCount {
    code: String,
    label: String,
    in_period: i64,
    on_class: i64,
}

async fn mode_counts(db: &DatabaseConnection, study: &ReliabilityStudy) -> AppResult<BTreeMap<i64, ModeCount>> {
    let ids: Vec<i64> = study.assets.iter().map(|a| a.equipment_id).collect();
    let classes: Vec<i64> = study.assets.iter().filter_map(|a| a.class_id).collect();
    let mut out: BTreeMap<i64, ModeCount> = BTreeMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let in_period = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT fe.failure_mode_id AS id, rv.code, rv.label, COUNT(*) AS n FROM failure_events fe
                 JOIN reference_values rv ON rv.id = fe.failure_mode_id
                 WHERE fe.equipment_id IN ({}) AND COALESCE(fe.is_planned, 0) = 0
                   AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
                   AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
                 GROUP BY fe.failure_mode_id",
                ids_sql(&ids)
            ),
            [study.period_start.clone().into(), study.period_end.clone().into()],
        ))
        .await?;
    for r in &in_period {
        out.insert(
            get(r, "id")?,
            ModeCount {
                code: get(r, "code")?,
                label: get(r, "label")?,
                in_period: get(r, "n")?,
                on_class: 0,
            },
        );
    }
    let class_filter = if classes.is_empty() {
        format!("e.id IN ({})", ids_sql(&ids))
    } else {
        format!("(e.class_id IN ({}) OR e.id IN ({}))", ids_sql(&classes), ids_sql(&ids))
    };
    let on_class = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT fe.failure_mode_id AS id, rv.code, rv.label, COUNT(*) AS n FROM failure_events fe
                 JOIN equipment e ON e.id = fe.equipment_id
                 JOIN reference_values rv ON rv.id = fe.failure_mode_id
                 WHERE {class_filter} AND COALESCE(fe.is_planned, 0) = 0
                 GROUP BY fe.failure_mode_id"
            ),
        ))
        .await?;
    for r in &on_class {
        let id: i64 = get(r, "id")?;
        let n: i64 = get(r, "n")?;
        out.entry(id).and_modify(|m| m.on_class = n).or_insert(ModeCount {
            code: get(r, "code")?,
            label: get(r, "label")?,
            in_period: 0,
            on_class: n,
        });
    }
    Ok(out)
}

/// Most frequent mechanism / cause recorded with a mode on the study equipment.
async fn usual_coding(
    db: &DatabaseConnection,
    study: &ReliabilityStudy,
    mode: i64,
) -> AppResult<(Option<ReferenceOption>, Option<ReferenceOption>)> {
    let ids: Vec<i64> = study.assets.iter().map(|a| a.equipment_id).collect();
    if ids.is_empty() {
        return Ok((None, None));
    }
    let pick = |col: &str| {
        format!(
            "SELECT rv.id, rv.code, rv.label, COUNT(*) AS n FROM failure_events fe
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             LEFT JOIN work_order_failure_details wfd ON wfd.id = (
                  SELECT x.id FROM work_order_failure_details x WHERE x.work_order_id = wo.id
                  ORDER BY x.entity_sync_id, x.id LIMIT 1)
             JOIN reference_values rv ON rv.id = COALESCE(fe.{col}, wfd.{col})
             WHERE fe.equipment_id IN ({}) AND fe.failure_mode_id = ?
             GROUP BY rv.id ORDER BY n DESC, rv.code ASC LIMIT 1",
            ids_sql(&ids)
        )
    };
    let mut out = Vec::new();
    for col in ["failure_mechanism_id", "failure_cause_id"] {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                pick(col),
                [mode.into()],
            ))
            .await?;
        out.push(
            row.map(|r| {
                Ok::<_, AppError>(ReferenceOption {
                    id: get(&r, "id")?,
                    code: get(&r, "code")?,
                    label: get(&r, "label")?,
                })
            })
            .transpose()?,
        );
    }
    let cause = out.pop().flatten();
    let mechanism = out.pop().flatten();
    Ok((mechanism, cause))
}

/// Whether `mode` is the study mode or one of its child codes.
async fn is_study_mode(db: &DatabaseConnection, study: &ReliabilityStudy, mode: i64) -> AppResult<bool> {
    let Some(m) = study.failure_mode_id else {
        return Ok(false);
    };
    if m == mode {
        return Ok(true);
    }
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM reference_values WHERE id = ? AND parent_id = ?",
            [mode.into(), m.into()],
        ))
        .await?
        .is_some())
}

/// Occurrence proposal for one mode (pure).
#[must_use]
pub fn propose_occurrence(
    scales: &RatingScales,
    failures: i64,
    equipment_years: f64,
    weibull: Option<(f64, f64)>,
) -> OccurrenceProposal {
    if let Some((mean_life, hours_per_year)) = weibull.filter(|(m, h)| *m > 0.0 && *h > 0.0) {
        let per_year = hours_per_year / mean_life;
        return OccurrenceProposal {
            source: SOURCE_WEIBULL.into(),
            events_per_year: per_year,
            score: scales.occurrence_score(per_year),
            failures,
            equipment_years,
            mean_life_hours: Some(mean_life),
            hours_per_equipment_year: Some(hours_per_year),
        };
    }
    let per_year = if equipment_years > 0.0 {
        failures as f64 / equipment_years
    } else {
        0.0
    };
    OccurrenceProposal {
        source: SOURCE_HISTORY.into(),
        events_per_year: per_year,
        score: scales.occurrence_score(per_year),
        failures,
        equipment_years,
        mean_life_hours: None,
        hours_per_equipment_year: None,
    }
}

pub async fn get_worksheet(db: &DatabaseConnection, study_id: i64) -> AppResult<FmecaWorksheet> {
    let study = get_study(db, study_id).await?;
    let scales = load_scales(db).await?;
    let eq_years = equipment_years(&study)?;
    let counts = mode_counts(db, &study).await?;

    // Weibull basis for the study mode (a single-mode study with a fit).
    let mut weibull: Option<(f64, f64)> = None;
    if study.failure_mode_id.is_some() {
        let (a, _) = life_analysis(
            db,
            LifeAnalysisRequest {
                study_id,
                ..LifeAnalysisRequest::default()
            },
        )
        .await?;
        if let (Some(fit), Some(hpy)) = (a.fit, a.operating_hours_per_year) {
            if a.asset_count > 0 {
                weibull = Some((fit.mean_life, hpy / a.asset_count as f64));
            }
        }
    }

    let mut proposals_by_mode: HashMap<i64, OccurrenceProposal> = HashMap::new();
    let mut proposals = Vec::new();
    for (id, c) in &counts {
        let wb = if is_study_mode(db, &study, *id).await? {
            weibull
        } else {
            None
        };
        let occ = propose_occurrence(&scales, c.in_period, eq_years, wb);
        proposals_by_mode.insert(*id, occ.clone());
        let (mechanism, cause) = usual_coding(db, &study, *id).await?;
        proposals.push(ModeProposal {
            failure_mode_id: *id,
            code: c.code.clone(),
            label: c.label.clone(),
            failures_in_period: c.in_period,
            failures_on_class: c.on_class,
            occurrence: occ,
            mechanism,
            cause,
        });
    }
    proposals.sort_by(|a, b| {
        b.failures_in_period
            .cmp(&a.failures_in_period)
            .then(b.failures_on_class.cmp(&a.failures_on_class))
            .then(a.code.cmp(&b.code))
    });

    let mut rows = Vec::new();
    if let Some(analysis_id) = study_fmeca_analysis_id(db, study.id).await? {
        let items = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT i.id, i.row_version, i.component_label, i.failure_mode_id, m.code AS mode_code,
                        m.label AS mode_label, i.functional_failure, i.failure_effect, i.failure_mechanism_id,
                        mech.label AS mechanism_label, i.failure_cause_id, c.label AS cause_label,
                        i.severity, i.occurrence, i.detectability, i.rpn, i.current_control,
                        i.recommended_action, i.occurrence_source, i.occurrence_events_per_year,
                        (SELECT d.id FROM rcm_decisions d WHERE d.fmeca_item_id = i.id ORDER BY d.id DESC LIMIT 1) AS rcm_id,
                        (SELECT d.selected_tactic FROM rcm_decisions d WHERE d.fmeca_item_id = i.id
                          AND d.decision_path_json IS NOT NULL ORDER BY d.id DESC LIMIT 1) AS rcm_tactic,
                        (SELECT x.id FROM reliability_study_decisions x WHERE x.fmeca_item_id = i.id
                          ORDER BY x.created_at DESC, x.id DESC LIMIT 1) AS decision_id,
                        (SELECT x.status FROM reliability_study_decisions x WHERE x.fmeca_item_id = i.id
                          ORDER BY x.created_at DESC, x.id DESC LIMIT 1) AS decision_status,
                        i.review_required
                 FROM fmeca_items i
                 LEFT JOIN reference_values m ON m.id = i.failure_mode_id
                 LEFT JOIN reference_values mech ON mech.id = i.failure_mechanism_id
                 LEFT JOIN reference_values c ON c.id = i.failure_cause_id
                 WHERE i.analysis_id = ?
                 ORDER BY i.review_required ASC, i.rpn DESC, i.id ASC",
                [analysis_id.into()],
            ))
            .await?;
        for r in &items {
            let severity: i64 = get(r, "severity")?;
            let occurrence: i64 = get(r, "occurrence")?;
            let mode: Option<i64> = get(r, "failure_mode_id")?;
            rows.push(FmecaRow {
                item_id: get(r, "id")?,
                row_version: get(r, "row_version")?,
                component_label: get(r, "component_label")?,
                failure_mode_id: mode,
                mode_code: get(r, "mode_code")?,
                mode_label: get(r, "mode_label")?,
                functional_failure: get(r, "functional_failure")?,
                failure_effect: get(r, "failure_effect")?,
                failure_mechanism_id: get(r, "failure_mechanism_id")?,
                mechanism_label: get(r, "mechanism_label")?,
                failure_cause_id: get(r, "failure_cause_id")?,
                cause_label: get(r, "cause_label")?,
                severity,
                occurrence,
                detectability: get(r, "detectability")?,
                rpn: get(r, "rpn")?,
                current_control: get(r, "current_control")?,
                recommended_action: get(r, "recommended_action")?,
                occurrence_source: get(r, "occurrence_source")?,
                occurrence_events_per_year: get(r, "occurrence_events_per_year")?,
                occurrence_proposal: mode.and_then(|m| proposals_by_mode.get(&m).cloned()),
                severity_class: scales.class_of(severity),
                occurrence_class: scales.class_of(occurrence),
                zone: scales.zone(severity, occurrence),
                rcm_decision_id: get(r, "rcm_id")?,
                rcm_tactic: get(r, "rcm_tactic")?,
                decision_id: get(r, "decision_id")?,
                decision_status: get(r, "decision_status")?,
                review_required: get::<i64>(r, "review_required")? != 0,
            });
        }
    }
    let in_sheet: Vec<i64> = rows.iter().filter_map(|r| r.failure_mode_id).collect();
    proposals.retain(|p| !in_sheet.contains(&p.failure_mode_id));

    let mut matrix = Vec::new();
    for sc in (1..=scales.matrix.classes).rev() {
        for oc in 1..=scales.matrix.classes {
            let count = rows
                .iter()
                .filter(|r| !r.review_required && r.severity_class == sc && r.occurrence_class == oc)
                .count() as i64;
            // Zone of the class pair: highest scores of the classes.
            let s_score = (sc * 10 / scales.matrix.classes).clamp(1, 10);
            let o_score = (oc * 10 / scales.matrix.classes).clamp(1, 10);
            matrix.push(MatrixCell {
                severity_class: sc,
                occurrence_class: oc,
                count,
                zone: scales.zone(s_score, o_score),
            });
        }
    }
    let mut class_names: Vec<String> = study.assets.iter().filter_map(|a| a.class_name.clone()).collect();
    class_names.sort();
    class_names.dedup();
    let rows_to_review = rows.iter().filter(|r| r.review_required).count() as i64;

    Ok(FmecaWorksheet {
        study_id,
        analysis_id: study_fmeca_analysis_id(db, study.id).await?,
        rows,
        proposals,
        matrix,
        rpn_threshold: fmeca_queries::fmeca_rpn_critical_threshold_i64(db).await?,
        equipment_years: eq_years,
        class_names,
        modes: reference_options(db, DOMAIN_MODES).await?,
        mechanisms: reference_options(db, DOMAIN_MECHANISMS).await?,
        causes: reference_options(db, DOMAIN_CAUSES).await?,
        editable: study.status == STUDY_ACTIVE,
        rows_to_review,
        scales,
    })
}

/// The placeholder row written by the former root-cause promotion (no alias): S = O = D = 5 with
/// the English action "Address RCA cause: …", from a diagram. Same signature as migration 221.
pub const PLACEHOLDER_SIGNATURE_SQL: &str = "source_ram_ishikawa_diagram_id IS NOT NULL \
     AND severity = 5 AND occurrence = 5 AND detectability = 5 \
     AND recommended_action LIKE 'Address RCA cause: %'";

/// Startup pass (idempotent): flags placeholder rows that arrived by sync from a device that had
/// not run migration 221 yet. Only the exact signature; ratings are not changed. Returns the count.
pub async fn flag_placeholder_rows(db: &impl ConnectionTrait) -> AppResult<u64> {
    let res = db
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "UPDATE fmeca_items SET review_required = 1 WHERE review_required = 0 AND {PLACEHOLDER_SIGNATURE_SQL}"
            ),
        ))
        .await?;
    Ok(res.rows_affected())
}

/// FMECA rows of the study still to review (ratings nobody chose, migration 221).
pub async fn rows_to_review(db: &impl ConnectionTrait, study_id: i64) -> AppResult<i64> {
    let Some(analysis_id) = study_fmeca_analysis_id(db, study_id).await? else {
        return Ok(0);
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM fmeca_items WHERE analysis_id = ? AND review_required = 1",
            [analysis_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("fmeca rows to review count returned no row")))?;
    get(&row, "n")
}

/// FMECA analysis linked to a study (none until the first row is saved).
pub async fn study_fmeca_analysis_id(db: &impl ConnectionTrait, study_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT fmeca_analysis_id FROM reliability_studies WHERE id = ?",
            [study_id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| get::<Option<i64>>(&r, "fmeca_analysis_id"))
        .transpose()?
        .flatten())
}

async fn ensure_analysis(db: &DatabaseConnection, study: &ReliabilityStudy, actor_id: i32) -> AppResult<i64> {
    if let Some(id) = study_fmeca_analysis_id(db, study.id).await? {
        return Ok(id);
    }
    let first = study.assets.first().ok_or_else(|| invalid("STUDY_NO_ASSETS"))?;
    let analysis = fmeca_queries::create_fmeca_analysis(
        db,
        Some(actor_id),
        CreateFmecaAnalysisInput {
            equipment_id: first.equipment_id,
            title: format!("AMDEC · {}", study.title),
            boundary_definition: Some(
                study
                    .assets
                    .iter()
                    .map(|a| a.asset_code.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            status: None,
        },
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reliability_studies SET fmeca_analysis_id = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            analysis.id.into(),
            crate::clock::now().to_rfc3339().into(),
            study.id.into(),
        ],
    ))
    .await?;
    Ok(analysis.id)
}

async fn item_in_analysis(db: &DatabaseConnection, item_id: i64, analysis_id: i64) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM fmeca_items WHERE id = ? AND analysis_id = ?",
            [item_id.into(), analysis_id.into()],
        ))
        .await?
        .is_some())
}

pub async fn save_item(db: &DatabaseConnection, input: SaveFmecaItemInput, actor_id: i32) -> AppResult<FmecaWorksheet> {
    let study = get_study(db, input.study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    if ![SOURCE_WEIBULL, SOURCE_HISTORY, SOURCE_ANALYST].contains(&input.occurrence_source.as_str()) {
        return Err(invalid("FMECA_OCCURRENCE_SOURCE_INVALID"));
    }
    if !value_in_domain(db, input.failure_mode_id, DOMAIN_MODES).await? {
        return Err(invalid("FMECA_FAILURE_MODE_UNKNOWN"));
    }
    if let Some(m) = input.failure_mechanism_id {
        if !value_in_domain(db, m, DOMAIN_MECHANISMS).await? {
            return Err(invalid("FMECA_MECHANISM_UNKNOWN"));
        }
    }
    if let Some(c) = input.failure_cause_id {
        if !value_in_domain(db, c, DOMAIN_CAUSES).await? {
            return Err(invalid("FMECA_CAUSE_UNKNOWN"));
        }
    }
    let analysis_id = ensure_analysis(db, &study, actor_id).await?;
    if let Some(id) = input.item_id {
        if !item_in_analysis(db, id, analysis_id).await? {
            return Err(invalid("FMECA_ITEM_NOT_IN_STUDY"));
        }
    }
    let mode_label: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT label FROM reference_values WHERE id = ?",
            [input.failure_mode_id.into()],
        ))
        .await?
        .map(|r| get(&r, "label"))
        .transpose()?;
    let functional_failure = input
        .functional_failure
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(mode_label);
    let item = fmeca_queries::upsert_fmeca_item(
        db,
        UpsertFmecaItemInput {
            id: input.item_id,
            analysis_id,
            expected_row_version: input.expected_row_version,
            component_id: None,
            functional_failure,
            failure_mode_id: Some(input.failure_mode_id),
            failure_effect: input.failure_effect.clone(),
            severity: input.severity,
            occurrence: input.occurrence,
            detectability: input.detectability,
            recommended_action: input.recommended_action.clone(),
            current_control: input.current_control.clone(),
            linked_pm_plan_id: None,
            linked_work_order_id: None,
            revised_rpn: None,
            // Used on creation only (the update keeps the row's link).
            source_ram_ishikawa_diagram_id: input.source_ram_ishikawa_diagram_id,
            source_ishikawa_flow_node_id: input
                .source_ishikawa_flow_node_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
    )
    .await?;
    // A person rated and saved the row: it is no longer "to review".
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE fmeca_items SET component_label = ?, failure_mechanism_id = ?, failure_cause_id = ?,
            occurrence_source = ?, occurrence_events_per_year = ?, review_required = 0 WHERE id = ?",
        [
            opt_str(
                input
                    .component_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            ),
            opt_i64(input.failure_mechanism_id),
            opt_i64(input.failure_cause_id),
            input.occurrence_source.clone().into(),
            opt_f64(input.occurrence_events_per_year.filter(|v| v.is_finite() && *v >= 0.0)),
            item.id.into(),
        ],
    ))
    .await?;
    get_worksheet(db, input.study_id).await
}

pub async fn delete_item(db: &DatabaseConnection, study_id: i64, item_id: i64) -> AppResult<FmecaWorksheet> {
    let study = get_study(db, study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    let analysis = study_fmeca_analysis_id(db, study_id)
        .await?
        .ok_or_else(|| invalid("FMECA_ITEM_NOT_IN_STUDY"))?;
    if !item_in_analysis(db, item_id, analysis).await? {
        return Err(invalid("FMECA_ITEM_NOT_IN_STUDY"));
    }
    let linked = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM reliability_study_decisions WHERE fmeca_item_id = ?
             UNION ALL SELECT 1 FROM rcm_decisions WHERE fmeca_item_id = ? LIMIT 1",
            [item_id.into(), item_id.into()],
        ))
        .await?
        .is_some();
    if linked {
        // The row is the evidence of a recorded decision: it stays.
        return Err(invalid("FMECA_ITEM_HAS_DECISIONS"));
    }
    fmeca_queries::delete_fmeca_item(db, item_id).await?;
    get_worksheet(db, study_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reliability::study::scales::default_scales;

    #[test]
    fn occurrence_from_weibull_and_history() {
        let s = default_scales();
        // Mockup pump bearing: mean life 3 967 h, 8 000 operating h per pump-year → 2.0 / year → 7.
        let w = propose_occurrence(&s, 8, 4.0, Some((3967.0, 8000.0)));
        assert_eq!(w.source, SOURCE_WEIBULL);
        assert!((w.events_per_year - 8000.0 / 3967.0).abs() < 1e-12);
        assert_eq!(w.score, 7);
        // History: 3 failures over 4 equipment-years → 0.75 / year → about once a year (6).
        let h = propose_occurrence(&s, 3, 4.0, None);
        assert_eq!(h.source, SOURCE_HISTORY);
        assert!((h.events_per_year - 0.75).abs() < 1e-12);
        assert_eq!(h.score, 6);
        assert_eq!(propose_occurrence(&s, 0, 4.0, None).score, 1);
        assert_eq!(propose_occurrence(&s, 2, 0.0, None).events_per_year, 0.0);
    }
}
