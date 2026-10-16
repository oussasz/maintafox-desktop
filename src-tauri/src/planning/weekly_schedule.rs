//! The weekly maintenance schedule (planning step D, migration 212): one per planning site and
//! week, the site's crews as rows.
//!
//! - A crew's planning site is the nearest organisation node, at or above its primary node, whose
//!   node type can own work (`org_node_types.can_own_work`). A crew without one is listed, never
//!   placed in a guessed schedule.
//! - Status: `draft` → `approved` (plan.confirm) → `frozen` → `published` (the crews are notified).
//!   Freezing takes the baseline: the week's commitments as they stand, and marks them frozen
//!   (they then change only through a recorded break-in).
//! - The next week freezes automatically at the end of the current week's last working day. An
//!   automatic freeze never stands for an approval: a week that was not approved is frozen
//!   "without approval" and stays marked — and notified — until someone approves it after the
//!   fact (which does not change the baseline).
//! - Unfreezing is four-eyes: requested with a reason, then approved (or rejected) by another
//!   holder of plan.confirm. A site with a single plan.confirm holder cannot unfreeze.
//!
//! Several devices: a schedule, its baseline items and its automatic-freeze event have identities
//! built from the synchronised rows they come from (site + week, schedule + baseline + commitment),
//! so two devices that create or freeze the same week offline produce the same rows instead of
//! clashing on `UNIQUE (entity_id, week_start)`. A received schedule never moves a week backwards
//! in its lifecycle (`inbound_lifecycle_conflict`, called by the sync apply): that change goes to
//! the synchronisation conflicts for a person to decide.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppError, AppResult};
use crate::sync::identity::{new_entity_sync_id, optional_entity_sync_id, seeded_entity_sync_id};

use super::domain::{ScheduleCommitment, ScheduleCommitmentFilter};
use super::weekly::{site_week, SiteWeek};

pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_FROZEN: &str = "frozen";
pub const STATUS_PUBLISHED: &str = "published";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("weekly schedule decode '{field}': {err}"))
}

fn now_z() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The first instant of `day` in a time zone (`resolve` maps a local time to its instants):
/// midnight, or — where the clock jumps over midnight (daylight saving) — the first valid local
/// time after the gap; the earlier one where midnight happens twice.
pub(crate) fn first_instant_of_day(
    day: NaiveDate,
    resolve: impl Fn(NaiveDateTime) -> LocalResult<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let midnight = day.and_hms_opt(0, 0, 0)?;
    (0..=24 * 60)
        .map(|minute| midnight + Duration::minutes(minute))
        .find_map(|local| resolve(local).earliest())
}

