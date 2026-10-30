//! Day-by-day history of the demo company.
//!
//! An agenda of timed steps drives several kinds of jobs side by side: breakdowns (DI -> OT ->
//! close-out, some under lockout), preventive occurrences, inspection rounds, purchases, stock
//! counts, plus recurring bookkeeping (running hours, PM generation, reliability snapshots). Each
//! step sets the simulated clock to its moment and calls the services a user would trigger at
//! that moment. Jobs still under way when the history reaches `today` stay open: that is the
//! company's current backlog.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use sea_orm::ConnectionTrait;
use serde_json::json;

use super::catalog::{self, Duty};
use super::master::{de, is_working_day, local, local_day, rfc, AssetInfo, CheckKind, Company};
use super::{column_i64, row_version, scalar_f64, scalar_i64, Ctx, GenerationOptions};
use crate::errors::AppResult;

pub async fn run(
    ctx: &mut Ctx<'_>,
    company: &Company,
    opts: &GenerationOptions,
    start_day: NaiveDate,
) -> AppResult<()> {
    let mut sim = Sim {
        rng: StdRng::seed_from_u64(opts.seed),
        queue: BTreeMap::new(),
        jobs: Vec::new(),
        seq: 0,
        handled_occurrences: BTreeSet::new(),
        pending_purchase: BTreeSet::new(),
    };

    // Recurring bookkeeping.
    let first_thursday = next_weekday(start_day, chrono::Weekday::Thu);
    sim.schedule(
        local(first_thursday, 22, 30),
        Job::Exposure {
            week_end: first_thursday,
            start_day,
        },
    );
    sim.schedule(local(start_day, 7, 0), Job::PmGeneration);
    sim.schedule(local(start_day, 6, 0), Job::Snapshots);
    for (i, _) in company.routes.iter().enumerate() {
        let first = next_weekday(start_day, chrono::Weekday::Sun) + Duration::days(i as i64 * 2);
        sim.schedule(local(first, 9, 0), Job::InspectionPlan { route: i });
    }
    for site in catalog::SITES {
        let first = next_weekday(start_day, chrono::Weekday::Wed);
        sim.schedule(local(first, 10, 0), Job::PurchaseReview { site: site.code });
        let count_day = next_weekday(start_day + Duration::days(80), chrono::Weekday::Tue);
        sim.schedule(local(count_day, 9, 0), Job::CountPlan { site: site.code });
    }

    // Breakdowns drawn per asset in catalogue order.
    let history_end = ctx.limit;
    let mut breakdowns = Vec::new();
    for (idx, asset) in company.assets.iter().enumerate() {
        // Every asset fails a few times a year at least (MTBF needs more than one event).
        let mean_gap_days = 365.0 / asset.failures_per_year.max(3.0);
        let mut day = start_day;
        loop {
            let gap: f64 = -mean_gap_days * (1.0 - sim.rng.gen::<f64>()).ln();
            day += Duration::days(gap.round().max(1.0) as i64);
            if !is_working_day(day) && sim.rng.gen::<f64>() < 0.85 {
                // Most breakdowns are noticed when the lines run; a few happen at the weekend.
                while !is_working_day(day) {
                    day += Duration::days(1);
                }
            }
            let minutes = sim.rng.gen_range(6 * 60..22 * 60);
            let at = local(day, 0, 0) + Duration::minutes(minutes);
            if at >= history_end {
                break;
            }
            breakdowns.push((at, idx));
        }
    }
    // This morning's breakdowns: still being handled when the history stops.
    let today_day = local_day(history_end);
    let critical: Vec<usize> = (0..company.assets.len())
        .filter(|i| company.assets[*i].criticality != "C" && !company.assets[*i].is_component)
        .collect();
    for (k, (h, m)) in [(7, 40), (9, 5), (10, 25)].into_iter().enumerate() {
        let at = local(today_day, h, m);
        if at < history_end && today_day > start_day {
            let pick = critical[(sim.rng.gen_range(0..critical.len()) + k) % critical.len()];
            breakdowns.push((at, pick));
        }
    }
    breakdowns.sort();
    for (at, idx) in breakdowns {
        let job = Corrective::new(&mut sim.rng, company, idx, at);
        let report_at = at + Duration::minutes(sim.rng.gen_range(10..40));
        sim.schedule(report_at, Job::Corrective(Box::new(job)));
    }

    // Planned work raised in the last weeks, prepared for the coming shutdown.
    let today = local_day(ctx.limit);
    for (i, _) in catalog::BACKLOG_PROJECTS.iter().enumerate() {
        let days_before = catalog::BACKLOG_PROJECTS[i].4;
        let mut day = today - Duration::days(days_before);
        while !is_working_day(day) {
            day += Duration::days(1);
        }
        if day >= start_day {
            sim.schedule(local(day, 10, 15), Job::Project { idx: i });
        }
    }

    // The agenda.
    while let Some((&key, _)) = sim.queue.iter().next() {
        if key.0 > ctx.limit {
            break;
        }
        let slot = sim.queue.remove(&key).expect("present");
        if let Err(e) = ctx.at(key.0) {
            ctx.warn(format!("{} ({}): {e}", sim.jobs[slot].label(), rfc(key.0)));
            continue;
        }
        let mut job = std::mem::replace(&mut sim.jobs[slot], Job::Done);
        let next = step(ctx, company, &mut sim, &mut job).await;
        match next {
            Ok(Some(at)) => {
                sim.jobs[slot] = job;
                sim.reschedule(at, slot);
            }
            Ok(None) => {}
            Err(e) => ctx.warn(format!("{} ({}): {e}", job.label(), rfc(key.0))),
        }
    }
    Ok(())
}

// ── Agenda ───────────────────────────────────────────────────────────────────────────────────

struct Sim {
    rng: StdRng,
    queue: BTreeMap<(DateTime<Utc>, u64), usize>,
    jobs: Vec<Job>,
    seq: u64,
    handled_occurrences: BTreeSet<i64>,
    /// (site, article id) with a purchase under way.
    pending_purchase: BTreeSet<(&'static str, i64)>,
}

impl Sim {
    fn schedule(&mut self, at: DateTime<Utc>, job: Job) {
        self.jobs.push(job);
        let slot = self.jobs.len() - 1;
        self.reschedule(at, slot);
    }

    fn reschedule(&mut self, at: DateTime<Utc>, slot: usize) {
        self.seq += 1;
        self.queue.insert((at, self.seq), slot);
    }

    fn chance(&mut self, p: f64) -> bool {
        self.rng.gen::<f64>() < p
    }

