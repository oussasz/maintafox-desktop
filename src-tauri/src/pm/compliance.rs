//! PM compliance KPI (SMRP Best Practice 5.4.1 style) — the one definition used by the PM page,
//! the home tile, the Analytics SMRP tile and the monthly report.
//!
//! `PM compliance % = PM occurrences done within their tolerance window ÷ PM occurrences due in
//! the period whose outcome is known × 100`.
//!
//! - **Due in the period**: due date in `[from, to)`, never later than `now`, not cancelled
//!   (a cancelled PM is a planning decision, not work owed).
//! - **Outcome known**: the work was done, or its window has closed (end of window, else the end
//!   of the due day) by `now`. Work still inside its window is `pending` and left out: it cannot
//!   be judged yet.
//! - **Within the window**: the completion is `on_time` (`pm::schedule`). Work done before the
//!   window opens (`early`) or after it closes (`late`) is not compliant; an occurrence whose
//!   window closed without work is `not_done`.
//! - **Completion**: the last completed execution; an occurrence completed by a status change
//!   only counts from that change. Executions recorded before windows existed carry no timing:
//!   they are classified on read with the default window (the due day).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{PmComplianceInput, PmComplianceReport};
use crate::pm::schedule::{classify_timing, compliance_pct, rfc3339, time_window, OccurrenceWindow};
use crate::wo::access::ScopeSql;

fn parse(ts: &str, field: &str) -> AppResult<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(ts)
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_PM_DATE_INVALID: {field} must be a valid RFC3339 timestamp."
            )])
        })?
        .with_timezone(&Utc))
}

/// Which occurrences count: one plan, and/or the assets installed at one organisation node.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComplianceScope {
    pub pm_plan_id: Option<i64>,
    pub org_node_id: Option<i64>,
}

/// Compliance of the occurrences due in `[from, to)`, judged at `now` (see module docs).
pub async fn compliance_between(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
) -> AppResult<PmComplianceReport> {
    compliance_between_for_criticality(db, from, to, now, scope, None).await
}

/// [`compliance_between`] limited to the plans of one criticality (`pm_plans.criticality_value_id`
/// → `lookup_values.code`, compared upper-case) — the PM governance report's filter.
pub async fn compliance_between_for_criticality(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
    criticality_code: Option<&str>,
) -> AppResult<PmComplianceReport> {
    compliance_between_visible(db, from, to, now, scope, criticality_code, &ScopeSql::everywhere()).await
}

/// [`compliance_between_for_criticality`] counting only the occurrences matching `visible` (an SQL
/// predicate on the occurrence alias `po`: `pm::access::occurrence_view_filter`, the PM reader's
/// sites).
pub async fn compliance_between_visible(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
    criticality_code: Option<&str>,
    visible: &ScopeSql,
) -> AppResult<PmComplianceReport> {
    let rows = judged_occurrences(db, from, to, now, scope, criticality_code, visible).await?;
    let (mut on_time, mut early, mut late, mut not_done, mut pending) = (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
    for row in &rows {
        match row.outcome {
            Outcome::OnTime => on_time += 1,
            Outcome::Early => early += 1,
            Outcome::Late => late += 1,
            Outcome::NotDone => not_done += 1,
            Outcome::Pending => pending += 1,
        }
    }
    let due_count = on_time + early + late + not_done;
    Ok(PmComplianceReport {
        from: rfc3339(from),
        to: rfc3339(to),
        pm_plan_id: scope.pm_plan_id,
        due_count,
        on_time_count: on_time,
        early_count: early,
        late_count: late,
        not_done_count: not_done,
        pending_count: pending,
        compliance_pct: compliance_pct(on_time, due_count),
    })
}

/// How one occurrence due in the period is judged (see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    OnTime,
    Early,
    Late,
    NotDone,
    /// Still inside its window: not judged yet, left out of the ratio.
    Pending,
}

/// One occurrence due in the period, with what the breakdowns group it by.
struct JudgedOccurrence {
    outcome: Outcome,
    /// Organisation node the asset is installed at (`None`: no asset or not installed).
    node_id: Option<i64>,
    /// The plan's assigned team — its trade (`None`: no team).
    team_id: Option<i64>,
    team_name: Option<String>,
}