/// The first instant of the local `day` (its midnight, or the end of a daylight-saving gap).
fn local_midnight_utc(day: NaiveDate) -> AppResult<DateTime<Utc>> {
    first_instant_of_day(day, |local| {
        Local.from_local_datetime(&local).map(|t| t.with_timezone(&Utc))
    })
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("local day {day} has no valid instant")))
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ── Model ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklySchedule {
    pub id: i64,
    pub entity_id: i64,
    pub week_start: String,
    pub week_end: String,
    pub status: String,
    pub approved_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub frozen_at: Option<String>,
    /// `None` with `frozen_at` set: frozen automatically.
    pub frozen_by_id: Option<i64>,
    pub frozen_without_approval: i64,
    pub baseline_no: i64,
    pub published_by_id: Option<i64>,
    pub published_at: Option<String>,
    pub unfreeze_requested_by_id: Option<i64>,
    pub unfreeze_requested_at: Option<String>,
    pub unfreeze_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

const SCHEDULE_COLS: &str = "id, entity_id, week_start, week_end, status, approved_by_id, approved_at, frozen_at, \
    frozen_by_id, frozen_without_approval, baseline_no, published_by_id, published_at, unfreeze_requested_by_id, \
    unfreeze_requested_at, unfreeze_reason, row_version, created_at, updated_at";

fn map_schedule(r: &QueryResult) -> AppResult<WeeklySchedule> {
    Ok(WeeklySchedule {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_id: r.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
        week_start: r.try_get("", "week_start").map_err(|e| decode_err("week_start", e))?,
        week_end: r.try_get("", "week_end").map_err(|e| decode_err("week_end", e))?,
        status: r.try_get("", "status").map_err(|e| decode_err("status", e))?,
        approved_by_id: r
            .try_get("", "approved_by_id")
            .map_err(|e| decode_err("approved_by_id", e))?,
        approved_at: r.try_get("", "approved_at").map_err(|e| decode_err("approved_at", e))?,
        frozen_at: r.try_get("", "frozen_at").map_err(|e| decode_err("frozen_at", e))?,
        frozen_by_id: r
            .try_get("", "frozen_by_id")
            .map_err(|e| decode_err("frozen_by_id", e))?,
        frozen_without_approval: r
            .try_get("", "frozen_without_approval")
            .map_err(|e| decode_err("frozen_without_approval", e))?,
        baseline_no: r.try_get("", "baseline_no").map_err(|e| decode_err("baseline_no", e))?,
        published_by_id: r
            .try_get("", "published_by_id")
            .map_err(|e| decode_err("published_by_id", e))?,
        published_at: r
            .try_get("", "published_at")
            .map_err(|e| decode_err("published_at", e))?,
        unfreeze_requested_by_id: r
            .try_get("", "unfreeze_requested_by_id")
            .map_err(|e| decode_err("unfreeze_requested_by_id", e))?,
        unfreeze_requested_at: r
            .try_get("", "unfreeze_requested_at")
            .map_err(|e| decode_err("unfreeze_requested_at", e))?,
        unfreeze_reason: r
            .try_get("", "unfreeze_reason")
            .map_err(|e| decode_err("unfreeze_reason", e))?,
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: r.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyScheduleEvent {
    pub id: i64,
    pub action: String,
    pub actor_id: Option<i64>,
    pub actor_name: Option<String>,
    pub approver_id: Option<i64>,
    pub approver_name: Option<String>,
    pub reason: Option<String>,
    pub baseline_no: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyCrew {
    pub team_id: i64,
    pub team_code: String,
    pub team_name: String,
}

/// Everything the weekly schedule screen shows for one site and week.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyScheduleView {
    pub week: SiteWeek,
    pub site_name: String,
    /// `None`: nothing was done on this week yet (it is a draft).
    pub schedule: Option<WeeklySchedule>,
    /// Frozen without approval and still not approved after the fact.
    pub awaiting_approval: bool,
    /// When this week freezes automatically (end of the previous week's last working day).
    pub freeze_due_at: String,
    pub crews: Vec<WeeklyCrew>,
    /// The crews' commitments over the week (held and finished).
    pub commitments: Vec<ScheduleCommitment>,
    pub events: Vec<WeeklyScheduleEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyScheduleRef {
    pub site_id: i64,
    /// Any day of the week (`YYYY-MM-DD`).
    pub day: String,
    pub expected_row_version: Option<i64>,
}

// ── Sites and crews ──────────────────────────────────────────────────────────────────────

/// The planning site of an organisation node: the nearest node at or above it whose type can own
/// work.
pub async fn planning_site_of_node(db: &impl ConnectionTrait, node_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE anc(id, parent_id, depth) AS ( \
                SELECT id, parent_id, 0 FROM org_nodes WHERE id = ? \
                UNION ALL \
                SELECT n.id, n.parent_id, a.depth + 1 FROM org_nodes n JOIN anc a ON n.id = a.parent_id) \
             SELECT anc.id FROM anc JOIN org_nodes n ON n.id = anc.id \
               JOIN org_node_types t ON t.id = n.node_type_id \
              WHERE t.can_own_work = 1 AND n.deleted_at IS NULL \
              ORDER BY anc.depth LIMIT 1",
            [node_id.into()],
        ))
        .await?;
    row.map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
        .transpose()
}

/// Active crews and their planning site (`None`: no site).
async fn crews_with_sites(db: &impl ConnectionTrait) -> AppResult<Vec<(WeeklyCrew, Option<i64>)>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, primary_node_id FROM teams \
              WHERE deleted_at IS NULL AND status = 'active' ORDER BY code, id"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let node: Option<i64> = r
            .try_get("", "primary_node_id")
            .map_err(|e| decode_err("primary_node_id", e))?;
        let site = match node {
            Some(n) => planning_site_of_node(db, n).await?,
            None => None,
        };
        out.push((
            WeeklyCrew {
                team_id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                team_code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
                team_name: r.try_get("", "name").map_err(|e| decode_err("name", e))?,
            },
            site,
        ));
    }
    Ok(out)
}

/// The crews whose planning site is `site`.
pub async fn site_crews(db: &impl ConnectionTrait, site: i64) -> AppResult<Vec<WeeklyCrew>> {
    Ok(crews_with_sites(db)
        .await?
        .into_iter()
        .filter(|(_, s)| *s == Some(site))
        .map(|(c, _)| c)
        .collect())
}

/// The crews that have no planning site (shown so they are not silently left out).
pub async fn crews_without_site(db: &impl ConnectionTrait) -> AppResult<Vec<WeeklyCrew>> {
    Ok(crews_with_sites(db)
        .await?
        .into_iter()
        .filter(|(_, s)| s.is_none())
        .map(|(c, _)| c)
        .collect())
}

// ── Loading ──────────────────────────────────────────────────────────────────────────────

async fn load_schedule(
    db: &impl ConnectionTrait,
    site: i64,
    week_start: NaiveDate,
) -> AppResult<Option<WeeklySchedule>> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT {SCHEDULE_COLS} FROM weekly_schedules WHERE entity_id = ? AND week_start = ?"),
        [site.into(), week_start.to_string().into()],
    ))
    .await?
    .as_ref()
    .map(map_schedule)
    .transpose()
}

/// Identity of the schedule of a site and week: the same on every device (site sync id + week).
async fn schedule_sync_id(db: &impl ConnectionTrait, week: &SiteWeek) -> AppResult<String> {
    Ok(match optional_entity_sync_id(db, "org_nodes", week.entity_id).await? {
        Some(site) => seeded_entity_sync_id("weekly_schedules", &format!("{site}|{}", week.week_start)),
        // A database without sync capture (tools): nothing to converge with.
        None => new_entity_sync_id(),
    })
}

async fn ensure_schedule(db: &impl ConnectionTrait, week: &SiteWeek) -> AppResult<WeeklySchedule> {
    if let Some(found) = load_schedule(db, week.entity_id, week.week_start).await? {
        return Ok(found);
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO weekly_schedules (entity_sync_id, entity_id, week_start, week_end, status) VALUES (?, ?, ?, ?, 'draft') \
         ON CONFLICT DO NOTHING",
        [
            schedule_sync_id(db, week).await?.into(),
            week.entity_id.into(),
            week.week_start.to_string().into(),
            week.week_end.to_string().into(),
        ],
    ))
    .await?;
    load_schedule(db, week.entity_id, week.week_start)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("weekly schedule just created is missing")))
}