    fn minutes(&mut self, lo: i64, hi: i64) -> Duration {
        Duration::minutes(self.rng.gen_range(lo..=hi))
    }
}

enum Job {
    Done,
    Exposure { week_end: NaiveDate, start_day: NaiveDate },
    PmGeneration,
    Snapshots,
    InspectionPlan { route: usize },
    Inspection(Box<Inspection>),
    PurchaseReview { site: &'static str },
    Purchase(Box<Purchase>),
    CountPlan { site: &'static str },
    Corrective(Box<Corrective>),
    Pm(Box<PmJob>),
    Project { idx: usize },
}

impl Job {
    fn label(&self) -> String {
        match self {
            Job::Done => "terminé".into(),
            Job::Exposure { .. } => "heures de marche".into(),
            Job::PmGeneration => "génération préventif".into(),
            Job::Snapshots => "indicateurs fiabilité".into(),
            Job::InspectionPlan { .. } | Job::Inspection(_) => "ronde d'inspection".into(),
            Job::PurchaseReview { .. } | Job::Purchase(_) => "achat".into(),
            Job::CountPlan { .. } => "inventaire tournant".into(),
            Job::Corrective(c) => format!("panne {} étape {:?}", c.asset_code, c.stage),
            Job::Pm(p) => format!("préventif occurrence {} étape {:?}", p.occurrence_id, p.stage),
            Job::Project { idx } => format!("travaux planifiés {idx}"),
        }
    }
}

async fn step(ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim, job: &mut Job) -> AppResult<Option<DateTime<Utc>>> {
    match job {
        Job::Done => Ok(None),
        Job::Exposure { week_end, start_day } => exposure(ctx, company, week_end, *start_day).await,
        Job::PmGeneration => pm_generation(ctx, company, sim).await,
        Job::Snapshots => snapshots(ctx, company).await,
        Job::InspectionPlan { route } => inspection_plan(ctx, company, sim, *route).await,
        Job::Inspection(i) => i.step(ctx, company, sim).await,
        Job::PurchaseReview { site } => purchase_review(ctx, company, sim, site).await,
        Job::Purchase(p) => p.step(ctx, company, sim).await,
        Job::CountPlan { site } => stock_count(ctx, company, sim, site).await,
        Job::Corrective(c) => c.step(ctx, company, sim).await,
        Job::Pm(p) => p.step(ctx, company, sim).await,
        Job::Project { idx } => project(ctx, company, sim, *idx).await,
    }
}

fn next_weekday(from: NaiveDate, weekday: chrono::Weekday) -> NaiveDate {
    let mut d = from;
    while d.weekday() != weekday {
        d += Duration::days(1);
    }
    d
}

/// The next moment the day crew is at work (08:00–16:30, Sunday to Thursday), from `at`.
fn next_work_moment(at: DateTime<Utc>) -> DateTime<Utc> {
    let day = local_day(at);
    let local_minutes = {
        let l = at + Duration::hours(super::master::UTC_OFFSET_HOURS);
        i64::from(l.hour()) * 60 + i64::from(l.minute())
    };
    if is_working_day(day) && (8 * 60..16 * 60).contains(&local_minutes) {
        return at;
    }
    let mut d = if is_working_day(day) && local_minutes < 8 * 60 {
        day
    } else {
        day + Duration::days(1)
    };
    while !is_working_day(d) {
        d += Duration::days(1);
    }
    local(d, 8, 10)
}

// ── Recurring bookkeeping ────────────────────────────────────────────────────────────────────

/// Weekly running hours reported by production (Sunday to Thursday).
async fn exposure(
    ctx: &mut Ctx<'_>,
    company: &Company,
    week_end: &mut NaiveDate,
    start_day: NaiveDate,
) -> AppResult<Option<DateTime<Utc>>> {
    let week_start = (*week_end - Duration::days(4)).max(start_day);
    let days = (0..=(*week_end - week_start).num_days())
        .map(|k| week_start + Duration::days(k))
        .filter(|d| is_working_day(*d))
        .count() as f64;
    for asset in &company.assets {
        ctx.tick()?;
        crate::reliability::queries::upsert_runtime_exposure_log(
            ctx.db,
            de(json!({
                "id": null,
                "expected_row_version": null,
                "equipment_id": asset.id,
                "exposure_type": "hours",
                "value": asset.hours_per_day * days,
                "recorded_at": rfc(ctx.last),
                "source_type": "manual",
            }))?,
        )
        .await?;
        ctx.count("relevés d'heures de marche", 1);
    }
    *week_end += Duration::days(7);
    Ok(Some(local(*week_end, 22, 30)))
}

/// Weekly generation of the preventive occurrences of the next three weeks.
async fn pm_generation(ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim) -> AppResult<Option<DateTime<Utc>>> {
    crate::pm::queries::generate_pm_occurrences(ctx.db, de(json!({ "as_of": rfc(ctx.last), "horizon_days": 21 }))?)
        .await?;
    let rows = ctx
        .db
        .query_all(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT id, strftime('%Y-%m-%dT%H:%M:%SZ', due_at) AS due, \
                    strftime('%Y-%m-%dT%H:%M:%SZ', window_end_at) AS window_end, pm_plan_id \
             FROM pm_occurrences WHERE status = 'generated' ORDER BY due_at, id"
                .to_string(),
        ))
        .await?;
    for r in rows {
        let id: i64 = r.try_get("", "id")?;
        if !sim.handled_occurrences.insert(id) {
            continue;
        }
        let plan_id: i64 = r.try_get("", "pm_plan_id")?;
        let Some(plan) = company.pm_plans.iter().find(|p| p.plan_id == plan_id) else {
            continue;
        };
        let due = parse(&r.try_get::<String>("", "due")?)?;
        let window_end = match r.try_get::<Option<String>>("", "window_end")? {
            Some(s) => parse(&s)?,
            None => due + Duration::days(3),
        };
        let asset = company
            .assets
            .iter()
            .find(|a| a.code == plan.asset_code)
            .expect("plan asset");
        let fate = {
            let r: f64 = sim.rng.gen();
            if r < 0.86 {
                PmFate::OnTime
            } else if r < 0.96 {
                PmFate::Late
            } else {
                PmFate::Missed
            }
        };
        let plan_at = next_work_moment((due - Duration::days(4)).max(ctx.last) + sim.minutes(0, 120));
        let job = PmJob {
            occurrence_id: id,
            asset: asset.clone(),
            hours: plan.hours,
            due,
            window_end,
            fate,
            wo_id: 0,
            crew: Vec::new(),
            parts: Vec::new(),
            started: None,
            stage: PmStage::Plan,
        };
        sim.schedule(plan_at, Job::Pm(Box::new(job)));
    }
    ctx.count("générations préventives", 1);
    Ok(Some(ctx.last + Duration::days(7)))
}

/// Monthly reliability snapshot of every asset (12 rolling months), as the RAMS screen refresh.
async fn snapshots(ctx: &mut Ctx<'_>, company: &Company) -> AppResult<Option<DateTime<Utc>>> {
    for asset in &company.assets {
        ctx.tick()?;
        crate::reliability::queries::refresh_reliability_kpi_snapshot(
            ctx.db,
            de(json!({
                "equipment_id": asset.id,
                "period_start": rfc(ctx.last - Duration::days(365)),
                "period_end": rfc(ctx.last),
                "min_sample_n": null,
                "repeat_lookback_days": null,
            }))?,
        )
        .await?;
    }
    ctx.count("calculs d'indicateurs fiabilité", 1);
    let day = local_day(ctx.last);
    let next_month = if day.month() == 12 {
        NaiveDate::from_ymd_opt(day.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(day.year(), day.month() + 1, 1)
    }
    .expect("valid date");
    // The last refresh happens on the last day of the history.
    let next = local(next_month, 6, 0);
    if next > ctx.limit && ctx.last < ctx.limit - Duration::hours(2) {
        return Ok(Some(ctx.limit - Duration::hours(1)));
    }
    Ok(Some(next))
}

/// A date the app wrote; an unreadable one stops the step (never replaced by the wall clock).
fn parse(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("date illisible '{s}': {e}")))
}

// ── Breakdowns: DI -> OT -> close-out ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum CStage {
    Report,
    Triage,
    Resubmit,
    Screen,
    Approve,
    Convert,
    Plan,
    Permit,
    Start,
    Pause,
    Resume,
    Finish,
    RemoveIsolation,
    Verify,
    Close,
}

struct Corrective {
    asset_idx: usize,
    asset_code: &'static str,
    story: usize,
    failed_at: DateTime<Utc>,
    urgency: &'static str,
    loto: bool,
    returned: bool,
    rejected: bool,
    pause: bool,
    repair_hours: f64,
    crew: Vec<i64>,
    parts: Vec<(i64, f64)>,
    di_id: i64,
    wo_id: i64,
    permit_id: i64,
    isolation_id: i64,
    segment_id: i64,
    started: Option<DateTime<Utc>>,
    paused_at: Option<DateTime<Utc>>,
    resumed: Option<DateTime<Utc>>,
    /// Set when the DI comes from an inspection anomaly (already submitted).
    from_inspection: bool,
    stage: CStage,
}

impl Corrective {
    fn new(rng: &mut StdRng, company: &Company, asset_idx: usize, failed_at: DateTime<Utc>) -> Self {
        let asset = &company.assets[asset_idx];
        let stories: Vec<usize> = catalog::FAILURE_STORIES
            .iter()
            .enumerate()
            .filter(|(_, s)| s.classes.is_empty() || s.classes.contains(&asset.class))
            .map(|(i, _)| i)
            .collect();
        let story = stories[rng.gen_range(0..stories.len())];
        let r: f64 = rng.gen();
        let urgency = match asset.criticality {
            "A" if r < 0.2 => "emergency",
            "A" if r < 0.8 => "urgent",
            "A" => "high",
            "B" if r < 0.3 => "urgent",
            "B" if r < 0.7 => "high",
            "B" => "normal",
            _ if r < 0.3 => "high",
            _ if r < 0.85 => "normal",
            _ => "low",
        };
        let base = match asset.criticality {
            "A" => rng.gen_range(1.5..5.5),
            "B" => rng.gen_range(1.0..4.0),
            _ => rng.gen_range(0.75..2.5),
        };
        let repair_hours: f64 = ((base * 4.0_f64).round() / 4.0).max(0.75);
        let loto = asset.loto && rng.gen::<f64>() < 0.6;
        Self {
            asset_idx,
            asset_code: asset.code,
            story,
            failed_at,
            urgency,
            loto,
            returned: !loto && rng.gen::<f64>() < 0.05,
            rejected: rng.gen::<f64>() < 0.04,
            pause: !loto && rng.gen::<f64>() < 0.08,
            repair_hours,
            crew: Vec::new(),
            parts: Vec::new(),
            di_id: 0,
            wo_id: 0,
            permit_id: 0,
            isolation_id: 0,
            segment_id: 0,
            started: None,
            paused_at: None,
            resumed: None,
            from_inspection: false,
            stage: CStage::Report,
        }
    }