/// The occurrences due in `[from, to)` matching the filters, each judged at `now`.
async fn judged_occurrences(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
    criticality_code: Option<&str>,
    visible: &ScopeSql,
) -> AppResult<Vec<JudgedOccurrence>> {
    let upper = to.min(now);
    // Timestamps are normalised to RFC3339 in SQL (rows written by SQLite defaults use another form).
    let mut sql = String::from(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', po.due_at) AS due_at, po.window_start_at, po.window_end_at,
                po.window_start_meter, po.window_end_meter,
                strftime('%Y-%m-%dT%H:%M:%SZ', COALESCE(done.executed_at,
                    CASE WHEN po.status = 'completed' THEN COALESCE(
                        (SELECT MIN(t.transitioned_at) FROM pm_occurrence_transitions t
                         WHERE t.pm_occurrence_id = po.id AND t.to_status = 'completed'),
                        po.updated_at) END)) AS done_at,
                done.timing_status, done.meter_reading,
                eq.installed_at_node_id AS node_id, pp.assigned_team_id AS team_id, tm.name AS team_name
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN teams tm ON tm.id = pp.assigned_team_id
         LEFT JOIN equipment eq ON eq.id =
             COALESCE(po.equipment_id, CASE WHEN pp.asset_scope_type = 'equipment' THEN pp.asset_scope_id END)
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         LEFT JOIN pm_executions done ON done.id = (
             SELECT pe.id FROM pm_executions pe
             WHERE pe.pm_occurrence_id = po.id
               AND pe.execution_result IN ('completed_no_findings','completed_with_findings')
             ORDER BY pe.executed_at DESC, pe.id DESC LIMIT 1)
         WHERE po.due_at IS NOT NULL AND po.status <> 'cancelled'
           AND datetime(po.due_at) >= datetime(?) AND datetime(po.due_at) < datetime(?)",
    );
    let mut binds: Vec<sea_orm::Value> = vec![rfc3339(from).into(), rfc3339(upper).into()];
    sql.push_str(" AND ");
    sql.push_str(&visible.predicate);
    binds.extend(visible.binds.iter().cloned());
    if let Some(plan_id) = scope.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(plan_id.into());
    }
    if let Some(node) = scope.org_node_id {
        sql.push_str(" AND eq.installed_at_node_id = ?");
        binds.push(node.into());
    }
    if let Some(code) = criticality_code {
        sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        binds.push(code.trim().to_uppercase().into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let node_id: Option<i64> = row.try_get("", "node_id")?;
        let team_id: Option<i64> = row.try_get("", "team_id")?;
        let team_name: Option<String> = row.try_get("", "team_name")?;
        let due_at: String = row.try_get("", "due_at")?;
        let mut window = OccurrenceWindow {
            start_at: row.try_get("", "window_start_at")?,
            end_at: row.try_get("", "window_end_at")?,
            start_meter: row.try_get("", "window_start_meter")?,
            end_meter: row.try_get("", "window_end_meter")?,
        };
        if window.end_at.is_none() {
            let (start, end) = time_window(parse(&due_at, "due_at")?, None, None);
            window.start_at = start.map(rfc3339);
            window.end_at = Some(rfc3339(end));
        }
        let done_at: Option<String> = row.try_get("", "done_at")?;
        let Some(done_at) = done_at else {
            let closed = window
                .end_at
                .as_deref()
                .map(|e| parse(e, "window_end_at"))
                .transpose()?
                .is_some_and(|end| end <= now);
            out.push(JudgedOccurrence {
                outcome: if closed { Outcome::NotDone } else { Outcome::Pending },
                node_id,
                team_id,
                team_name,
            });
            continue;
        };
        let stored: Option<String> = row.try_get("", "timing_status")?;
        let timing = match stored {
            Some(t) => Some(t),
            None => {
                let reading: Option<f64> = row.try_get("", "meter_reading")?;
                classify_timing(&window, parse(&done_at, "done_at")?, reading).map(str::to_string)
            }
        };
        let outcome = match timing.as_deref() {
            Some("on_time") => Outcome::OnTime,
            Some("early") => Outcome::Early,
            Some("late") => Outcome::Late,
            _ => Outcome::NotDone,
        };
        out.push(JudgedOccurrence {
            outcome,
            node_id,
            team_id,
            team_name,
        });
    }
    Ok(out)
}