/// Records what a person (or the system, `actor_id` `None`) did to a schedule. An event that
/// follows from the data alone (the automatic freeze of a baseline) gets an identity built from
/// it, so the devices that do it produce one event; a person's action gets a new identity.
async fn record_event(
    db: &impl ConnectionTrait,
    schedule_id: i64,
    action: &str,
    actor_id: Option<i64>,
    approver_id: Option<i64>,
    reason: Option<&str>,
    baseline_no: Option<i64>,
) -> AppResult<()> {
    let sync_id = match (actor_id, baseline_no) {
        (None, Some(baseline)) => match optional_entity_sync_id(db, "weekly_schedules", schedule_id).await? {
            Some(schedule) => {
                seeded_entity_sync_id("weekly_schedule_events", &format!("{schedule}|{action}|{baseline}"))
            }
            None => new_entity_sync_id(),
        },
        _ => new_entity_sync_id(),
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO weekly_schedule_events \
            (entity_sync_id, weekly_schedule_id, action, actor_id, approver_id, reason, baseline_no) \
         VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        [
            sync_id.into(),
            schedule_id.into(),
            action.into(),
            actor_id.into(),
            approver_id.into(),
            reason.map(str::to_string).into(),
            baseline_no.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn list_events(db: &impl ConnectionTrait, schedule_id: i64) -> AppResult<Vec<WeeklyScheduleEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.action, e.actor_id, COALESCE(a.display_name, a.username) AS actor_name, \
                    e.approver_id, COALESCE(p.display_name, p.username) AS approver_name, e.reason, \
                    e.baseline_no, e.created_at \
               FROM weekly_schedule_events e \
               LEFT JOIN user_accounts a ON a.id = e.actor_id \
               LEFT JOIN user_accounts p ON p.id = e.approver_id \
              WHERE e.weekly_schedule_id = ? ORDER BY e.created_at, e.id",
            [schedule_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(WeeklyScheduleEvent {
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                action: r.try_get("", "action").map_err(|e| decode_err("action", e))?,
                actor_id: r.try_get("", "actor_id").map_err(|e| decode_err("actor_id", e))?,
                actor_name: r.try_get("", "actor_name").map_err(|e| decode_err("actor_name", e))?,
                approver_id: r.try_get("", "approver_id").map_err(|e| decode_err("approver_id", e))?,
                approver_name: r
                    .try_get("", "approver_name")
                    .map_err(|e| decode_err("approver_name", e))?,
                reason: r.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
                baseline_no: r.try_get("", "baseline_no").map_err(|e| decode_err("baseline_no", e))?,
                created_at: r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
            })
        })
        .collect()
}

/// When `week` freezes automatically: local midnight after the previous week's last working day
/// (after the previous week's end when it had none).
async fn freeze_due_at(db: &impl ConnectionTrait, week: &SiteWeek) -> AppResult<DateTime<Utc>> {
    let previous = site_week(db, week.entity_id, week.week_start - Duration::days(1)).await?;
    let last = previous.last_working_day.unwrap_or(previous.week_end);
    local_midnight_utc(last + Duration::days(1))
}

/// The commitments held (or finished) by these crews over the week, `committed_start` order.
async fn week_commitments(
    db: &DatabaseConnection,
    crews: &[WeeklyCrew],
    week: &SiteWeek,
) -> AppResult<Vec<ScheduleCommitment>> {
    let start = rfc3339(local_midnight_utc(week.week_start)?);
    let end = rfc3339(local_midnight_utc(week.week_end + Duration::days(1))?);
    let mut out = Vec::new();
    for crew in crews {
        out.extend(
            super::scheduling::list_schedule_commitments(
                db,
                ScheduleCommitmentFilter {
                    period_start: Some(start.clone()),
                    period_end: Some(end.clone()),
                    team_id: Some(crew.team_id),
                    personnel_id: None,
                },
            )
            .await?,
        );
    }
    out.sort_by(|a, b| a.committed_start.cmp(&b.committed_start).then(a.id.cmp(&b.id)));
    Ok(out)
}

async fn site_name(db: &impl ConnectionTrait, site: i64) -> AppResult<String> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT name FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
        [site.into()],
    ))
    .await?
    .map(|r| r.try_get::<String>("", "name").map_err(|e| decode_err("name", e)))
    .transpose()?
    .ok_or_else(|| AppError::NotFound {
        entity: "OrgNode".to_string(),
        id: site.to_string(),
    })
}

fn parse_day(day: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d").map_err(|_| {
        org_validation_failed(
            "PLANNING_WEEK_DAY_INVALID",
            format!("'{day}' is not a date (YYYY-MM-DD)."),
            HashMap::from([("day".to_string(), day.to_string())]),
        )
    })
}

/// The weekly schedule of `site` for the week containing `day`.
pub async fn get_weekly_schedule(db: &DatabaseConnection, site: i64, day: &str) -> AppResult<WeeklyScheduleView> {
    let week = site_week(db, site, parse_day(day)?).await?;
    let schedule = load_schedule(db, site, week.week_start).await?;
    let crews = site_crews(db, site).await?;
    let commitments = week_commitments(db, &crews, &week).await?;
    let events = match &schedule {
        Some(s) => list_events(db, s.id).await?,
        None => Vec::new(),
    };
    Ok(WeeklyScheduleView {
        site_name: site_name(db, site).await?,
        awaiting_approval: schedule.as_ref().is_some_and(awaits_approval),
        freeze_due_at: rfc3339(freeze_due_at(db, &week).await?),
        schedule,
        crews,
        commitments,
        events,
        week,
    })
}

/// Frozen (or published) without approval, and nobody approved it since.
fn awaits_approval(s: &WeeklySchedule) -> bool {
    matches!(s.status.as_str(), STATUS_FROZEN | STATUS_PUBLISHED)
        && s.frozen_without_approval == 1
        && s.approved_at.is_none()
}

// ── Transitions ──────────────────────────────────────────────────────────────────────────

fn check_version(schedule: &WeeklySchedule, expected: Option<i64>) -> AppResult<()> {
    match expected {
        Some(v) if v != schedule.row_version => Err(AppError::ValidationFailed(vec![
            "Weekly schedule was modified elsewhere (stale row_version).".to_string(),
        ])),
        _ => Ok(()),
    }
}

fn wrong_status(schedule: &WeeklySchedule, action: &str) -> AppError {
    org_validation_failed(
        "PLANNING_WEEK_STATUS",
        format!("This week is {}: it cannot be {action} now.", schedule.status),
        HashMap::from([
            ("status".to_string(), schedule.status.clone()),
            ("action".to_string(), action.to_string()),
        ]),
    )
}