    fn quick(&self) -> bool {
        matches!(self.urgency, "emergency" | "urgent")
    }

    async fn step(&mut self, ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim) -> AppResult<Option<DateTime<Utc>>> {
        let asset = &company.assets[self.asset_idx];
        let site = asset.site;
        let story = &catalog::FAILURE_STORIES[self.story];
        let supervisor = company.user(site, Duty::Supervisor);
        let manager = company.user(site, Duty::Manager);
        let responsible = self.responsible(company);
        let db = ctx.db;
        match self.stage {
            CStage::Report => {
                let submitter = self.submitter(company);
                let symptom = company.refs.symptoms[story.symptom];
                let di = crate::di::queries::create_intervention_request(
                    db,
                    de(json!({
                        "equipment_id": asset.id,
                        "org_node_id": asset.node_id,
                        "title": format!("{} — {}", story.title, asset.code),
                        "description": format!("{} constaté(e) sur {} en cours de production.", story.title, asset.code),
                        "origin_type": if submitter == company.user(site, Duty::Production) { "operator" } else { "technician" },
                        "request_type": "repair",
                        "symptom_code_id": symptom,
                        "impact_level": match asset.criticality { "A" => "major", "B" => "minor", _ => "minor" },
                        "production_impact": asset.criticality != "C",
                        "safety_flag": self.loto,
                        "environmental_flag": false,
                        "quality_flag": false,
                        "reported_urgency": self.urgency,
                        "observed_at": rfc(self.failed_at),
                        "submitter_id": submitter,
                        "source_inspection_anomaly_id": null,
                    }))?,
                )
                .await?;
                self.di_id = di.id;
                ctx.count("demandes d'intervention", 1);
                self.stage = CStage::Triage;
                Ok(Some(self.reaction(ctx.last, sim)))
            }
            CStage::Triage => {
                let status = super::scalar_text(
                    db,
                    "SELECT status FROM intervention_requests WHERE id = ?",
                    vec![self.di_id.into()],
                )
                .await?
                .unwrap_or_default();
                // A DI raised from an inspection anomaly can already be under review.
                if matches!(status.as_str(), "submitted" | "returned_for_clarification") {
                    let rv = row_version(db, "intervention_requests", self.di_id).await?;
                    crate::di::queries::triage_submitted_di(
                        db,
                        de(json!({ "di_id": self.di_id, "expected_row_version": rv }))?,
                        supervisor,
                    )
                    .await?;
                }
                if self.returned {
                    ctx.tick()?;
                    let rv = row_version(db, "intervention_requests", self.di_id).await?;
                    crate::di::review::return_di_for_clarification(
                        db,
                        de(json!({
                            "di_id": self.di_id,
                            "actor_id": supervisor,
                            "expected_row_version": rv,
                            "reviewer_note": "Préciser le symptôme et la zone de la machine concernée.",
                            "reason_code": "INFO_INSUFFISANTE",
                        }))?,
                    )
                    .await?;
                    ctx.count("demandes renvoyées pour précision", 1);
                    self.stage = CStage::Resubmit;
                    return Ok(Some(ctx.last + sim.minutes(30, 150)));
                }
                self.stage = CStage::Screen;
                Ok(Some(ctx.last + sim.minutes(3, 15)))
            }
            CStage::Resubmit => {
                let submitter = scalar_i64(
                    db,
                    "SELECT submitter_id FROM intervention_requests WHERE id = ?",
                    vec![self.di_id.into()],
                )
                .await?
                .unwrap_or(supervisor);
                let rv = row_version(db, "intervention_requests", self.di_id).await?;
                crate::di::review::resubmit_returned_di(
                    db,
                    de(json!({
                        "di_id": self.di_id,
                        "actor_id": submitter,
                        "expected_row_version": rv,
                        "response_note": format!("{} côté entraînement, constaté au démarrage du poste.", story.title),
                    }))?,
                )
                .await?;
                self.returned = false;
                self.stage = CStage::Triage;
                Ok(Some(next_work_moment(ctx.last + sim.minutes(10, 60))))
            }
            CStage::Screen => {
                let rv = row_version(db, "intervention_requests", self.di_id).await?;
                let symptom = scalar_i64(
                    db,
                    "SELECT symptom_code_id FROM intervention_requests WHERE id = ?",
                    vec![self.di_id.into()],
                )
                .await?;
                if self.rejected {
                    crate::di::review::close_di(
                        db,
                        de(json!({
                            "di_id": self.di_id,
                            "actor_id": supervisor,
                            "expected_row_version": rv,
                            "disposition_code": "solved_immediately",
                            "notes": "Réarmement et nettoyage par le chef d'équipe, pas d'intervention nécessaire.",
                            "related_di_id": null,
                        }))?,
                    )
                    .await?;
                    ctx.count("demandes clôturées sans OT", 1);
                    return Ok(None);
                }
                crate::di::review::screen_di(
                    db,
                    de(json!({
                        "di_id": self.di_id,
                        "actor_id": supervisor,
                        "expected_row_version": rv,
                        "validated_urgency": self.urgency,
                        "classification_code_id": symptom,
                        "reviewer_note": null,
                    }))?,
                )
                .await?;
                self.stage = CStage::Approve;
                Ok(Some(ctx.last + sim.minutes(5, if self.quick() { 15 } else { 90 })))
            }
            CStage::Approve => {
                let rv = row_version(db, "intervention_requests", self.di_id).await?;
                crate::di::review::approve_di(
                    db,
                    de(json!({ "di_id": self.di_id, "actor_id": manager, "expected_row_version": rv, "notes": null }))?,
                )
                .await?;
                self.stage = CStage::Convert;
                Ok(Some(ctx.last + sim.minutes(3, 20)))
            }
            CStage::Convert => {
                let rv = row_version(db, "intervention_requests", self.di_id).await?;
                let result = crate::di::conversion::convert_di_to_work_order(
                    db,
                    de(json!({
                        "di_id": self.di_id,
                        "actor_id": supervisor,
                        "expected_row_version": rv,
                        "conversion_notes": null,
                    }))?,
                )
                .await?;
                self.wo_id = result.wo_id;
                // The plant's permit rules (or the DI safety flag) decide whether a lockout is needed.
                self.loto = scalar_i64(
                    db,
                    "SELECT requires_permit FROM work_orders WHERE id = ?",
                    vec![self.wo_id.into()],
                )
                .await?
                .unwrap_or(0)
                    != 0;
                if self.loto {
                    self.pause = false;
                }
                ctx.count("ordres de travail correctifs", 1);
                self.stage = CStage::Plan;
                Ok(Some(ctx.last + sim.minutes(2, 10)))
            }
            CStage::Plan => {
                let start = if self.quick() {
                    ctx.last + sim.minutes(20, 60)
                } else if self.urgency == "high" {
                    next_work_moment(ctx.last + sim.minutes(60, 240))
                } else {
                    let days = sim.rng.gen_range(1..=3);
                    next_work_moment(local(local_day(ctx.last) + Duration::days(days), 8, 0) + sim.minutes(0, 300))
                };
                let planner = company
                    .people_with(site, &[Duty::Planner])
                    .first()
                    .and_then(|p| p.user_id)
                    .unwrap_or(supervisor);
                // Crew: the responsible technician, sometimes a second one.
                let techs = company.people_with(site, &[Duty::Technician, Duty::LeadTechnician]);
                let lead = company
                    .people
                    .iter()
                    .find(|p| p.user_id == Some(responsible))
                    .map(|p| p.personnel_id)
                    .expect("responsible has a personnel card");
                self.crew = vec![lead];
                if sim.chance(0.3) {
                    let others: Vec<i64> = techs.iter().map(|p| p.personnel_id).filter(|id| *id != lead).collect();
                    self.crew.push(others[sim.rng.gen_range(0..others.len())]);
                }
                plan_and_ready(
                    ctx,
                    company,
                    self.wo_id,
                    planner,
                    responsible,
                    site,
                    start,
                    self.repair_hours,
                )
                .await?;
                // Spare parts for the repair, reserved from the site store.
                self.parts = pick_parts(ctx, company, sim, asset, story_uses_parts(self.story)).await?;
                for (article_id, qty) in &self.parts {
                    ctx.tick()?;
                    crate::wo::parts::add_planned_part(
                        db,
                        de(json!({
                            "wo_id": self.wo_id,
                            "article_id": article_id,
                            "quantity_planned": qty,
                            "stock_location_id": company.stores[site].1,
                            "auto_reserve": true,
                            "is_required": true,
                        }))?,
                    )
                    .await?;
                }
                if self.loto {
                    self.stage = CStage::Permit;
                    return Ok(Some(
                        (start - Duration::minutes(45)).max(ctx.last + Duration::minutes(2)),
                    ));
                }
                self.stage = CStage::Start;
                Ok(Some(start))
            }
            CStage::Permit => {
                let hse = company.user(site, Duty::Hse);
                let permit = crate::permit::queries::create_work_permit(
                    db,
                    de(json!({
                        "permit_type_id": company.refs.loto_permit_type,
                        "asset_id": asset.id,
                        "entity_id": null,
                        "linked_work_order_id": self.wo_id,
                        "expires_at": rfc(ctx.last + Duration::hours(12)),
                    }))?,
                    // Requested by the technician who does the work…
                    responsible,
                )
                .await?;
                self.permit_id = permit.id;
                ctx.count("permis de travail (consignation)", 1);
                // …submitted by them, approved by the HSE officer (four-eyes), issued by the team
                // leader (separation between who approves and who issues).
                for (status, actor) in [
                    ("pending_review", responsible),
                    ("approved", hse),
                    ("issued", supervisor),
                ] {
                    ctx.tick()?;
                    let rv = row_version(db, "work_permits", self.permit_id).await?;
                    crate::permit::queries::set_work_permit_status(
                        db,
                        de(json!({ "id": self.permit_id, "status": status, "expected_row_version": rv }))?,
                        actor,
                    )
                    .await?;
                }
                let hydraulic = matches!(asset.class, "PRESS" | "PUMP");
                ctx.tick()?;
                let isolation = crate::permit::queries::upsert_permit_isolation(
                    db,
                    de(json!({
                        "id": null,
                        "permit_id": self.permit_id,
                        "isolation_point": if hydraulic {
                            format!("Vanne d'isolement centrale hydraulique {}", asset.code)
                        } else {
                            format!("Sectionneur d'alimentation armoire {}", asset.code)
                        },
                        "energy_type_id": if hydraulic { company.refs.energy_hydraulic } else { company.refs.energy_electrical },
                        "isolation_method_id": if hydraulic { company.refs.method_valve } else { company.refs.method_breaker },
                        "lock_number": null,
                    }))?,
                )
                .await?;
                self.isolation_id = isolation.id;
                let lock = format!("CAD-{}-{:03}", site, sim.rng.gen_range(1..60));
                ctx.tick()?;
                let rv = row_version(db, "permit_isolations", self.isolation_id).await?;
                // Applied by the technician who does the work…
                crate::permit::queries::apply_permit_isolation(
                    db,
                    de(json!({ "isolation_id": self.isolation_id, "expected_row_version": rv, "lock_number": lock }))?,
                    responsible,
                )
                .await?;
                ctx.at(ctx.last + sim.minutes(5, 15))?;
                let rv = row_version(db, "permit_isolations", self.isolation_id).await?;
                // …verified by the HSE officer (a different person).
                crate::permit::queries::verify_permit_isolation(
                    db,
                    de(json!({ "isolation_id": self.isolation_id, "expected_row_version": rv, "lock_number": null }))?,
                    hse,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_permits", self.permit_id).await?;
                crate::permit::queries::set_work_permit_status(
                    db,
                    de(json!({ "id": self.permit_id, "status": "active", "expected_row_version": rv }))?,
                    supervisor,
                )
                .await?;
                self.stage = CStage::Start;
                Ok(Some(ctx.last + sim.minutes(5, 20)))
            }
            CStage::Start => {
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::start_wo(
                    db,
                    de(json!({ "wo_id": self.wo_id, "actor_id": responsible, "expected_row_version": rv }))?,
                )
                .await?;
                self.started = Some(ctx.last);
                ctx.tick()?;
                let seg = crate::wo::delay::open_downtime_segment(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "downtime_type": "full",
                        "comment": "Machine à l'arrêt pendant l'intervention.",
                        "actor_id": responsible,
                        "classification_code": if story.mode == "PANNE_ELEC" { "electrical" } else { "mechanical" },
                    }))?,
                )
                .await?;
                self.segment_id = seg.id;
                if self.pause {
                    self.stage = CStage::Pause;
                    return Ok(Some(ctx.last + sim.minutes(45, 90)));
                }
                self.stage = CStage::Finish;
                Ok(Some(ctx.last + Duration::minutes((self.repair_hours * 60.0) as i64)))
            }
            CStage::Pause => {
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::pause_wo(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "actor_id": responsible,
                        "expected_row_version": rv,
                        "delay_reason_id": company.refs.delay_waiting_part,
                        "comment": "Pièce non disponible au magasin, commande en urgence.",
                    }))?,
                )
                .await?;
                self.paused_at = Some(ctx.last);
                self.add_labor(ctx, self.started.expect("started"), ctx.last).await?;
                ctx.count("OT mis en attente", 1);
                self.stage = CStage::Resume;
                let next_day = local_day(ctx.last) + Duration::days(1);
                Ok(Some(next_work_moment(local(next_day, 9, 0))))
            }
            CStage::Resume => {
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::resume_wo(
                    db,
                    de(json!({ "wo_id": self.wo_id, "actor_id": responsible, "expected_row_version": rv }))?,
                )
                .await?;
                self.resumed = Some(ctx.last);
                let worked =
                    (self.paused_at.expect("paused") - self.started.expect("started")).num_minutes() as f64 / 60.0;
                let remaining = (self.repair_hours - worked).max(0.5);
                self.stage = CStage::Finish;
                Ok(Some(ctx.last + Duration::minutes((remaining * 60.0) as i64)))
            }
            CStage::Finish => {
                let from = self.resumed.or(self.started).expect("started");
                self.add_labor(ctx, from, ctx.last).await?;
                finish_parts(ctx, self.wo_id, responsible).await?;
                if !self.loto {
                    // Under a lockout the downtime ends when the removal of the last lock is
                    // verified (`permit::queries::verify_permit_isolation_removal`).
                    ctx.tick()?;
                    crate::wo::delay::close_downtime_segment(db, self.segment_id, Some(rfc(ctx.last))).await?;
                }
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::closeout::save_failure_coding(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "expected_row_version": rv,
                        "symptom_id": company.refs.symptoms[story.symptom],
                        "failure_mode_id": company.refs.modes[story.mode],
                        "failure_cause_id": company.refs.causes[story.cause],
                        "failure_effect_id": null,
                        "is_temporary_repair": false,
                        "is_permanent_repair": true,
                        "cause_not_determined": false,
                        "notes": story.root_cause,
                        "failure_mechanism_id": null,
                        "detection_method_id": null,
                    }))?,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::closeout::update_wo_rca(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "expected_row_version": rv,
                        "root_cause_summary": story.root_cause,
                        "corrective_action_summary": story.action,
                    }))?,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::complete_wo_mechanically(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "actor_id": responsible,
                        "expected_row_version": rv,
                        "actual_end": rfc(ctx.last),
                        "actual_duration_hours": self.repair_hours,
                        "conclusion": story.action,
                    }))?,
                )
                .await?;
                if self.loto {
                    self.stage = CStage::RemoveIsolation;
                    return Ok(Some(ctx.last + sim.minutes(10, 30)));
                }
                self.stage = CStage::Verify;
                Ok(Some(ctx.last + sim.minutes(20, 90)))
            }
            CStage::RemoveIsolation => {
                let hse = company.user(site, Duty::Hse);
                let rv = row_version(db, "permit_isolations", self.isolation_id).await?;
                // The technician who fitted the lock takes it off ("J'ai retiré mon cadenas")…
                crate::permit::queries::record_permit_isolation_removed(
                    db,
                    de(json!({ "isolation_id": self.isolation_id, "expected_row_version": rv, "lock_number": null }))?,
                    responsible,
                )
                .await?;
                ctx.at(ctx.last + sim.minutes(3, 10))?;
                let rv = row_version(db, "permit_isolations", self.isolation_id).await?;
                // …and the removal is verified by a third person: the team leader, neither the
                // technician who applied and removed the lock nor the HSE officer who verified it.
                crate::permit::queries::verify_permit_isolation_removal(
                    db,
                    de(json!({ "isolation_id": self.isolation_id, "expected_row_version": rv, "lock_number": null }))?,
                    supervisor,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_permits", self.permit_id).await?;
                crate::permit::queries::set_work_permit_status(
                    db,
                    de(json!({ "id": self.permit_id, "status": "closed", "expected_row_version": rv }))?,
                    hse,
                )
                .await?;
                ctx.tick()?;
                crate::permit::queries::append_permit_handover_log(
                    db,
                    de(json!({
                        "permit_id": self.permit_id,
                        "handed_from_role": "EXECUTANT",
                        "handed_to_role": "EMETTEUR",
                        "confirmation_note": "Travaux terminés, consignation levée, machine restituée à l'émetteur du permis.",
                        // The hand-back is received by the team leader who issued the permit: the
                        // receiver must hold the permit close right at the site.
                        "received_by_id": supervisor,
                    }))?,
                    responsible,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_permits", self.permit_id).await?;
                crate::permit::queries::set_work_permit_status(
                    db,
                    de(json!({ "id": self.permit_id, "status": "handed_back", "expected_row_version": rv }))?,
                    hse,
                )
                .await?;
                self.stage = CStage::Verify;
                Ok(Some(ctx.last + sim.minutes(15, 60)))
            }
            CStage::Verify => {
                verify(ctx, self.wo_id, supervisor).await?;
                self.stage = CStage::Close;
                Ok(Some(next_work_moment(ctx.last + sim.minutes(30, 600))))
            }
            CStage::Close => {
                close(ctx, self.wo_id, manager).await?;
                ctx.count("OT correctifs clôturés", 1);
                Ok(None)
            }
        }
    }

    fn submitter(&self, company: &Company) -> i64 {
        let asset = &company.assets[self.asset_idx];
        if asset.code.contains("-UTL") || matches!(asset.class, "COMPRESSOR" | "TRANSFORMER" | "HEAT_EXCHANGER") {
            company.user(asset.site, Duty::LeadTechnician)
        } else {
            company.user(asset.site, Duty::Production)
        }
    }

    /// Electrical stories go to the electrician lead, the others to the mechanical lead.
    fn responsible(&self, company: &Company) -> i64 {
        let asset = &company.assets[self.asset_idx];
        let leads = company.people_with(asset.site, &[Duty::LeadTechnician]);
        let electrical = catalog::FAILURE_STORIES[self.story].mode == "PANNE_ELEC";
        let pick = leads
            .iter()
            .find(|p| (p.skill == "ELEC_INDUSTRIAL") == electrical)
            .or(leads.first())
            .expect("lead technician");
        pick.user_id.expect("lead technician login")
    }

    /// When maintenance takes the request: on call for urgent work, otherwise in working hours.
    fn reaction(&self, now: DateTime<Utc>, sim: &mut Sim) -> DateTime<Utc> {
        if self.quick() {
            now + sim.minutes(10, 40)
        } else {
            next_work_moment(now + sim.minutes(15, 90))
        }
    }

    async fn add_labor(&self, ctx: &mut Ctx<'_>, from: DateTime<Utc>, to: DateTime<Utc>) -> AppResult<()> {
        for personnel in &self.crew {
            ctx.tick()?;
            crate::wo::labor::add_personnel_labor_entry(
                ctx.db,
                de(json!({
                    "wo_id": self.wo_id,
                    "personnel_id": personnel,
                    "is_overtime": !is_working_day(local_day(from)),
                    "skill_id": null,
                    "started_at": rfc(from),
                    "ended_at": rfc(to),
                    "hours_worked": null,
                    "hourly_rate": null,
                    "notes": null,
                }))?,
            )
            .await?;
        }
        Ok(())
    }
}