/// PM compliance of one site or one team (trade): the same rule as the total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PmComplianceGroup {
    /// Site (`org_nodes.id`) or team (`teams.id`); `None`: occurrences with no site / no team.
    pub id: Option<i64>,
    pub label: Option<String>,
    pub due_count: i64,
    pub on_time_count: i64,
    pub compliance_pct: Option<f64>,
}

/// PM compliance split by site (planning site of the asset's installation node) and by trade
/// (the plan's assigned team). Occurrences not judged yet are left out, as in the total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PmComplianceBreakdown {
    pub by_site: Vec<PmComplianceGroup>,
    pub by_team: Vec<PmComplianceGroup>,
}

type GroupTally = BTreeMap<Option<i64>, (Option<String>, i64, i64)>;

fn tally(groups: &mut GroupTally, key: Option<i64>, label: Option<String>, outcome: Outcome) {
    if outcome == Outcome::Pending {
        return;
    }
    let entry = groups.entry(key).or_insert((label, 0, 0));
    entry.1 += 1;
    if outcome == Outcome::OnTime {
        entry.2 += 1;
    }
}

fn into_groups(groups: GroupTally) -> Vec<PmComplianceGroup> {
    let mut out: Vec<PmComplianceGroup> = groups
        .into_iter()
        .map(|(id, (label, due, on_time))| PmComplianceGroup {
            id,
            label,
            due_count: due,
            on_time_count: on_time,
            compliance_pct: compliance_pct(on_time, due),
        })
        .collect();
    // Named groups by name, the "none" group last.
    out.sort_by(|a, b| (a.id.is_none(), a.label.as_deref()).cmp(&(b.id.is_none(), b.label.as_deref())));
    out
}

/// [`compliance_between`] split by site and by trade (see [`PmComplianceBreakdown`]).
pub async fn compliance_breakdown(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
) -> AppResult<PmComplianceBreakdown> {
    let rows = judged_occurrences(db, from, to, now, scope, None, &ScopeSql::everywhere()).await?;
    let mut site_of_node: BTreeMap<i64, Option<(i64, String)>> = BTreeMap::new();
    for node in rows.iter().filter_map(|r| r.node_id) {
        if site_of_node.contains_key(&node) {
            continue;
        }
        let site = match crate::planning::weekly_schedule::planning_site_of_node(db, node).await? {
            Some(site_id) => {
                let name = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT name FROM org_nodes WHERE id = ?",
                        [site_id.into()],
                    ))
                    .await?
                    .map(|r| r.try_get::<String>("", "name"))
                    .transpose()?
                    .unwrap_or_default();
                Some((site_id, name))
            }
            None => None,
        };
        site_of_node.insert(node, site);
    }
    let mut by_site = GroupTally::new();
    let mut by_team = GroupTally::new();
    for r in rows {
        let site = r.node_id.and_then(|n| site_of_node.get(&n).cloned().flatten());
        let (site_id, site_name) = match site {
            Some((id, name)) => (Some(id), Some(name)),
            None => (None, None),
        };
        tally(&mut by_site, site_id, site_name, r.outcome);
        tally(&mut by_team, r.team_id, r.team_name, r.outcome);
    }
    Ok(PmComplianceBreakdown {
        by_site: into_groups(by_site),
        by_team: into_groups(by_team),
    })
}

/// IPC entry point (PM page): the period given, judged now.
pub async fn get_pm_compliance(db: &impl ConnectionTrait, input: PmComplianceInput) -> AppResult<PmComplianceReport> {
    get_pm_compliance_visible(db, input, &ScopeSql::everywhere()).await
}

/// [`get_pm_compliance`] counting only the occurrences matching `visible` (see
/// [`compliance_between_visible`]).
pub async fn get_pm_compliance_visible(
    db: &impl ConnectionTrait,
    input: PmComplianceInput,
    visible: &ScopeSql,
) -> AppResult<PmComplianceReport> {
    let from = parse(&input.from, "from")?;
    let to = parse(&input.to, "to")?;
    if to <= from {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PM_PERIOD_ORDER: to must be after from.".to_string(),
        ]));
    }
    compliance_between_visible(
        db,
        from,
        to,
        crate::clock::now(),
        ComplianceScope {
            pm_plan_id: input.pm_plan_id,
            org_node_id: None,
        },
        None,
        visible,
    )
    .await
}