/// Approves the week (plan.confirm, checked by the command). On a week frozen without approval it
/// records the approval after the fact; the baseline is unchanged.
pub async fn approve_weekly_schedule(
    db: &DatabaseConnection,
    actor_id: i64,
    input: WeeklyScheduleRef,
) -> AppResult<WeeklyScheduleView> {
    let tx = db.begin().await?;
    let week = site_week(&tx, input.site_id, parse_day(&input.day)?).await?;
    let schedule = ensure_schedule(&tx, &week).await?;
    check_version(&schedule, input.expected_row_version)?;
    let now = now_z();
    let action = match schedule.status.as_str() {
        STATUS_DRAFT => "approved",
        STATUS_FROZEN | STATUS_PUBLISHED if schedule.approved_at.is_none() => "approved_after_freeze",
        _ => return Err(wrong_status(&schedule, "approved")),
    };
    let next_status = if action == "approved" {
        STATUS_APPROVED
    } else {
        schedule.status.as_str()
    };
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE weekly_schedules SET status = ?, approved_by_id = ?, approved_at = ?, \
                row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [
            next_status.into(),
            actor_id.into(),
            now.clone().into(),
            now.into(),
            schedule.id.into(),
        ],
    ))
    .await?;
    record_event(&tx, schedule.id, action, Some(actor_id), None, None, None).await?;
    tx.commit().await?;
    get_weekly_schedule(db, input.site_id, &input.day).await
}

/// Freezes a week: takes its baseline (the commitments as they stand) and marks them frozen.
/// `actor_id` `None`: the automatic freeze.
async fn freeze_core(
    db: &impl ConnectionTrait,
    schedule: &WeeklySchedule,
    week: &SiteWeek,
    actor_id: Option<i64>,
) -> AppResult<i64> {
    let crews = site_crews(db, week.entity_id).await?;
    let baseline_no = schedule.baseline_no + 1;
    let schedule_sid = optional_entity_sync_id(db, "weekly_schedules", schedule.id).await?;
    let start = rfc3339(local_midnight_utc(week.week_start)?);
    let end = rfc3339(local_midnight_utc(week.week_end + Duration::days(1))?);
    let now = now_z();
    for crew in &crews {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, assigned_team_id, assigned_personnel_id, committed_start, committed_end, \
                        planned_labor_hours \
                   FROM schedule_commitments \
                  WHERE status = 'committed' AND assigned_team_id = ? \
                    AND julianday(committed_start) < julianday(?) AND julianday(committed_end) > julianday(?)",
                [crew.team_id.into(), end.clone().into(), start.clone().into()],
            ))
            .await?;
        for r in rows {
            let commitment_id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
            // Same item on every device that freezes this baseline: schedule + baseline + commitment.
            let item_sid = match (
                &schedule_sid,
                optional_entity_sync_id(db, "schedule_commitments", commitment_id).await?,
            ) {
                (Some(schedule), Some(commitment)) => seeded_entity_sync_id(
                    "weekly_schedule_baseline_items",
                    &format!("{schedule}|{baseline_no}|{commitment}"),
                ),
                _ => new_entity_sync_id(),
            };
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO weekly_schedule_baseline_items \
                    (entity_sync_id, weekly_schedule_id, baseline_no, schedule_commitment_id, team_id, personnel_id, \
                     committed_start, committed_end, planned_labor_hours) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
                [
                    item_sid.into(),
                    schedule.id.into(),
                    baseline_no.into(),
                    commitment_id.into(),
                    r.try_get::<i64>("", "assigned_team_id")
                        .map_err(|e| decode_err("assigned_team_id", e))?
                        .into(),
                    r.try_get::<Option<i64>>("", "assigned_personnel_id")
                        .map_err(|e| decode_err("assigned_personnel_id", e))?
                        .into(),
                    r.try_get::<String>("", "committed_start")
                        .map_err(|e| decode_err("committed_start", e))?
                        .into(),
                    r.try_get::<String>("", "committed_end")
                        .map_err(|e| decode_err("committed_end", e))?
                        .into(),
                    r.try_get::<Option<f64>>("", "planned_labor_hours")
                        .map_err(|e| decode_err("planned_labor_hours", e))?
                        .into(),
                ],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE schedule_commitments SET frozen_at = COALESCE(frozen_at, ?), row_version = row_version + 1, \
                        updated_at = ? WHERE id = ?",
                [now.clone().into(), now.clone().into(), commitment_id.into()],
            ))
            .await?;
        }
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE weekly_schedules SET status = 'frozen', frozen_at = ?, frozen_by_id = ?, \
                frozen_without_approval = CASE WHEN approved_at IS NULL THEN 1 ELSE frozen_without_approval END, \
                baseline_no = ?, unfreeze_requested_by_id = NULL, unfreeze_requested_at = NULL, unfreeze_reason = NULL, \
                row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [
            now.clone().into(),
            actor_id.into(),
            baseline_no.into(),
            now.into(),
            schedule.id.into(),
        ],
    ))
    .await?;
    record_event(
        db,
        schedule.id,
        if actor_id.is_some() { "frozen" } else { "auto_frozen" },
        actor_id,
        None,
        None,
        Some(baseline_no),
    )
    .await?;
    Ok(baseline_no)
}

/// Freezes the week now (plan.confirm, checked by the command). A draft is approved by the same
/// person as part of it: holding plan.confirm, freezing it is approving it.
pub async fn freeze_weekly_schedule(
    db: &DatabaseConnection,
    actor_id: i64,
    input: WeeklyScheduleRef,
) -> AppResult<WeeklyScheduleView> {
    let tx = db.begin().await?;
    let week = site_week(&tx, input.site_id, parse_day(&input.day)?).await?;
    let mut schedule = ensure_schedule(&tx, &week).await?;
    check_version(&schedule, input.expected_row_version)?;
    match schedule.status.as_str() {
        STATUS_DRAFT => {
            let now = now_z();
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE weekly_schedules SET status = 'approved', approved_by_id = ?, approved_at = ?, \
                        row_version = row_version + 1, updated_at = ? WHERE id = ?",
                [actor_id.into(), now.clone().into(), now.into(), schedule.id.into()],
            ))
            .await?;
            record_event(&tx, schedule.id, "approved", Some(actor_id), None, None, None).await?;
            schedule = load_schedule(&tx, input.site_id, week.week_start)
                .await?
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("weekly schedule vanished")))?;
        }
        STATUS_APPROVED => {}
        _ => return Err(wrong_status(&schedule, "frozen")),
    }
    freeze_core(&tx, &schedule, &week, Some(actor_id)).await?;
    tx.commit().await?;
    get_weekly_schedule(db, input.site_id, &input.day).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnfreezeRequestInput {
    pub site_id: i64,
    pub day: String,
    pub expected_row_version: Option<i64>,
    pub reason: String,
}