fn story_uses_parts(story: usize) -> bool {
    // Adjustments and resets need no part; the other stories replace one.
    !matches!(catalog::FAILURE_STORIES[story].cause, "operating_error") || story % 2 == 0
}

/// Spare parts a job on `asset` uses, when the site store has them.
async fn pick_parts(
    ctx: &mut Ctx<'_>,
    company: &Company,
    sim: &mut Sim,
    asset: &AssetInfo,
    wanted: bool,
) -> AppResult<Vec<(i64, f64)>> {
    if !wanted || sim.chance(0.25) {
        return Ok(Vec::new());
    }
    let candidates: Vec<&super::master::ArticleInfo> = company
        .articles
        .iter()
        .filter(|a| a.used_by.contains(&asset.class))
        .collect();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let n = if sim.chance(0.3) { 2 } else { 1 };
    let mut out: Vec<(i64, f64)> = Vec::new();
    for _ in 0..n {
        let a = candidates[sim.rng.gen_range(0..candidates.len())];
        if out.iter().any(|(id, _)| *id == a.id) {
            continue;
        }
        let qty = if a.unit_cost < 1_000.0 {
            f64::from(sim.rng.gen_range(5..=20))
        } else {
            1.0
        };
        let available = scalar_f64(
            ctx.db,
            "SELECT available_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![a.id.into(), company.stores[asset.site].1.into()],
        )
        .await?
        .unwrap_or(0.0);
        // Critical spares keep at least one unit on the shelf.
        let keep = if a.critical { 1.0 } else { 0.0 };
        if available - keep >= qty {
            out.push((a.id, qty));
        }
    }
    Ok(out)
}

