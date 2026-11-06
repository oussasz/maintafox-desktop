//! Data quality of the reliability module (replaces the former "foundation" screen): what keeps
//! failure records out of the analyses, with the record to open for each point.
//!
//! - failure records of the period / class / site not completely coded (mode, cause or "not
//!   determined", mechanism — ISO 14224), with the predicate of the RAMS home card
//!   (`fleet_kpis::complete_sql`); those raised from a work order are listed;
//! - equipment with failures and no operating-hours data (meter, runtime log, schedule);
//! - the remaining issues of `v_ram_data_quality_issues` (small samples, inspection coverage),
//!   which a reliability manager can dismiss.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;
use crate::reliability::domain::RamDataQualityIssuesFilter;
use crate::reliability::queries;
use crate::reliability::study::clock::load_clock;
use crate::reliability::study::domain::FilterOption;
use crate::reliability::study::fleet_kpis::{
    complete_sql, filter_options, load_asset_aggs, scope_from_sql, AssetAgg, CodingCounts, FleetScope, CAUSE_CODED_SQL,
    MODE_SQL,
};
use crate::reliability::study::store::{get, opt_i64};

#[derive(Debug, Clone, Serialize)]
pub struct WoCodingGap {
    pub work_order_id: i64,
    pub work_order_code: String,
    pub equipment_id: Option<i64>,
    pub asset_code: Option<String>,
    pub closed_at: Option<String>,
    /// `mode_missing` | `cause_missing` | `mechanism_missing`.
    pub gap: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentGap {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub failures: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewIssue {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    /// `LOW_SAMPLE` | `MISSING_INSPECTION_COVERAGE`.
    pub issue_code: String,
    /// `blocking` | `warning`.
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataQualityView {
    pub period_start: String,
    pub period_end: String,
    pub coding_gaps: Vec<WoCodingGap>,
    pub no_operating_hours: Vec<EquipmentGap>,
    pub review_issues: Vec<ReviewIssue>,
    /// Failure records of the scope and those fully coded (mode, cause or not determined,
    /// mechanism) — the same predicate and numbers as the RAMS home card.
    pub records: i64,
    pub records_complete: i64,
    pub records_missing_mode: i64,
    pub records_missing_cause: i64,
    pub records_missing_mechanism: i64,
    /// Filter choices (org nodes holding equipment, equipment classes in use).
    pub org_nodes: Vec<FilterOption>,
    pub equipment_classes: Vec<FilterOption>,
    /// A list stopped at [`MAX_ROWS`]: more exist than are listed (the screen says so; its counts
    /// are then "at least").
    pub coding_gaps_truncated: bool,
    pub no_operating_hours_truncated: bool,
    pub review_issues_truncated: bool,
}

pub const MAX_ROWS: i64 = 300;

/// Period and filters of the quality page — the same as the RAMS home page's.
#[derive(Debug, Clone, Deserialize)]
pub struct DataQualityFilter {
    /// RFC 3339 start of the period (inclusive).
    pub period_start: String,
    /// RFC 3339 end of the period (inclusive, clipped to now).
    pub period_end: String,
    /// Org node: equipment installed at this node or below it.
    #[serde(default)]
    pub org_node_id: Option<i64>,
    #[serde(default)]
    pub equipment_class_id: Option<i64>,
}

/// The worklist of a scope. Its counts and its coding-gap list use the completeness predicate of
/// `fleet_kpis` (mode + cause or "not determined" + mechanism), so the RAMS home card and this page
/// agree under the same filters.
pub async fn data_quality(
    db: &DatabaseConnection,
    filter: &DataQualityFilter,
    user_id: i32,
) -> AppResult<DataQualityView> {
    let scope = FleetScope::new(
        &filter.period_start,
        &filter.period_end,
        filter.equipment_class_id,
        filter.org_node_id,
    )?;
    let (s, e) = (scope.start.to_rfc3339(), scope.end.to_rfc3339());

    let aggs = load_asset_aggs(db, &scope).await?;
    let coding = CodingCounts::of(&aggs);

    // Incomplete records that came from a work order (the record to open), latest closed first;
    // the gap named is the first missing of mode, cause, mechanism.
    let mut binds = scope.binds();
    binds.push(MAX_ROWS.into());
    let gaps = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, wo.code, wo.closed_at, fe.equipment_id, e.asset_id_code,
                        CASE WHEN {MODE_SQL} IS NULL THEN 'mode_missing'
                             WHEN NOT {CAUSE_CODED_SQL} THEN 'cause_missing'
                             ELSE 'mechanism_missing' END AS gap
                 {from} AND wo.id IS NOT NULL AND NOT {complete}
                 ORDER BY datetime(wo.closed_at) DESC, wo.id DESC LIMIT ?",
                from = scope_from_sql(),
                complete = complete_sql()
            ),
            binds,
        ))
        .await?;
    let coding_gaps_truncated = gaps.len() as i64 >= MAX_ROWS;
    let coding_gaps = gaps
        .iter()
        .map(|r| {
            Ok(WoCodingGap {
                work_order_id: get(r, "id")?,
                work_order_code: get(r, "code")?,
                equipment_id: get(r, "equipment_id")?,
                asset_code: get(r, "asset_id_code")?,
                closed_at: get(r, "closed_at")?,
                gap: get(r, "gap")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    // Equipment of the scope with failures, most failures first; those beyond the first MAX_ROWS
    // are not examined.
    let mut with_failures: Vec<&AssetAgg> = aggs.iter().collect();
    with_failures.sort_by(|a, b| {
        b.failures
            .cmp(&a.failures)
            .then_with(|| a.asset_code.cmp(&b.asset_code))
    });
    let no_operating_hours_truncated = with_failures.len() as i64 > MAX_ROWS;
    let mut no_operating_hours = Vec::new();
    for a in with_failures.into_iter().take(MAX_ROWS as usize) {
        if !load_clock(db, a.equipment_id).await?.is_operating() {
            no_operating_hours.push(EquipmentGap {
                equipment_id: a.equipment_id,
                asset_code: a.asset_code.clone(),
                asset_name: a.asset_name.clone(),
                failures: a.failures,
            });
        }
    }

    let mut review_issues = Vec::new();
    let mut review_issues_truncated = false;
    for i in
        queries::list_ram_data_quality_issues(db, RamDataQualityIssuesFilter { equipment_id: None }, user_id).await?
    {
        if i.issue_code != "LOW_SAMPLE" && i.issue_code != "MISSING_INSPECTION_COVERAGE" {
            continue;
        }
        if let Some(r) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT e.asset_id_code, e.name FROM equipment e LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
                 WHERE e.id = ? AND e.deleted_at IS NULL
                   AND (? IS NULL OR e.class_id = ?)
                   AND (? IS NULL OR n.ancestor_path LIKE '%/' || ? || '/%')",
                [
                    i.equipment_id.into(),
                    opt_i64(scope.equipment_class_id),
                    opt_i64(scope.equipment_class_id),
                    opt_i64(scope.org_node_id),
                    opt_i64(scope.org_node_id),
                ],
            ))
            .await?
        {
            review_issues.push(ReviewIssue {
                equipment_id: i.equipment_id,
                asset_code: get(&r, "asset_id_code")?,
                asset_name: get(&r, "name")?,
                issue_code: i.issue_code,
                severity: i.severity,
            });
        }
        if review_issues.len() as i64 >= MAX_ROWS {
            review_issues_truncated = true;
            break;
        }
    }
    let (org_nodes, equipment_classes) = filter_options(db).await?;
    Ok(DataQualityView {
        period_start: s,
        period_end: e,
        coding_gaps,
        no_operating_hours,
        review_issues,
        records: coding.records,
        records_complete: coding.complete,
        records_missing_mode: coding.missing_mode,
        records_missing_cause: coding.missing_cause,
        records_missing_mechanism: coding.missing_mechanism,
        org_nodes,
        equipment_classes,
        coding_gaps_truncated,
        no_operating_hours_truncated,
        review_issues_truncated,
    })
}