/// Requests to unfreeze a frozen (or published) week, with a reason; another holder of
/// plan.confirm decides.
pub async fn request_unfreeze(
    db: &DatabaseConnection,
    actor_id: i64,
    input: UnfreezeRequestInput,
) -> AppResult<WeeklyScheduleView> {
    let reason = input.reason.trim().to_string();
    if reason.is_empty() {
        return Err(org_validation_failed(
            "PLANNING_UNFREEZE_REASON_REQUIRED",
            "Give the reason the week must be unfrozen.",
            HashMap::new(),
        ));
    }
    let tx = db.begin().await?;
    let week = site_week(&tx, input.site_id, parse_day(&input.day)?).await?;
    let schedule = load_schedule(&tx, input.site_id, week.week_start)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WeeklySchedule".to_string(),
            id: format!("{}:{}", input.site_id, week.week_start),
        })?;
    check_version(&schedule, input.expected_row_version)?;
    if !matches!(schedule.status.as_str(), STATUS_FROZEN | STATUS_PUBLISHED) {
        return Err(wrong_status(&schedule, "unfrozen"));
    }
    if schedule.unfreeze_requested_at.is_some() {
        return Err(org_validation_failed(
            "PLANNING_UNFREEZE_ALREADY_REQUESTED",
            "An unfreeze of this week is already waiting for a decision.",
            HashMap::new(),
        ));
    }
    let now = now_z();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE weekly_schedules SET unfreeze_requested_by_id = ?, unfreeze_requested_at = ?, unfreeze_reason = ?, \
                row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [
            actor_id.into(),
            now.clone().into(),
            reason.clone().into(),
            now.into(),
            schedule.id.into(),
        ],
    ))
    .await?;
    record_event(
        &tx,
        schedule.id,
        "unfreeze_requested",
        Some(actor_id),
        None,
        Some(&reason),
        None,
    )
    .await?;
    tx.commit().await?;
    get_weekly_schedule(db, input.site_id, &input.day).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnfreezeDecisionInput {
    pub site_id: i64,
    pub day: String,
    pub expected_row_version: Option<i64>,
    pub approve: bool,
    pub comment: Option<String>,
}

/// Approves or rejects a pending unfreeze. The decider holds plan.confirm (checked by the
/// command) and is not the requester. Approved: the week's commitments are no longer frozen and
/// the week goes back to approved (or draft); freezing it again takes a new baseline.
pub async fn decide_unfreeze(
    db: &DatabaseConnection,
    actor_id: i64,
    input: UnfreezeDecisionInput,
) -> AppResult<WeeklyScheduleView> {
    let tx = db.begin().await?;
    let week = site_week(&tx, input.site_id, parse_day(&input.day)?).await?;
    let schedule = load_schedule(&tx, input.site_id, week.week_start)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WeeklySchedule".to_string(),
            id: format!("{}:{}", input.site_id, week.week_start),
        })?;
    check_version(&schedule, input.expected_row_version)?;
    let Some(requester) = schedule.unfreeze_requested_by_id else {
        return Err(org_validation_failed(
            "PLANNING_UNFREEZE_NOT_REQUESTED",
            "No unfreeze of this week is waiting for a decision.",
            HashMap::new(),
        ));
    };
    if requester == actor_id {
        return Err(org_validation_failed(
            "PLANNING_UNFREEZE_SAME_PERSON",
            "The unfreeze must be decided by another planner holding plan.confirm (four-eyes).",
            HashMap::new(),
        ));
    }
    let comment = input.comment.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    let now = now_z();
    if !input.approve {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE weekly_schedules SET unfreeze_requested_by_id = NULL, unfreeze_requested_at = NULL, \
                    unfreeze_reason = NULL, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [now.into(), schedule.id.into()],
        ))
        .await?;
        record_event(
            &tx,
            schedule.id,
            "unfreeze_rejected",
            Some(requester),
            Some(actor_id),
            comment.as_deref().or(schedule.unfreeze_reason.as_deref()),
            None,
        )
        .await?;
        tx.commit().await?;
        return get_weekly_schedule(db, input.site_id, &input.day).await;
    }
    unfreeze_week_commitments(&tx, &schedule, &week, &now).await?;
    let back_to = if schedule.approved_at.is_some() {
        STATUS_APPROVED
    } else {
        STATUS_DRAFT
    };
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE weekly_schedules SET status = ?, frozen_at = NULL, frozen_by_id = NULL, published_at = NULL, \
                published_by_id = NULL, unfreeze_requested_by_id = NULL, unfreeze_requested_at = NULL, \
                unfreeze_reason = NULL, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [back_to.into(), now.into(), schedule.id.into()],
    ))
    .await?;
    record_event(
        &tx,
        schedule.id,
        "unfrozen",
        Some(requester),
        Some(actor_id),
        schedule.unfreeze_reason.as_deref(),
        Some(schedule.baseline_no),
    )
    .await?;
    tx.commit().await?;
    get_weekly_schedule(db, input.site_id, &input.day).await
}