/// Planned start/end, crew and readiness of a work order in `planning`.
#[allow(clippy::too_many_arguments)]
async fn plan_and_ready(
    ctx: &mut Ctx<'_>,
    company: &Company,
    wo_id: i64,
    planner: i64,
    responsible: i64,
    site: &str,
    start: DateTime<Utc>,
    hours: f64,
) -> AppResult<()> {
    let db = ctx.db;
    ctx.tick()?;
    let rv = row_version(db, "work_orders", wo_id).await?;
    crate::wo::execution::plan_wo(
        db,
        de(json!({
            "wo_id": wo_id,
            "actor_id": planner,
            "expected_row_version": rv,
            "planner_id": planner,
            "planned_start": rfc(start),
            "planned_end": rfc(start + Duration::minutes((hours * 60.0) as i64)),
            "shift": null,
            "shift_class_version_id": null,
            "clear_shift": false,
            "expected_duration_hours": hours,
            "planned_crew_size": null,
            "clear_crew_size": false,
            "urgency_code": null,
            "planned_downtime_hours": hours,
        }))?,
    )
    .await?;
    let team = company
        .people
        .iter()
        .find(|p| p.user_id == Some(responsible))
        .map(|p| p.team_id)
        .unwrap_or_else(|| company.person(site, Duty::Supervisor).team_id);
    ctx.tick()?;
    let rv = row_version(db, "work_orders", wo_id).await?;
    crate::wo::execution::assign_wo(
        db,
        de(json!({
            "wo_id": wo_id,
            "actor_id": planner,
            "expected_row_version": rv,
            "assigned_team_id": team,
            "primary_responsible_id": responsible,
            "scheduled_at": null,
            "override_reason": "Affectation selon la spécialité et la disponibilité de l'équipe.",
        }))?,
    )
    .await?;
    ctx.tick()?;
    let rv = row_version(db, "work_orders", wo_id).await?;
    crate::wo::workflow::actions::mark_ready::mark_wo_ready(
        db,
        de(json!({ "wo_id": wo_id, "actor_id": planner, "expected_row_version": rv }))?,
    )
    .await?;
    Ok(())
}

/// Records the use of every planned part line (stock issue), or that none was needed.
async fn finish_parts(ctx: &mut Ctx<'_>, wo_id: i64, actor: i64) -> AppResult<()> {
    let rows = ctx
        .db
        .query_all(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "SELECT id, quantity_planned FROM work_order_parts WHERE work_order_id = ? \
             AND COALESCE(consumption_status, 'planned') NOT IN ('used', 'not_used', 'cancelled') ORDER BY id",
            vec![wo_id.into()],
        ))
        .await?;
    if rows.is_empty() {
        ctx.tick()?;
        crate::wo::parts::confirm_no_parts_used(ctx.db, wo_id, actor).await?;
        return Ok(());
    }
    for r in rows {
        let id: i64 = r.try_get("", "id")?;
        let qty: f64 = r.try_get("", "quantity_planned")?;
        ctx.tick()?;
        crate::wo::parts::record_actual_usage(ctx.db, id, qty, None).await?;
        ctx.count("sorties de stock sur OT", 1);
    }
    Ok(())
}

async fn verify(ctx: &mut Ctx<'_>, wo_id: i64, verifier: i64) -> AppResult<()> {
    let rv = row_version(ctx.db, "work_orders", wo_id).await?;
    crate::wo::closeout::save_verification(
        ctx.db,
        de(json!({
            "wo_id": wo_id,
            "verified_by_id": verifier,
            "result": "pass",
            "return_to_service_confirmed": true,
            "recurrence_risk_level": "low",
            "notes": "Essai en charge concluant.",
            "expected_row_version": rv,
        }))?,
    )
    .await?;
    Ok(())
}