/// The frozen commitments of the site's crews that touch the week are no longer frozen — those of
/// its baseline and those that entered it as break-ins — except a commitment that also touches
/// another week of the site still frozen or published (it stays frozen for that week).
async fn unfreeze_week_commitments(
    db: &impl ConnectionTrait,
    schedule: &WeeklySchedule,
    week: &SiteWeek,
    now: &str,
) -> AppResult<()> {
    let start = rfc3339(local_midnight_utc(week.week_start)?);
    let end = rfc3339(local_midnight_utc(week.week_end + Duration::days(1))?);
    for crew in site_crews(db, week.entity_id).await? {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, committed_start, committed_end FROM schedule_commitments \
                  WHERE status = 'committed' AND frozen_at IS NOT NULL AND assigned_team_id = ? \
                    AND julianday(committed_start) < julianday(?) AND julianday(committed_end) > julianday(?)",
                [crew.team_id.into(), end.clone().into(), start.clone().into()],
            ))
            .await?;
        for r in rows {
            let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
            let slot_start: String = r
                .try_get("", "committed_start")
                .map_err(|e| decode_err("committed_start", e))?;
            let slot_end: String = r
                .try_get("", "committed_end")
                .map_err(|e| decode_err("committed_end", e))?;
            if frozen_schedule_touching(db, week.entity_id, &slot_start, &slot_end, Some(schedule.id))
                .await?
                .is_some()
            {
                continue;
            }
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE schedule_commitments SET frozen_at = NULL, row_version = row_version + 1, updated_at = ? \
                  WHERE id = ?",
                [now.into(), id.into()],
            ))
            .await?;
        }
    }
    Ok(())
}

/// Publishes a frozen week to its crews (plan.confirm, checked by the command): each crew is
/// notified of its commitments.
pub async fn publish_weekly_schedule(
    db: &DatabaseConnection,
    actor_id: i64,
    input: WeeklyScheduleRef,
) -> AppResult<WeeklyScheduleView> {
    let week = site_week(db, input.site_id, parse_day(&input.day)?).await?;
    {
        let tx = db.begin().await?;
        let schedule = load_schedule(&tx, input.site_id, week.week_start)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WeeklySchedule".to_string(),
                id: format!("{}:{}", input.site_id, week.week_start),
            })?;
        check_version(&schedule, input.expected_row_version)?;
        if schedule.status != STATUS_FROZEN {
            return Err(wrong_status(&schedule, "published"));
        }
        let now = now_z();
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE weekly_schedules SET status = 'published', published_by_id = ?, published_at = ?, \
                    row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [actor_id.into(), now.clone().into(), now.into(), schedule.id.into()],
        ))
        .await?;
        record_event(&tx, schedule.id, "published", Some(actor_id), None, None, None).await?;
        tx.commit().await?;
    }
    let start = rfc3339(local_midnight_utc(week.week_start)?);
    let end = rfc3339(local_midnight_utc(week.week_end + Duration::days(1))?);
    for crew in site_crews(db, input.site_id).await? {
        super::scheduling::notify_schedule_teams(
            db,
            actor_id,
            super::domain::NotifyTeamsInput {
                period_start: start.clone(),
                period_end: end.clone(),
                team_id: Some(crew.team_id),
                include_break_ins: Some(true),
            },
        )
        .await?;
    }
    get_weekly_schedule(db, input.site_id, &input.day).await
}

// ── Automatic freeze ─────────────────────────────────────────────────────────────────────

/// Freezes, for every planning site, the next week once the current week's last working day is
/// over (`now`), and catches up the current week if no desktop ran when its own freeze fell due.
/// Only a week that was never frozen (`baseline_no = 0`) freezes by itself: a week unfrozen
/// through the four-eyes procedure stays open until a planner freezes it again. Returns the weeks
/// frozen. A site that fails (no weekend, a calendar it cannot read) is skipped and logged — never
/// frozen on a guessed week, never stopping the other sites. "Frozen without approval" is told to
/// the planners by [`remind_unapproved_weeks`].
pub async fn auto_freeze_due(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<Vec<(i64, NaiveDate)>> {
    let mut sites: Vec<i64> = crews_with_sites(db).await?.into_iter().filter_map(|(_, s)| s).collect();
    sites.sort_unstable();
    sites.dedup();
    let mut frozen = Vec::new();
    for site in sites {
        match auto_freeze_site(db, site, now).await {
            Ok(weeks) => frozen.extend(weeks.into_iter().map(|w| (site, w))),
            Err(err) => tracing::warn!(site, error = %err, "planning auto-freeze: site skipped"),
        }
    }
    Ok(frozen)
}

async fn auto_freeze_site(db: &DatabaseConnection, site: i64, now: DateTime<Utc>) -> AppResult<Vec<NaiveDate>> {
    let today = now.with_timezone(&Local).date_naive();
    let current = site_week(db, site, today).await?;
    // The current week's own freeze fell due at the end of the previous week: already past.
    let mut due_weeks = vec![current.clone()];
    let next_due = local_midnight_utc(current.last_working_day.unwrap_or(current.week_end) + Duration::days(1))?;
    if now >= next_due {
        due_weeks.push(site_week(db, site, current.week_end + Duration::days(1)).await?);
    }
    let mut frozen = Vec::new();
    for week in due_weeks {
        if auto_freeze_week(db, &week).await? {
            frozen.push(week.week_start);
        }
    }
    Ok(frozen)
}

/// Freezes one due week if it was never frozen.
async fn auto_freeze_week(db: &DatabaseConnection, week: &SiteWeek) -> AppResult<bool> {
    let tx = db.begin().await?;
    let schedule = ensure_schedule(&tx, week).await?;
    if schedule.baseline_no != 0 || !matches!(schedule.status.as_str(), STATUS_DRAFT | STATUS_APPROVED) {
        tx.rollback().await?;
        return Ok(false);
    }
    freeze_core(&tx, &schedule, week, None).await?;
    tx.commit().await?;
    Ok(true)
}

// ── "Frozen without approval": recalled until approved ───────────────────────────────────

/// Notification category of a week frozen without approval (migration 212).
pub const WEEK_UNAPPROVED_CATEGORY: &str = "planning.week_unapproved";

/// Tells the planning confirm holders of each site about its weeks frozen without approval, and
/// recalls it: a notification someone read longer ago than the rule's window
/// (`dedupe_window_minutes`) is made unread again while the week is still not approved. Once
/// approved (here or on another device), the notifications are resolved. Each device does it from
/// its own copy of the data, with identities built from the schedule's sync id (sync capture off),
/// so a recipient's read / acknowledge applies everywhere. A week whose notification could not be
/// raised is logged and tried again at the next tick. Returns the weeks notified or recalled.
pub async fn remind_unapproved_weeks(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<u64> {
    resolve_approved_week_notifications(db).await?;
    let rule = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT dedupe_window_minutes FROM notification_rules WHERE category_code = ? AND is_active = 1 LIMIT 1",
            [WEEK_UNAPPROVED_CATEGORY.into()],
        ))
        .await?;
    let Some(rule) = rule else {
        return Ok(0);
    };
    let window: i64 = rule
        .try_get::<Option<i64>>("", "dedupe_window_minutes")
        .map_err(|e| decode_err("dedupe_window_minutes", e))?
        .unwrap_or(1440)
        .max(1);
    let recall_before = (now - Duration::minutes(window))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let weeks = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ws.id, ws.entity_sync_id, ws.entity_id, ws.week_start, n.name AS site_name \
               FROM weekly_schedules ws JOIN org_nodes n ON n.id = ws.entity_id \
              WHERE ws.frozen_without_approval = 1 AND ws.approved_at IS NULL \
                AND ws.status IN ('frozen', 'published')"
                .to_string(),
        ))
        .await?;
    let mut notified = 0;
    for w in weeks {
        let id: i64 = w.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let sid: String = w
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?;
        let site: i64 = w.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?;
        let week_start: String = w.try_get("", "week_start").map_err(|e| decode_err("week_start", e))?;
        let site_name: String = w.try_get("", "site_name").map_err(|e| decode_err("site_name", e))?;
        let basis = format!("{WEEK_UNAPPROVED_CATEGORY}|{sid}");
        let event = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, resolved_at FROM notification_events WHERE entity_sync_id = ?",
                [seeded_entity_sync_id("notification_events", &basis).into()],
            ))
            .await?;
        let due = match event {
            None => true,
            Some(e) => {
                let event_id: i64 = e.try_get("", "id").map_err(|err| decode_err("id", err))?;
                let resolved: Option<String> = e
                    .try_get("", "resolved_at")
                    .map_err(|err| decode_err("resolved_at", err))?;
                resolved.is_some()
                    || db
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT 1 AS x FROM notifications WHERE notification_event_id = ? \
                               AND delivery_state = 'read' AND archived_at IS NULL AND read_at <= ?",
                            [event_id.into(), recall_before.clone().into()],
                        ))
                        .await?
                        .is_some()
            }
        };
        if !due {
            continue;
        }
        match notify_unapproved_week(db, id, site, &site_name, &week_start, &basis).await {
            Ok(()) => notified += 1,
            Err(err) => tracing::error!(
                weekly_schedule_id = id,
                error = %err,
                "planning: week frozen without approval not notified; retried at the next tick"
            ),
        }
    }
    Ok(notified)
}

async fn notify_unapproved_week(
    db: &DatabaseConnection,
    schedule_id: i64,
    site: i64,
    site_name: &str,
    week_start: &str,
    basis: &str,
) -> AppResult<()> {
    let tx = db.begin().await?;
    crate::sync::capture::begin_suppression(&tx).await?;
    crate::notifications::emitter::emit_device_event(
        &tx,
        crate::notifications::emitter::NotificationEventInput {
            source_module: "planning".to_string(),
            source_record_id: Some(schedule_id.to_string()),
            event_code: WEEK_UNAPPROVED_CATEGORY.to_string(),
            category_code: WEEK_UNAPPROVED_CATEGORY.to_string(),
            severity: "warning".to_string(),
            dedupe_key: Some(format!("planning.week-unapproved.{schedule_id}")),
            payload_json: Some(
                serde_json::json!({
                    "org_node_id": site,
                    "weekly_schedule_id": schedule_id,
                    "week_start": week_start,
                })
                .to_string(),
            ),
            title: format!("Semaine gelée sans approbation — {site_name} / Week frozen without approval — {site_name}"),
            body: Some(format!(
                "La semaine du {week_start} ({site_name}) a été gelée automatiquement sans approbation : approuvez-la. \
                 / The week of {week_start} ({site_name}) was frozen automatically without approval: approve it."
            )),
            action_url: Some("/planning".to_string()),
        },
        Some(basis),
    )
    .await?;
    crate::sync::capture::end_suppression(&tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Resolves the "frozen without approval" notifications of the weeks approved since (or no longer
/// frozen): they stay in the inbox, marked resolved, and are no longer recalled.
async fn resolve_approved_week_notifications(db: &DatabaseConnection) -> AppResult<()> {
    let tx = db.begin().await?;
    crate::sync::capture::begin_suppression(&tx).await?;
    let resolved = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notification_events SET resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
              WHERE category_code = ? AND resolved_at IS NULL AND NOT EXISTS ( \
                    SELECT 1 FROM weekly_schedules ws WHERE CAST(ws.id AS TEXT) = notification_events.source_record_id \
                       AND ws.frozen_without_approval = 1 AND ws.approved_at IS NULL \
                       AND ws.status IN ('frozen', 'published'))",
            [WEEK_UNAPPROVED_CATEGORY.into()],
        ))
        .await?
        .rows_affected();
    if resolved > 0 {
        crate::notifications::conditions::resolve_notifications_of_resolved_events(&tx).await?;
    }
    crate::sync::capture::end_suppression(&tx).await?;
    tx.commit().await?;
    Ok(())
}

// ── Sync: a received schedule never moves a week backwards ───────────────────────────────

/// Position of a schedule in its lifecycle: draft < approved < frozen < published < reopened
/// (draft / approved again after an unfreeze) < frozen again … Each freeze raises `baseline_no`,
/// so a reopened week (status draft / approved with `baseline_no` ≥ 1) comes after its freeze.
pub(crate) fn lifecycle_rank(status: &str, baseline_no: i64) -> i64 {
    match status {
        STATUS_FROZEN => 4 * baseline_no - 2,
        STATUS_PUBLISHED => 4 * baseline_no - 1,
        STATUS_APPROVED => 4 * baseline_no + 1,
        _ => 4 * baseline_no,
    }
}

/// A received weekly schedule (sync payload: `entity_id` the site's sync id, `week_start`,
/// `status`, `baseline_no`) that would move the local week backwards in its lifecycle — frozen
/// back to draft, or a later baseline back to an earlier one — is not applied silently: the reason
/// is returned and the sync apply records it as a conflict for a person to decide.
pub(crate) async fn inbound_lifecycle_conflict(
    conn: &impl ConnectionTrait,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<String>> {
    let text = |k: &str| payload.get(k).and_then(serde_json::Value::as_str);
    let (Some(status), Some(week_start)) = (text("status"), text("week_start")) else {
        return Ok(None);
    };
    let Some(baseline_no) = payload.get("baseline_no").and_then(serde_json::Value::as_i64) else {
        return Ok(None);
    };
    let site_filter = match payload.get("entity_id") {
        Some(serde_json::Value::String(sync_id)) => (
            "entity_id = (SELECT id FROM org_nodes WHERE entity_sync_id = ?)",
            sea_orm::Value::from(sync_id.clone()),
        ),
        Some(serde_json::Value::Number(n)) if n.as_i64().is_some() => {
            ("entity_id = ?", sea_orm::Value::from(n.as_i64()))
        }
        _ => return Ok(None),
    };
    let local = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT status, baseline_no FROM weekly_schedules WHERE {} AND week_start = ?",
                site_filter.0
            ),
            [site_filter.1, week_start.into()],
        ))
        .await?;
    let Some(local) = local else {
        return Ok(None);
    };
    let local_status: String = local.try_get("", "status").map_err(|e| decode_err("status", e))?;
    let local_baseline: i64 = local
        .try_get("", "baseline_no")
        .map_err(|e| decode_err("baseline_no", e))?;
    if lifecycle_rank(status, baseline_no) < lifecycle_rank(&local_status, local_baseline) {
        return Ok(Some(format!(
            "week of {week_start}: received '{status}' (baseline {baseline_no}) would move it back from \
             '{local_status}' (baseline {local_baseline}) here"
        )));
    }
    Ok(None)
}