async fn close(ctx: &mut Ctx<'_>, wo_id: i64, closer: i64) -> AppResult<()> {
    let rv = row_version(ctx.db, "work_orders", wo_id).await?;
    crate::wo::closeout::close_wo(
        ctx.db,
        crate::wo::closeout::WoCloseInput {
            wo_id,
            actor_id: closer,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

// ── Preventive occurrences ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum PmFate {
    OnTime,
    Late,
    Missed,
}

#[derive(Debug, Clone, Copy)]
enum PmStage {
    Plan,
    Start,
    Finish,
    Miss,
}

struct PmJob {
    occurrence_id: i64,
    asset: AssetInfo,
    hours: f64,
    due: DateTime<Utc>,
    window_end: DateTime<Utc>,
    fate: PmFate,
    wo_id: i64,
    crew: Vec<i64>,
    parts: Vec<(i64, f64)>,
    started: Option<DateTime<Utc>>,
    stage: PmStage,
}

impl PmJob {
    async fn step(&mut self, ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim) -> AppResult<Option<DateTime<Utc>>> {
        let site = self.asset.site;
        let db = ctx.db;
        let planner = company
            .people_with(site, &[Duty::Planner])
            .first()
            .and_then(|p| p.user_id)
            .unwrap_or_else(|| company.user(site, Duty::Supervisor));
        let leads = company.people_with(site, &[Duty::LeadTechnician]);
        let lead = leads[(self.occurrence_id as usize) % leads.len()];
        let responsible = lead.user_id.expect("lead login");
        match self.stage {
            PmStage::Plan => {
                let rv = row_version(db, "pm_occurrences", self.occurrence_id).await?;
                crate::pm::queries::transition_pm_occurrence(
                    db,
                    de(json!({
                        "occurrence_id": self.occurrence_id,
                        "expected_row_version": rv,
                        "next_status": "ready_for_scheduling",
                        "reason_code": null,
                        "note": null,
                        "generate_work_order": true,
                        "work_order_type_id": null,
                        "actor_id": planner,
                    }))?,
                )
                .await?;
                self.wo_id = scalar_i64(
                    db,
                    "SELECT linked_work_order_id FROM pm_occurrences WHERE id = ?",
                    vec![self.occurrence_id.into()],
                )
                .await?
                .unwrap_or(0);
                ctx.count("ordres de travail préventifs", 1);
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::workflow::actions::submit::submit_wo(
                    db,
                    de(json!({ "wo_id": self.wo_id, "actor_id": planner, "expected_row_version": rv }))?,
                )
                .await?;
                let mut exec_day = local_day(self.due);
                match self.fate {
                    PmFate::Late => {
                        exec_day = local_day(self.window_end) + Duration::days(sim.rng.gen_range(1..=6));
                    }
                    PmFate::OnTime | PmFate::Missed => {}
                }
                while !is_working_day(exec_day) {
                    exec_day += Duration::days(1);
                }
                let start = local(exec_day, 8, 30) + sim.minutes(0, 240);
                // Planned on the due date; a late job is the one that slipped.
                let planned = local(local_day(self.due), 8, 30);
                plan_and_ready(
                    ctx,
                    company,
                    self.wo_id,
                    planner,
                    responsible,
                    site,
                    planned.max(ctx.last),
                    self.hours,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "pm_occurrences", self.occurrence_id).await?;
                crate::pm::queries::transition_pm_occurrence(
                    db,
                    de(json!({
                        "occurrence_id": self.occurrence_id,
                        "expected_row_version": rv,
                        "next_status": "scheduled",
                        "actor_id": planner,
                    }))?,
                )
                .await?;
                self.crew = vec![lead.personnel_id];
                if self.hours > 3.0 {
                    let techs = company.people_with(site, &[Duty::Technician]);
                    self.crew.push(techs[sim.rng.gen_range(0..techs.len())].personnel_id);
                }
                // Filters, oil or grease for the job.
                let consumables: Vec<&super::master::ArticleInfo> = company
                    .articles
                    .iter()
                    .filter(|a| {
                        (a.code.starts_with("FILT") || a.code.starts_with("HUILE") || a.code.starts_with("GRAISSE"))
                            && (a.used_by.is_empty() || a.used_by.contains(&self.asset.class))
                    })
                    .collect();
                if !consumables.is_empty() && sim.chance(0.6) {
                    let a = consumables[sim.rng.gen_range(0..consumables.len())];
                    let qty = if a.unit_cost < 1_000.0 {
                        f64::from(sim.rng.gen_range(2..=10))
                    } else {
                        1.0
                    };
                    let available = scalar_f64(
                        db,
                        "SELECT available_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
                        vec![a.id.into(), company.stores[site].1.into()],
                    )
                    .await?
                    .unwrap_or(0.0);
                    if available >= qty {
                        ctx.tick()?;
                        crate::wo::parts::add_planned_part(
                            db,
                            de(json!({
                                "wo_id": self.wo_id,
                                "article_id": a.id,
                                "quantity_planned": qty,
                                "stock_location_id": company.stores[site].1,
                                "auto_reserve": true,
                                "is_required": true,
                            }))?,
                        )
                        .await?;
                        self.parts.push((a.id, qty));
                    }
                }
                if matches!(self.fate, PmFate::Missed) {
                    self.stage = PmStage::Miss;
                    return Ok(Some(next_work_moment(self.window_end + Duration::hours(10))));
                }
                self.stage = PmStage::Start;
                Ok(Some(start))
            }
            PmStage::Start => {
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::start_wo(
                    db,
                    de(json!({ "wo_id": self.wo_id, "actor_id": responsible, "expected_row_version": rv }))?,
                )
                .await?;
                self.started = Some(ctx.last);
                self.stage = PmStage::Finish;
                Ok(Some(ctx.last + Duration::minutes((self.hours * 60.0) as i64)))
            }
            PmStage::Finish => {
                let tasks = column_i64(
                    db,
                    "SELECT id FROM work_order_tasks WHERE work_order_id = ? ORDER BY sequence_order, id",
                    vec![self.wo_id.into()],
                )
                .await?;
                let started = self.started.expect("started");
                let step_minutes = ((self.hours * 60.0) as i64 / (tasks.len().max(1) as i64)).max(5);
                for (k, task) in tasks.iter().enumerate() {
                    ctx.at(started + Duration::minutes(step_minutes * (k as i64 + 1)) - Duration::minutes(2))?;
                    crate::wo::tasks::complete_task(db, *task, responsible, "ok".into(), None).await?;
                }
                ctx.tick()?;
                for personnel in &self.crew {
                    ctx.tick()?;
                    crate::wo::labor::add_personnel_labor_entry(
                        db,
                        de(json!({
                            "wo_id": self.wo_id,
                            "personnel_id": personnel,
                            "is_overtime": false,
                            "started_at": rfc(started),
                            "ended_at": rfc(ctx.last),
                        }))?,
                    )
                    .await?;
                }
                finish_parts(ctx, self.wo_id, responsible).await?;
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::execution::complete_wo_mechanically(
                    db,
                    de(json!({
                        "wo_id": self.wo_id,
                        "actor_id": responsible,
                        "expected_row_version": rv,
                        "actual_end": rfc(ctx.last),
                        "actual_duration_hours": self.hours,
                        "conclusion": "Gamme préventive réalisée, aucune anomalie relevée.",
                    }))?,
                )
                .await?;
                ctx.at(ctx.last + sim.minutes(15, 45))?;
                verify(ctx, self.wo_id, company.user(site, Duty::Supervisor)).await?;
                ctx.at(ctx.last + sim.minutes(10, 40))?;
                close(ctx, self.wo_id, company.user(site, Duty::Manager)).await?;
                ctx.count("OT préventifs clôturés", 1);
                match self.fate {
                    PmFate::Late => ctx.count("préventifs réalisés en retard", 1),
                    _ => ctx.count("préventifs réalisés à temps", 1),
                }
                Ok(None)
            }
            PmStage::Miss => {
                let manager = company.user(site, Duty::Manager);
                let rv = row_version(db, "pm_occurrences", self.occurrence_id).await?;
                crate::pm::queries::transition_pm_occurrence(
                    db,
                    de(json!({
                        "occurrence_id": self.occurrence_id,
                        "expected_row_version": rv,
                        "next_status": "missed",
                        "reason_code": "capacity_constraint",
                        "note": "Équipe mobilisée sur des pannes, préventif non réalisé dans sa fenêtre.",
                        "actor_id": manager,
                    }))?,
                )
                .await?;
                ctx.tick()?;
                let rv = row_version(db, "work_orders", self.wo_id).await?;
                crate::wo::queries::cancel_work_order(
                    db,
                    de(json!({
                        "id": self.wo_id,
                        "expected_row_version": rv,
                        "actor_id": manager,
                        "cancel_reason": "Préventif non réalisé dans sa fenêtre (charge de dépannage).",
                    }))?,
                )
                .await?;
                ctx.count("préventifs manqués", 1);
                Ok(None)
            }
        }
    }
}

// ── Inspection rounds ────────────────────────────────────────────────────────────────────────

async fn inspection_plan(
    ctx: &mut Ctx<'_>,
    company: &Company,
    sim: &mut Sim,
    route: usize,
) -> AppResult<Option<DateTime<Utc>>> {
    let r = &company.routes[route];
    let inspector = company.person(r.site, Duty::LeadTechnician);
    let round = crate::inspection::queries::schedule_inspection_round(
        ctx.db,
        de(json!({
            "template_id": r.template_id,
            "scheduled_at": rfc(ctx.last + Duration::hours(1)),
            "assigned_to_id": inspector.personnel_id,
            "explicit_template_version_id": null,
            "assigned_team_id": inspector.team_id,
        }))?,
    )
    .await?;
    ctx.count("rondes d'inspection", 1);
    let at = ctx.last + Duration::hours(1) + sim.minutes(0, 30);
    sim.schedule(
        at,
        Job::Inspection(Box::new(Inspection {
            route,
            round_id: round.id,
            stage: 0,
        })),
    );
    Ok(Some(ctx.last + Duration::days(7)))
}

struct Inspection {
    route: usize,
    round_id: i64,
    stage: u8,
}

impl Inspection {
    async fn step(&mut self, ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim) -> AppResult<Option<DateTime<Utc>>> {
        let r = &company.routes[self.route];
        let db = ctx.db;
        let inspector = company.user(r.site, Duty::LeadTechnician);
        if self.stage == 0 {
            let rv = row_version(db, "inspection_rounds", self.round_id).await?;
            crate::inspection::runner::start_inspection_round(
                db,
                de(json!({ "round_id": self.round_id, "expected_row_version": rv, "note": null }))?,
            )
            .await?;
            for (checkpoint, kind, _) in &r.checkpoints {
                ctx.at(ctx.last + sim.minutes(3, 8))?;
                let anomaly = sim.chance(0.04);
                let (status, numeric, boolean, comment): (Option<&str>, Option<f64>, Option<bool>, Option<&str>) =
                    match kind {
                        CheckKind::Pressure => {
                            let v = if anomaly {
                                6.2
                            } else {
                                6.6 + f64::from(sim.rng.gen_range(0..8)) / 10.0
                            };
                            (
                                None,
                                Some(v),
                                None,
                                anomaly.then_some("Pression basse, fuite probable sur le réseau."),
                            )
                        }
                        CheckKind::Temperature => {
                            let v = if anomaly {
                                81.0
                            } else {
                                f64::from(sim.rng.gen_range(48..72))
                            };
                            (
                                None,
                                Some(v),
                                None,
                                anomaly.then_some("Température élevée, ventilation à contrôler."),
                            )
                        }
                        CheckKind::Boolean => (
                            None,
                            None,
                            Some(!anomaly),
                            anomaly.then_some("Suintement au presse-étoupe, bruit anormal."),
                        ),
                    };
                let rv = row_version(db, "inspection_rounds", self.round_id).await?;
                crate::inspection::results::record_inspection_result(
                    db,
                    de(json!({
                        "round_id": self.round_id,
                        "checkpoint_id": checkpoint,
                        "result_status": status,
                        "numeric_value": numeric,
                        "text_value": null,
                        "boolean_value": boolean,
                        "comment": comment,
                        "expected_row_version": rv,
                    }))?,
                    inspector,
                )
                .await?;
                ctx.count("relevés d'inspection", 1);
            }
            ctx.tick()?;
            let rv = row_version(db, "inspection_rounds", self.round_id).await?;
            crate::inspection::runner::complete_inspection_round(
                db,
                de(json!({ "round_id": self.round_id, "expected_row_version": rv, "note": null }))?,
            )
            .await?;
            let open = column_i64(
                db,
                "SELECT id FROM inspection_anomalies WHERE round_id = ? AND resolution_status = 'open' ORDER BY id",
                vec![self.round_id.into()],
            )
            .await?;
            if open.is_empty() {
                return Ok(None);
            }
            self.stage = 1;
            return Ok(Some(next_work_moment(ctx.last + sim.minutes(30, 120))));
        }
        // Each anomaly becomes a DI, then the round is reviewed by the team leader.
        let open = column_i64(
            db,
            "SELECT id FROM inspection_anomalies WHERE round_id = ? AND resolution_status = 'open' ORDER BY id",
            vec![self.round_id.into()],
        )
        .await?;
        for anomaly in open {
            ctx.tick()?;
            let rv = row_version(db, "inspection_anomalies", anomaly).await?;
            let (di, _) = crate::inspection::routing::route_inspection_anomaly_to_di(
                db,
                anomaly,
                rv,
                inspector,
                Some("Anomalie relevée en ronde d'inspection".into()),
                None,
            )
            .await?;
            ctx.count("anomalies d'inspection transmises en DI", 1);
            let asset_idx = company
                .assets
                .iter()
                .position(|a| a.id == di.equipment_id)
                .expect("routed asset");
            let mut job = Corrective::new(&mut sim.rng, company, asset_idx, ctx.last);
            job.di_id = di.id;
            job.from_inspection = true;
            job.urgency = "normal";
            job.loto = false;
            job.returned = false;
            job.rejected = false;
            job.stage = CStage::Triage;
            let at = next_work_moment(ctx.last + sim.minutes(20, 90));
            sim.schedule(at, Job::Corrective(Box::new(job)));
        }
        ctx.tick()?;
        let rv = row_version(db, "inspection_rounds", self.round_id).await?;
        crate::inspection::runner::review_inspection_round(
            db,
            de(
                json!({ "round_id": self.round_id, "expected_row_version": rv, "note": "Anomalies transmises en DI." }),
            )?,
            company.user(r.site, Duty::Supervisor),
        )
        .await?;
        Ok(None)
    }
}

// ── Purchases ────────────────────────────────────────────────────────────────────────────────

/// Weekly review of the store: articles under their reorder point are requested, approved,
/// ordered and received after the supplier's lead time.
async fn purchase_review(
    ctx: &mut Ctx<'_>,
    company: &Company,
    sim: &mut Sim,
    site: &'static str,
) -> AppResult<Option<DateTime<Utc>>> {
    let db = ctx.db;
    let location = company.stores[site].1;
    let mut lines = Vec::new();
    for a in &company.articles {
        if sim.pending_purchase.contains(&(site, a.id)) {
            continue;
        }
        let on_hand = scalar_f64(
            db,
            "SELECT on_hand_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![a.id.into(), location.into()],
        )
        .await?
        .unwrap_or(0.0);
        if on_hand <= a.reorder {
            lines.push((a.id, (a.max - on_hand).max(1.0), a.unit_cost));
        }
    }
    let next = Some(ctx.last + Duration::days(7));
    if lines.is_empty() {
        return Ok(next);
    }
    let storekeeper = company.user(site, Duty::Storekeeper);
    let manager = company.user(site, Duty::Manager);
    let requisition = crate::inventory::procurement::create_multi_line_requisition(
        db,
        de(json!({
            "lines": lines.iter().map(|(article, qty, cost)| json!({
                "article_id": article,
                "preferred_location_id": location,
                "requested_qty": qty,
                "estimated_unit_cost": cost,
            })).collect::<Vec<_>>(),
            "demand_source_type": "REORDER",
            "demand_source_ref": format!("Revue hebdomadaire magasin {site}"),
            "purchase_priority": "NORMAL",
            "reason": "Réapprovisionnement sous le point de commande.",
            "actor_id": storekeeper,
        }))?,
    )
    .await?;
    ctx.count("demandes d'achat", 1);
    for (status, actor) in [("SUBMITTED", storekeeper), ("APPROVED", manager)] {
        ctx.at(ctx.last + sim.minutes(5, 90))?;
        let rv = row_version(db, "procurement_requisitions", requisition.id).await?;
        crate::inventory::procurement::transition_procurement_requisition(
            db,
            de(json!({
                "requisition_id": requisition.id,
                "expected_row_version": rv,
                "next_status": status,
                "reason": null,
                "note": null,
                "actor_id": actor,
            }))?,
        )
        .await?;
    }
    let supplier = company.suppliers[sim.rng.gen_range(0..company.suppliers.len().min(5))];
    ctx.at(ctx.last + sim.minutes(30, 180))?;
    let po = crate::inventory::procurement::create_purchase_order_from_requisition(
        db,
        de(json!({
            "requisition_id": requisition.id,
            "supplier_id": supplier,
            "supplier_company_id": null,
            "actor_id": storekeeper,
            "currency_code": null,
        }))?,
    )
    .await?;
    ctx.count("commandes fournisseurs", 1);
    for (status, actor) in [("SUBMITTED", storekeeper), ("APPROVED", manager)] {
        ctx.at(ctx.last + sim.minutes(5, 60))?;
        let rv = row_version(db, "purchase_orders", po.id).await?;
        crate::inventory::procurement::transition_purchase_order(
            db,
            de(json!({
                "purchase_order_id": po.id,
                "expected_row_version": rv,
                "next_status": status,
                "reason": null,
                "note": null,
                "actor_id": actor,
            }))?,
        )
        .await?;
    }
    for (article, _, _) in &lines {
        sim.pending_purchase.insert((site, *article));
    }
    let lead = scalar_i64(
        db,
        "SELECT default_lead_time_days FROM inventory_suppliers WHERE id = ?",
        vec![supplier.into()],
    )
    .await?
    .unwrap_or(7);
    let arrival =
        next_work_moment(local(local_day(ctx.last) + Duration::days(lead.max(2)), 9, 0) + sim.minutes(0, 360));
    sim.schedule(
        arrival,
        Job::Purchase(Box::new(Purchase {
            site,
            po_id: po.id,
            articles: lines.iter().map(|l| l.0).collect(),
        })),
    );
    Ok(next)
}

struct Purchase {
    site: &'static str,
    po_id: i64,
    articles: Vec<i64>,
}

impl Purchase {
    async fn step(&mut self, ctx: &mut Ctx<'_>, company: &Company, sim: &mut Sim) -> AppResult<Option<DateTime<Utc>>> {
        let db = ctx.db;
        let rows = db
            .query_all(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "SELECT id, article_id, ordered_qty FROM purchase_order_lines WHERE purchase_order_id = ? ORDER BY id",
                vec![self.po_id.into()],
            ))
            .await?;
        let location = company.stores[self.site].1;
        let mut lines = Vec::new();
        for r in rows {
            let qty: f64 = r.try_get("", "ordered_qty")?;
            lines.push(json!({
                "po_line_id": r.try_get::<i64>("", "id")?,
                "article_id": r.try_get::<i64>("", "article_id")?,
                "location_id": location,
                "received_qty": qty,
                "accepted_qty": qty,
                "rejected_qty": 0.0,
                "rejection_reason": null,
                "lots": null,
                "entered_unit_value_id": null,
            }));
        }
        crate::inventory::procurement::receive_purchase_order_goods(
            db,
            de(json!({
                "purchase_order_id": self.po_id,
                "lines": lines,
                "actor_id": company.user(self.site, Duty::Storekeeper),
                "fulfillment_action": null,
                "exchange_rate": null,
                "posting_date": local_day(ctx.last).format("%Y-%m-%d").to_string(),
            }))?,
        )
        .await?;
        ctx.count("réceptions fournisseurs", 1);
        for a in &self.articles {
            sim.pending_purchase.remove(&(self.site, *a));
        }
        Ok(None)
    }
}

// ── Stock counts ─────────────────────────────────────────────────────────────────────────────

/// Quarterly cycle count of the site store, counted by the storekeeper and approved by the
/// maintenance manager.
async fn stock_count(
    ctx: &mut Ctx<'_>,
    company: &Company,
    sim: &mut Sim,
    site: &'static str,
) -> AppResult<Option<DateTime<Utc>>> {
    let db = ctx.db;
    let (warehouse, location) = company.stores[site];
    let storekeeper = company.user(site, Duty::Storekeeper);
    let manager = company.user(site, Duty::Manager);
    let session = crate::inventory::controls::create_count_session(
        db,
        de(json!({ "warehouse_id": warehouse, "location_id": location, "actor_id": storekeeper }))?,
    )
    .await?;
    ctx.count("inventaires tournants", 1);
    ctx.tick()?;
    let rv = row_version(db, "inventory_count_sessions", session.id).await?;
    crate::inventory::controls::transition_count_session(
        db,
        de(json!({ "session_id": session.id, "expected_row_version": rv, "next_status": "counting", "reason": null, "actor_id": storekeeper }))?,
    )
    .await?;
    for a in company.articles.iter().filter(|a| !a.critical) {
        let on_hand = scalar_f64(
            db,
            "SELECT on_hand_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![a.id.into(), location.into()],
        )
        .await?
        .unwrap_or(0.0);
        // A few small differences, always explained.
        let variance = if on_hand >= 2.0 && sim.chance(0.08) { -1.0 } else { 0.0 };
        ctx.at(ctx.last + sim.minutes(2, 6))?;
        crate::inventory::controls::upsert_count_line(
            db,
            de(json!({
                "session_id": session.id,
                "article_id": a.id,
                "location_id": location,
                "counted_qty": on_hand + variance,
                "variance_reason_code": if variance != 0.0 { Some("COUNT_ERROR") } else { None },
                "expected_row_version": null,
                // The storekeeper counts; the manager approves the variances (four-eyes).
                "actor_id": storekeeper,
            }))?,
        )
        .await?;
    }
    ctx.tick()?;
    let rv = row_version(db, "inventory_count_sessions", session.id).await?;
    crate::inventory::controls::transition_count_session(
        db,
        de(json!({ "session_id": session.id, "expected_row_version": rv, "next_status": "submitted", "reason": null, "actor_id": storekeeper }))?,
    )
    .await?;
    ctx.at(ctx.last + sim.minutes(60, 180))?;
    let to_approve = column_i64(
        db,
        "SELECT id FROM inventory_count_lines WHERE session_id = ? AND approval_required = 1 ORDER BY id",
        vec![session.id.into()],
    )
    .await?;
    for line in to_approve {
        ctx.tick()?;
        let rv = row_version(db, "inventory_count_lines", line).await?;
        crate::inventory::controls::approve_count_line(
            db,
            de(json!({
                "line_id": line,
                "expected_row_version": rv,
                "reviewer_id": manager,
                "reviewer_evidence": "Recomptage contradictoire effectué avec le magasinier.",
            }))?,
        )
        .await?;
    }
    ctx.tick()?;
    let rv = row_version(db, "inventory_count_sessions", session.id).await?;
    crate::inventory::controls::transition_count_session(
        db,
        de(json!({ "session_id": session.id, "expected_row_version": rv, "next_status": "approved", "reason": null, "actor_id": manager }))?,
    )
    .await?;
    ctx.tick()?;
    let rv = row_version(db, "inventory_count_sessions", session.id).await?;
    crate::inventory::controls::post_count_session(
        db,
        de(json!({ "session_id": session.id, "expected_row_version": rv, "actor_id": storekeeper }))?,
    )
    .await?;
    Ok(Some(next_work_moment(local(
        local_day(ctx.last) + Duration::days(91),
        9,
        0,
    ))))
}

// ── Planned overhauls and improvements (open at the end) ─────────────────────────────────────

async fn project(ctx: &mut Ctx<'_>, company: &Company, _sim: &mut Sim, idx: usize) -> AppResult<Option<DateTime<Utc>>> {
    let (code, type_code, title, hours, _) = catalog::BACKLOG_PROJECTS[idx];
    let asset = company.assets.iter().find(|a| a.code == code).expect("project asset");
    let site = asset.site;
    let planner = company
        .people_with(site, &[Duty::Planner])
        .first()
        .and_then(|p| p.user_id)
        .unwrap_or_else(|| company.user(site, Duty::Supervisor));
    let db = ctx.db;
    let wo = crate::wo::queries::create_work_order(
        db,
        de(json!({
            "type_code": type_code,
            "equipment_id": asset.id,
            "location_id": asset.node_id,
            "entity_id": asset.node_id,
            "planner_id": planner,
            "urgency_code": "normal",
            "title": title,
            "description": "Travaux préparés pour l'arrêt programmé de fin d'année.",
            "expected_duration_hours": hours,
            "creator_id": planner,
            "requires_permit": false,
        }))?,
    )
    .await?;
    ctx.count("travaux planifiés (révisions, améliorations)", 1);
    ctx.tick()?;
    let rv = row_version(db, "work_orders", wo.id).await?;
    crate::wo::workflow::actions::submit::submit_wo(
        db,
        de(json!({ "wo_id": wo.id, "actor_id": planner, "expected_row_version": rv }))?,
    )
    .await?;
    let start_day = local_day(ctx.limit) + Duration::days(7 + (idx as i64 * 4) % 35);
    let start = local(start_day, 8, 0);
    if idx % 3 == 2 {
        // Still being prepared: dates only.
        ctx.tick()?;
        let rv = row_version(db, "work_orders", wo.id).await?;
        crate::wo::execution::plan_wo(
            db,
            de(json!({
                "wo_id": wo.id,
                "actor_id": planner,
                "expected_row_version": rv,
                "planner_id": planner,
                "planned_start": rfc(start),
                "planned_end": rfc(start + Duration::days((hours / 16.0).ceil() as i64)),
                "clear_shift": false,
                "expected_duration_hours": hours,
                "clear_crew_size": false,
            }))?,
        )
        .await?;
    } else {
        let lead = company.people_with(site, &[Duty::LeadTechnician])[idx % 2]
            .user_id
            .expect("lead login");
        plan_and_ready(ctx, company, wo.id, planner, lead, site, start, hours).await?;
    }
    Ok(None)
}