/// A week of a crew's planning site that new or moved work may enter only as a break-in.
#[derive(Debug, Clone)]
pub struct FrozenWeek {
    pub week_start: String,
}

/// The week of the crew's planning site that `[start, end)` touches and that is frozen: frozen or
/// published, or due to freeze (its freeze time passed, it was never frozen and it is not over)
/// but not frozen yet — the automatic freeze has not run on this device (it waits for the first
/// sync, or the tick has not come). `None`: the crew has no site, the site has no planning week,
/// or every week the slot touches is open.
pub async fn frozen_week_for_slot(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    now: DateTime<Utc>,
) -> AppResult<Option<FrozenWeek>> {
    let node: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_node_id FROM teams WHERE id = ?",
            [team_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "primary_node_id").ok().flatten());
    let Some(node) = node else {
        return Ok(None);
    };
    let Some(site) = planning_site_of_node(db, node).await? else {
        return Ok(None);
    };
    if let Some(schedule) = frozen_schedule_touching(db, site, start, end, None).await? {
        return Ok(Some(FrozenWeek {
            week_start: schedule.week_start,
        }));
    }
    let (Some(slot_start), Some(slot_end)) = (
        crate::wo::shifts::planned_instant(start),
        crate::wo::shifts::planned_instant(end),
    ) else {
        return Ok(None);
    };
    let today = now.with_timezone(&Local).date_naive();
    let first = slot_start.with_timezone(&Local).date_naive();
    let last = (slot_end - Duration::seconds(1)).with_timezone(&Local).date_naive();
    let mut seen = BTreeSet::new();
    let mut day = first;
    while day <= last {
        let Ok(week) = site_week(db, site, day).await else {
            // No planning week for this site (no weekend): no weekly schedule to respect.
            return Ok(None);
        };
        day = week.week_end + Duration::days(1);
        if !seen.insert(week.week_start) || week.week_end < today {
            continue;
        }
        let never_frozen = load_schedule(db, site, week.week_start)
            .await?
            .is_none_or(|s| s.baseline_no == 0 && matches!(s.status.as_str(), STATUS_DRAFT | STATUS_APPROVED));
        if never_frozen && freeze_due_at(db, &week).await? <= now {
            return Ok(Some(FrozenWeek {
                week_start: week.week_start.to_string(),
            }));
        }
    }
    Ok(None)
}

/// The frozen or published schedule of `site` (other than `except`) whose week `[start, end)`
/// touches.
async fn frozen_schedule_touching(
    db: &impl ConnectionTrait,
    site: i64,
    start: &str,
    end: &str,
    except: Option<i64>,
) -> AppResult<Option<WeeklySchedule>> {
    let (Some(slot_start), Some(slot_end)) = (
        crate::wo::shifts::planned_instant(start),
        crate::wo::shifts::planned_instant(end),
    ) else {
        return Ok(None);
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {SCHEDULE_COLS} FROM weekly_schedules WHERE entity_id = ? AND status IN ('frozen', 'published')"),
            [site.into()],
        ))
        .await?;
    // Week bounds are local days: each candidate week is compared as instants.
    for r in rows {
        let schedule = map_schedule(&r)?;
        if Some(schedule.id) == except {
            continue;
        }
        let from = local_midnight_utc(parse_day(&schedule.week_start)?)?;
        let to = local_midnight_utc(parse_day(&schedule.week_end)? + Duration::days(1))?;
        if slot_start < to && slot_end > from {
            return Ok(Some(schedule));
        }
    }
    Ok(None)
}
