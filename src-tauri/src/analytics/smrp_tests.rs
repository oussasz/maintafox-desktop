//! Point-in-time baselines (`as_of`) and SMRP indicators (`smrp`) on hand-computed fixtures.
//!
//! Every timestamp is relative to one fixed `now`, so each expected value can be checked by hand
//! from the fixture comments.

use chrono::{DateTime, Duration, SecondsFormat, Timelike, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::analytics::as_of::{self, PmDueWindow};
use crate::analytics::smrp::{self, SmrpWindow};

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    // Fixture rows reference teams / candidates / reference values that the scenarios do not need.
    db.execute_unprepared("PRAGMA foreign_keys = OFF")
        .await
        .expect("fk off");
    db
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn id_of(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// A fixed "now" at 12:00 UTC so day arithmetic never crosses midnight by accident.
fn fixed_now() -> DateTime<Utc> {
    let n = Utc::now();
    n.with_hour(12)
        .and_then(|t| t.with_minute(0))
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .expect("noon")
        - Duration::days(1)
}

/// `'YYYY-MM-DDTHH:MM:SSZ'` (quoted) for `days` days and `hours` hours after `now`.
fn ts(now: DateTime<Utc>, days: i64, hours: i64) -> String {
    format!(
        "'{}'",
        (now + Duration::days(days) + Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

async fn status_id(db: &DatabaseConnection, code: &str) -> i64 {
    id_of(db, &format!("SELECT id FROM work_order_statuses WHERE code = '{code}'")).await
}

async fn type_id(db: &DatabaseConnection, code: &str) -> i64 {
    id_of(db, &format!("SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = '{code}'")).await
}

#[allow(clippy::too_many_arguments)]
async fn insert_wo(
    db: &DatabaseConnection,
    code: &str,
    type_code: &str,
    status: &str,
    created: &str,
    extra_cols: &str,
    extra_vals: &str,
) -> i64 {
    let (t, s) = (type_id(db, type_code).await, status_id(db, status).await);
    let (cols, vals) = if extra_cols.is_empty() {
        (String::new(), String::new())
    } else {
        (format!(", {extra_cols}"), format!(", {extra_vals}"))
    };
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, title, row_version, created_at, updated_at{cols}) \
             VALUES ('{code}', {t}, {s}, '{code}', 1, {created}, {created}{vals})"
        ),
    )
    .await;
    id_of(db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
}

// ── Baselines ──────────────────────────────────────────────────────────────────────────────

/// DI-A submitted −20 d, closed −5 d; DI-B submitted −50 d, still open; DI-C submitted −40 d,
/// converted −35 d.
#[tokio::test]
async fn di_open_counts_are_rebuilt_at_any_instant() {
    let db = setup().await;
    let now = fixed_now();
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    for (code, status, submitted, closed, converted) in [
        ("DI-A", "closed", ts(now, -20, 0), ts(now, -5, 0), "NULL".to_string()),
        (
            "DI-B",
            "submitted",
            ts(now, -50, 0),
            "NULL".to_string(),
            "NULL".to_string(),
        ),
        ("DI-C", "closed", ts(now, -40, 0), "NULL".to_string(), ts(now, -35, 0)),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO intervention_requests (code, equipment_id, org_node_id, status, title, description, \
                 origin_type, impact_level, reported_urgency, submitted_at, submitter_id, closed_at, converted_at, \
                 created_at, updated_at) VALUES ('{code}', 1, 1, '{status}', '{code}', 'd', 'operator', 'unknown', \
                 'medium', {submitted}, {admin}, {closed}, {converted}, {submitted}, {now_s})",
                now_s = ts(now, 0, 0)
            ),
        )
        .await;
    }
    let at = |d: i64| now + Duration::days(d);
    assert_eq!(as_of::di_open_at(&db, at(0), None, None).await.unwrap(), 1, "B");
    assert_eq!(as_of::di_open_at(&db, at(-10), None, None).await.unwrap(), 2, "A, B");
    assert_eq!(
        as_of::di_open_at(&db, at(-30), None, None).await.unwrap(),
        1,
        "B (C ended −35)"
    );
    assert_eq!(as_of::di_open_at(&db, at(-38), None, None).await.unwrap(), 2, "B, C");
    // Waiting more than 7 days at −10 d: A (submitted −20) and B.
    assert_eq!(as_of::di_open_at(&db, at(-10), Some(7), None).await.unwrap(), 2);
    // At −15 d with 7 days: A was submitted only 5 days earlier.
    assert_eq!(as_of::di_open_at(&db, at(-15), Some(7), None).await.unwrap(), 1);
    assert_eq!(
        as_of::di_open_at(&db, at(0), None, Some(2)).await.unwrap(),
        0,
        "other site"
    );
}

/// W1 corrective created −40 d, closed −20 d; W2 created −15 d, planning, 5 h, planned end −2 d,
/// logged in_progress at −8 d then on_hold at −3 d; W3 created −60 d, 3 h, cancelled −10 d.
#[tokio::test]
async fn wo_counts_backlog_overdue_and_in_progress_at_an_instant() {
    let db = setup().await;
    let now = fixed_now();
    insert_wo(
        &db,
        "W1",
        "corrective",
        "closed",
        &ts(now, -40, 0),
        "closed_at",
        &ts(now, -20, 0),
    )
    .await;
    let w2 = insert_wo(
        &db,
        "W2",
        "corrective",
        "planning",
        &ts(now, -15, 0),
        "expected_duration_hours, planned_end",
        &format!("5.0, {}", ts(now, -2, 0)),
    )
    .await;
    insert_wo(
        &db,
        "W3",
        "corrective",
        "cancelled",
        &ts(now, -60, 0),
        "expected_duration_hours, cancelled_at",
        &format!("3.0, {}", ts(now, -10, 0)),
    )
    .await;
    for (to, at) in [("in_progress", ts(now, -8, 0)), ("on_hold", ts(now, -3, 0))] {
        exec(
            &db,
            &format!(
                "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, acted_at) \
                 VALUES ({w2}, 'x', '{to}', 'test', {at})"
            ),
        )
        .await;
    }
    let at = |d: i64| now + Duration::days(d);
    assert_eq!(as_of::wo_open_at(&db, at(0), None).await.unwrap(), 1, "W2");
    assert_eq!(as_of::wo_open_at(&db, at(-30), None).await.unwrap(), 2, "W1, W3");
    assert_eq!(as_of::wo_open_at(&db, at(-12), None).await.unwrap(), 2, "W2, W3");
    assert!((as_of::wo_backlog_hours_at(&db, at(-12), None).await.unwrap() - 8.0).abs() < 1e-9);
    assert!((as_of::wo_backlog_hours_at(&db, at(0), None).await.unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(as_of::wo_overdue_at(&db, at(0), None).await.unwrap(), 1, "W2 past −2 d");
    assert_eq!(as_of::wo_overdue_at(&db, at(-12), None).await.unwrap(), 0);
    assert_eq!(as_of::wo_in_progress_at(&db, at(-5), None).await.unwrap(), 1);
    assert_eq!(
        as_of::wo_in_progress_at(&db, at(0), None).await.unwrap(),
        0,
        "on hold since −3 d"
    );
}

// ── PM ─────────────────────────────────────────────────────────────────────────────────────

async fn pm_plan(db: &DatabaseConnection) -> i64 {
    exec(
        db,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('sm-eq', 'EQ-SM', 'Pump', 'active_in_service', 1, datetime('now'), \
         datetime('now'))",
    )
    .await;
    let eq = id_of(db, "SELECT id FROM equipment WHERE asset_id_code = 'EQ-SM'").await;
    exec(
        db,
        &format!(
            "INSERT INTO pm_plans (code, title, asset_scope_type, asset_scope_id, strategy_type, lifecycle_status, \
             is_active, row_version, created_at, updated_at) VALUES ('PM-SM', 'PM', 'equipment', {eq}, 'fixed', \
             'active', 1, 1, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    id_of(db, "SELECT id FROM pm_plans WHERE code = 'PM-SM'").await
}

/// One definition with the PM page (`pm::compliance`):
/// P1 due −20 d, executed the same day → on time. P2 due −10 d, executed −7 d → late.
/// P3 due −5 d, never done, window (the due day) closed → due, not done.
/// P5 due −12 d with a window −13 d … −11 d, executed −14 d → early, not compliant.
/// P7 due −1 d with a window until +2 d, not done → still inside its window, not judged.
/// P4 due +2 d → not due yet. P6 due −15 d cancelled → excluded.
/// → 1 on time of 4 judged (P1, P2, P3, P5) = 25 %.
#[tokio::test]
async fn pm_compliance_and_pm_overdue_from_occurrences() {
    let db = setup().await;
    let now = fixed_now();
    let plan = pm_plan(&db).await;
    let created = ts(now, -40, 0);
    for (key, due, status, done, window) in [
        ("P1", ts(now, -20, 0), "completed", Some(ts(now, -20, 2)), None),
        ("P2", ts(now, -10, 0), "completed", Some(ts(now, -7, 0)), None),
        ("P3", ts(now, -5, 0), "forecasted", None, None),
        ("P4", ts(now, 2, 0), "forecasted", None, None),
        ("P6", ts(now, -15, 0), "cancelled", None, None),
        (
            "P5",
            ts(now, -12, 0),
            "completed",
            Some(ts(now, -14, 0)),
            Some((ts(now, -13, -12), ts(now, -11, 11))),
        ),
        (
            "P7",
            ts(now, -1, 0),
            "scheduled",
            None,
            Some((ts(now, -2, 0), ts(now, 2, 0))),
        ),
    ] {
        let updated = done.clone().unwrap_or_else(|| created.clone());
        let (start, end) = window.unwrap_or_else(|| ("NULL".to_string(), "NULL".to_string()));
        exec(
            &db,
            &format!(
                "INSERT INTO pm_occurrences (pm_plan_id, plan_version_id, due_basis, due_at, status, deferral_reason, created_at,                  updated_at, window_start_at, window_end_at)                  VALUES ({plan}, 1, 'calendar-{key}', {due}, '{status}', '{key}', {created}, {updated}, {start}, {end})"
            ),
        )
        .await;
        if let Some(done) = done {
            let occ = id_of(
                &db,
                &format!("SELECT id FROM pm_occurrences WHERE deferral_reason = '{key}'"),
            )
            .await;
            exec(
                &db,
                &format!(
                    "INSERT INTO pm_executions (pm_occurrence_id, execution_result, executed_at, created_at)                      VALUES ({occ}, 'completed_no_findings', {done}, {done})"
                ),
            )
            .await;
        }
    }
    let r = smrp::pm_compliance(&db, SmrpWindow::last_days(now, 30), now, None)
        .await
        .unwrap();
    assert_eq!((r.numerator, r.denominator), (1.0, 4.0), "P1 of P1, P2, P3, P5");
    assert!((r.value_pct.unwrap() - 25.0).abs() < 1e-9);
    // The PM page reads the same function over the same period.
    let page = crate::pm::compliance::compliance_between(
        &db,
        now - Duration::days(30),
        now,
        now,
        crate::pm::compliance::ComplianceScope::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            page.on_time_count,
            page.early_count,
            page.late_count,
            page.not_done_count,
            page.pending_count
        ),
        (1, 1, 1, 1, 1)
    );
    assert_eq!(page.compliance_pct, r.value_pct);
    let prev = smrp::pm_compliance(&db, SmrpWindow::last_days(now, 30).previous(), now, None)
        .await
        .unwrap();
    assert_eq!(prev.value_pct, None, "nothing due 30–60 days ago");

    let at = |d: i64| now + Duration::days(d);
    // Overdue is past the end of the tolerance window (the due date without one), the PM page's
    // rule: P3 (no window, due −5 d). P7 is due −1 d but its window runs to +2 d.
    assert_eq!(as_of::pm_overdue_at(&db, at(0), None).await.unwrap(), 1, "P3");
    // A day after its window closes P7 is overdue too, like P4 (due +2 d, no window).
    assert_eq!(as_of::pm_overdue_at(&db, at(3), None).await.unwrap(), 3, "P3, P4, P7");
    // The analytics count at now is the PM page's count (same expression, `PM_OVERDUE_SQL`).
    let page_rule = id_of(
        &db,
        &format!(
            "SELECT COUNT(*) FROM pm_occurrences po WHERE {}",
            crate::pm::queries::PM_OVERDUE_SQL.replace('?', &format!("'{}'", as_of::instant(now)))
        ),
    )
    .await;
    assert_eq!(page_rule, as_of::pm_overdue_at(&db, now, None).await.unwrap());
    assert_eq!(
        as_of::pm_overdue_at(&db, at(-8), None).await.unwrap(),
        1,
        "P2 (done −7 d)"
    );
    // At −12 d: P1 was done (−20 d), P5 done (−14 d) and due exactly then, P2, P3 and P7 not yet
    // due, P6 cancelled long before (its last change is its creation, −40 d).
    assert_eq!(as_of::pm_overdue_at(&db, at(-12), None).await.unwrap(), 0);
}

// ── Schedule compliance ────────────────────────────────────────────────────────────────────

/// Schedule compliance is measured against the frozen weekly baseline (`planning::kpis`; the full
/// computation is proved by `planning::tests::planning_kpis_measure_the_frozen_baseline`): live
/// commitments of weeks never frozen are not measured — no value, never a guess.
#[tokio::test]
async fn schedule_compliance_without_a_frozen_baseline_gives_no_value() {
    let db = setup().await;
    let now = fixed_now();
    let day = |d: i64, h: i64| ts(now, d, h - 12);
    let wo = insert_wo(
        &db,
        "S1",
        "preventive",
        "completed",
        &ts(now, -20, 0),
        "mechanically_completed_at",
        &day(-3, 11),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO schedule_commitments (schedule_candidate_id, source_type, source_id, \
             schedule_period_start, schedule_period_end, committed_start, committed_end, assigned_team_id) \
             VALUES (1, 'work_order', {wo}, date({start}), date({end}), {start}, {end}, 1)",
            start = day(-3, 8),
            end = day(-3, 12)
        ),
    )
    .await;
    let r = smrp::schedule_compliance(&db, SmrpWindow::last_days(now, 30), now, None)
        .await
        .unwrap();
    assert_eq!(r.denominator, 0.0, "{r:?}");
    assert_eq!(r.value_pct, None, "{r:?}");
}

// ── Work mix ───────────────────────────────────────────────────────────────────────────────

/// Completed in the last 30 days: corrective 5 h (labour 3 + 2), emergency 2 h (actual duration,
/// no labour), preventive 4 h (labour), inspection 1 h (actual duration). Previous window:
/// preventive 3 h closed −40 d.
#[tokio::test]
async fn work_mix_by_labour_hours() {
    let db = setup().await;
    let now = fixed_now();
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let closed = |d: i64| ts(now, d, 0);
    let corr = insert_wo(
        &db,
        "M1",
        "corrective",
        "closed",
        &ts(now, -12, 0),
        "closed_at",
        &closed(-10),
    )
    .await;
    insert_wo(
        &db,
        "M2",
        "emergency",
        "closed",
        &ts(now, -6, 0),
        "closed_at, actual_duration_hours",
        &format!("{}, 2.0", closed(-5)),
    )
    .await;
    let prev = insert_wo(
        &db,
        "M3",
        "preventive",
        "closed",
        &ts(now, -9, 0),
        "closed_at",
        &closed(-8),
    )
    .await;
    insert_wo(
        &db,
        "M4",
        "inspection",
        "completed",
        &ts(now, -4, 0),
        "mechanically_completed_at, actual_duration_hours",
        &format!("{}, 1.0", closed(-3)),
    )
    .await;
    let old = insert_wo(
        &db,
        "M5",
        "preventive",
        "closed",
        &ts(now, -45, 0),
        "closed_at",
        &closed(-40),
    )
    .await;
    insert_wo(
        &db,
        "M6",
        "corrective",
        "cancelled",
        &ts(now, -5, 0),
        "cancelled_at",
        &closed(-4),
    )
    .await;
    for (wo, h) in [(corr, 3.0), (corr, 2.0), (prev, 4.0), (old, 3.0)] {
        exec(
            &db,
            &format!(
                "INSERT INTO work_order_interveners (work_order_id, intervener_id, hours_worked) VALUES ({wo}, {admin}, {h})"
            ),
        )
        .await;
    }
    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.basis, "hours");
    assert_eq!(m.work_orders, 4, "the cancelled one is excluded");
    assert_eq!(
        (m.planned.current.numerator, m.planned.current.denominator),
        (5.0, 12.0)
    );
    assert_eq!(m.reactive.current.numerator, 7.0);
    assert_eq!(m.emergency.current.numerator, 2.0);
    assert_eq!(m.planned.previous.value_pct, Some(100.0));
    assert_eq!(m.without_nature, 0);

    // The OT list's "completed in the period" filter lists exactly the work orders the mix counted
    // (the link of the work-mix tiles).
    let window = SmrpWindow::last_days(now, 30);
    let listed = crate::wo::queries::list_work_orders(
        &db,
        crate::wo::queries::WoListFilter {
            completed_from: Some(crate::analytics::as_of::instant(window.from)),
            completed_to: Some(crate::analytics::as_of::instant(window.to)),
            limit: 200,
            offset: 0,
            ..Default::default()
        },
    )
    .await
    .expect("completed list");
    assert_eq!(listed.total, m.work_orders, "same population as the tile");
}

/// Choosing a site or any parent node counts the work of every node below it, as the equipment
/// list and the reliability fleet filter do for the same choice (audit AU-78).
#[tokio::test]
async fn the_organisation_filter_includes_the_sub_nodes() {
    let db = setup().await;
    let now = fixed_now();
    let parent = crate::auth::scope_tests::node(&db, "SMRP-PARENT", None).await;
    let child = crate::auth::scope_tests::node(&db, "SMRP-CHILD", Some(parent)).await;
    // The fixture helper leaves every path at "/": give the two nodes their real ancestry.
    exec(
        &db,
        &format!("UPDATE org_nodes SET ancestor_path = '/{parent}/' WHERE id = {parent}"),
    )
    .await;
    exec(
        &db,
        &format!("UPDATE org_nodes SET ancestor_path = '/{parent}/{child}/' WHERE id = {child}"),
    )
    .await;
    let wo = insert_wo(
        &db,
        "SUB1",
        "corrective",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -5, 0),
    )
    .await;
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = {child} WHERE id = {wo}"),
    )
    .await;
    let window = SmrpWindow::last_days(now, 30);
    let at_child = smrp::work_mix(&db, window, Some(child)).await.unwrap();
    let at_parent = smrp::work_mix(&db, window, Some(parent)).await.unwrap();
    assert_eq!(at_child.work_orders, 1);
    assert_eq!(
        at_parent.work_orders, 1,
        "the parent node counts its sub-node's work order"
    );
    let elsewhere = smrp::work_mix(&db, window, Some(child + 1000)).await.unwrap();
    assert_eq!(elsewhere.work_orders, 0);
}

/// A plant's own OT type without a maintenance nature (as migration 198 carried it over): its
/// work orders count in the total but in no share, and the figure says how many; the type is
/// listed for review with its use and flagged "to review" without any other change.
#[tokio::test]
async fn work_mix_counts_work_orders_whose_type_has_no_nature() {
    let db = setup().await;
    let now = fixed_now();
    exec(
        &db,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active, semantic_tag, metadata_json) \
         SELECT rs.id, 'PLANT_OWN', 'Type maison', 99, 1, NULL, '{\"origin\":\"migrated\"}' \
           FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'WORK.ORDER_TYPES' AND rs.status = 'published'",
    )
    .await;
    insert_wo(
        &db,
        "N1",
        "corrective",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -5, 0),
    )
    .await;
    insert_wo(
        &db,
        "N2",
        "PLANT_OWN",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -4, 0),
    )
    .await;
    insert_wo(&db, "N3", "PLANT_OWN", "planning", &ts(now, -2, 0), "", "").await;

    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.basis, "count");
    assert_eq!(m.work_orders, 2);
    assert_eq!(m.without_nature, 1, "N2 counts in no share");
    assert_eq!(m.reactive.current.numerator, 1.0);
    assert_eq!(m.planned.current.numerator, 0.0);

    let review = crate::wo::types::list_types_without_nature(&db).await.expect("review");
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].code, "PLANT_OWN");
    assert_eq!(review[0].work_order_count, 2);

    assert_eq!(crate::wo::types::flag_types_without_nature(&db).await.expect("flag"), 1);
    assert_eq!(
        crate::wo::types::flag_types_without_nature(&db).await.expect("again"),
        0,
        "idempotent"
    );
    let meta: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT metadata_json FROM reference_values WHERE code = 'PLANT_OWN'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index(0)
        .expect("meta");
    let meta: serde_json::Value = serde_json::from_str(&meta).expect("json");
    assert_eq!(meta["nature_review_required"], true);
    assert_eq!(meta["origin"], "migrated", "the rest is kept");
    assert_eq!(
        id_of(
            &db,
            "SELECT COUNT(*) FROM reference_values WHERE code = 'PLANT_OWN' AND semantic_tag IS NULL AND label = 'Type maison'"
        )
        .await,
        1,
        "nature and label not rewritten"
    );

    // Giving it a nature: the right, a reason and a known nature are required; a system type is
    // refused; once set, it is recorded and the type leaves the review list and joins the mix.
    use crate::wo::types::{nature_codes as nc, set_work_order_type_nature, SetWorkOrderTypeNatureInput};
    let code_of = |e: crate::errors::AppError| match e {
        crate::errors::AppError::OrgValidationFailed(i) => i.first().map(|x| x.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    };
    let own = id_of(&db, "SELECT id FROM reference_values WHERE code = 'PLANT_OWN'").await;
    let input = |nature: &str, reason: &str| SetWorkOrderTypeNatureInput {
        type_id: own,
        nature: nature.into(),
        reason: reason.into(),
    };
    exec(
        &db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES ('nt-user', 'clerk', 'Clerk', 'local', 1, 0, 0, \
         datetime('now'), datetime('now'))",
    )
    .await;
    let clerk = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'clerk'").await;
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let why = "Plant type is planned maintenance";
    assert_eq!(
        code_of(
            set_work_order_type_nature(&db, input("preventive_predetermined", why), clerk)
                .await
                .unwrap_err()
        ),
        nc::RIGHT_REQUIRED
    );
    assert_eq!(
        code_of(
            set_work_order_type_nature(&db, input("preventive_predetermined", "short"), admin)
                .await
                .unwrap_err()
        ),
        nc::REASON_REQUIRED
    );
    assert_eq!(
        code_of(
            set_work_order_type_nature(&db, input("planned", why), admin)
                .await
                .unwrap_err()
        ),
        nc::UNKNOWN_NATURE
    );
    let corrective = id_of(
        &db,
        "SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
         JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'",
    )
    .await;
    assert_eq!(
        code_of(
            set_work_order_type_nature(
                &db,
                SetWorkOrderTypeNatureInput {
                    type_id: corrective,
                    nature: "improvement".into(),
                    reason: why.into(),
                },
                admin,
            )
            .await
            .unwrap_err()
        ),
        nc::SYSTEM_TYPE
    );
    let set = set_work_order_type_nature(&db, input("preventive_predetermined", why), admin)
        .await
        .expect("set");
    assert_eq!(set.nature.as_deref(), Some("preventive_predetermined"));
    assert_eq!(
        code_of(
            set_work_order_type_nature(&db, input("improvement", why), admin)
                .await
                .unwrap_err()
        ),
        nc::ALREADY_SET,
        "set once, never overwritten"
    );
    assert!(crate::wo::types::list_types_without_nature(&db)
        .await
        .expect("review")
        .is_empty());
    assert_eq!(
        id_of(
            &db,
            "SELECT COUNT(*) FROM reference_values WHERE code = 'PLANT_OWN' \
             AND json_extract(metadata_json, '$.nature_review_required') IS NULL \
             AND json_extract(metadata_json, '$.origin') = 'migrated'"
        )
        .await,
        1,
        "review flag cleared, the rest kept"
    );
    assert_eq!(
        id_of(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'wo_type.nature.set' \
             AND json_extract(details_json, '$.reason') = 'Plant type is planned maintenance' \
             AND json_extract(details_json, '$.new_nature') = 'preventive_predetermined'"
        )
        .await,
        1,
        "audited with who, why and old -> new"
    );
    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.without_nature, 0);
    assert_eq!(m.planned.current.numerator, 1.0, "N2 is now planned work");
}

// ── Backlog ────────────────────────────────────────────────────────────────────────────────

/// Open: planning 10 h, ready 6 h, in progress 4 h, draft without estimate. Crew: P1 Mon–Fri
/// 08:00–16:00 (40 h), P2 three night shifts 22:00–06:00 (24 h), P3 no schedule, P4 terminated.
#[tokio::test]
async fn backlog_weeks_from_crew_schedules() {
    let db = setup().await;
    let now = fixed_now();
    for (code, status, hours) in [
        ("B1", "planning", "10.0"),
        ("B2", "ready", "6.0"),
        ("B3", "in_progress", "4.0"),
        ("B4", "draft", "NULL"),
    ] {
        insert_wo(
            &db,
            code,
            "corrective",
            status,
            &ts(now, -2, 0),
            "expected_duration_hours",
            hours,
        )
        .await;
    }
    // Two Classes horaires (weekly, continuous so the site's rest days do not apply, 100 %).
    for (rv, last_day, start, end) in [(9001, 5, "08:00", "16:00"), (9002, 3, "22:00", "06:00")] {
        exec(
            &db,
            &format!(
                "INSERT INTO reference_values (id, entity_sync_id, set_id, code, label, sort_order, is_active) \
                 SELECT {rv}, 'smrp-class-{rv}', rs.id, 'SMRP_{rv}', 'Classe {rv}', 99, 1 \
                 FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
                 ORDER BY rs.version_no DESC LIMIT 1"
            ),
        )
        .await;
        let days: Vec<serde_json::Value> = (1..=7)
            .map(|d| {
                serde_json::json!({
                    "cycle_day": d, "shift_start": start, "shift_end": end,
                    "break_minutes": 0, "is_rest_day": d > last_day,
                })
            })
            .collect();
        exec(
            &db,
            &format!(
                "INSERT INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, \
                 cycle_length_days, rotation_start_date, days_json, is_continuous, utilisation_percent, \
                 created_at, updated_at) \
                 VALUES ('smrp-ver-{rv}', {rv}, '2000-01-01', 7, NULL, '{}', 1, 100.0, datetime('now'), datetime('now'))",
                serde_json::Value::Array(days)
            ),
        )
        .await;
    }
    for (code, sched, status) in [
        ("P1", "9001", "active"),
        ("P2", "9002", "active"),
        ("P3", "NULL", "active"),
        ("P4", "9001", "terminated"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, employment_status, home_schedule_reference_value_id) \
                 VALUES ('{code}', '{code}', '{status}', {sched})"
            ),
        )
        .await;
    }
    let b = smrp::backlog(&db, SmrpWindow::last_days(now, 30), now, None)
        .await
        .unwrap();
    assert!((b.backlog_hours - 20.0).abs() < 1e-9, "{b:?}");
    assert!((b.ready_backlog_hours - 6.0).abs() < 1e-9);
    assert_eq!((b.open_work_orders, b.work_orders_without_estimate), (4, 1));
    // The backlog figure is "open (work to do)": the OT list's `open_only` filter lists it.
    assert_eq!(b.open_work_orders_rule, crate::wo::backlog::RULE_OPEN_WORK_TO_DO);
    let listed = crate::wo::queries::list_work_orders(
        &db,
        crate::wo::queries::WoListFilter {
            open_only: Some(true),
            limit: 200,
            offset: 0,
            ..Default::default()
        },
    )
    .await
    .expect("open list");
    assert_eq!(listed.total, b.open_work_orders);
    assert!((b.weekly_capacity_hours - 64.0).abs() < 1e-9, "{b:?}");
    assert_eq!((b.people_counted, b.people_without_schedule), (2, 1));
    assert!((b.weeks.unwrap() - 20.0 / 64.0).abs() < 1e-9);
    assert_eq!(b.previous_backlog_hours, 0.0, "nothing existed 30 days ago");
}

// ── Cost / RAV ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cost_to_rav_needs_replacement_values() {
    let db = setup().await;
    let now = fixed_now();
    let none = smrp::cost_to_rav(&db, SmrpWindow::last_days(now, 30), None)
        .await
        .unwrap();
    assert_eq!(none.value_pct, None, "no asset carries a replacement value");

    for (code, value) in [("RAV-1", "100000.0"), ("RAV-2", "NULL")] {
        exec(
            &db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
                 replacement_value, created_at, updated_at) VALUES ('sm-{code}', '{code}', '{code}', \
                 'active_in_service', 1, {value}, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }
    let e1 = id_of(&db, "SELECT id FROM equipment WHERE asset_id_code = 'RAV-1'").await;
    let e2 = id_of(&db, "SELECT id FROM equipment WHERE asset_id_code = 'RAV-2'").await;
    for (code, eq, cost) in [("C1", e1, "300.0"), ("C2", e2, "999.0")] {
        insert_wo(
            &db,
            code,
            "corrective",
            "closed",
            &ts(now, -10, 0),
            "equipment_id, closed_at, total_cost",
            &format!("{eq}, {}, {cost}", ts(now, -5, 0)),
        )
        .await;
    }
    let r = smrp::cost_to_rav(&db, SmrpWindow::last_days(now, 30), None)
        .await
        .unwrap();
    assert_eq!((r.assets_with_value, r.assets_total), (1, 2));
    assert_eq!(r.period_cost, 300.0, "cost on assets without a value is left out");
    // 300 over 30 days → 3650 a year on a 100 000 asset = 3.65 %.
    assert!((r.value_pct.unwrap() - 3.65).abs() < 1e-9, "{r:?}");
    assert_eq!(r.previous_value_pct, Some(0.0));
}

#[tokio::test]
async fn pm_due_windows_at_an_instant() {
    let db = setup().await;
    let now = fixed_now();
    let plan = pm_plan(&db).await;
    for (due, key) in [
        (ts(now, 0, 2), "TODAY"),
        (ts(now, 5, 0), "SOON"),
        (ts(now, -27, 0), "THEN"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO pm_occurrences (pm_plan_id, plan_version_id, due_basis, due_at, status, deferral_reason, created_at, \
                 updated_at) VALUES ({plan}, 1, 'calendar', {due}, 'forecasted', '{key}', {c}, {c})",
                c = ts(now, -60, 0)
            ),
        )
        .await;
    }
    assert_eq!(as_of::pm_due_at(&db, now, PmDueWindow::SameDay, None).await.unwrap(), 1);
    assert_eq!(
        as_of::pm_due_at(&db, now, PmDueWindow::NextDays(7), None)
            .await
            .unwrap(),
        2
    );
    let start = now - Duration::days(30);
    assert_eq!(
        as_of::pm_due_at(&db, start, PmDueWindow::NextDays(7), None)
            .await
            .unwrap(),
        1,
        "THEN"
    );
}

/// A plant type of the published OT type list, with an optional raw tag and metadata.
async fn plant_type(db: &DatabaseConnection, code: &str, tag: Option<&str>, meta: &str) -> i64 {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active, semantic_tag, metadata_json) \
         SELECT rs.id, ?, ?, 99, 1, ?, ? FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'WORK.ORDER_TYPES' AND rs.status = 'published'",
        [code.into(), code.into(), tag.map(str::to_string).into(), meta.into()],
    ))
    .await
    .expect("plant type");
    id_of(db, &format!("SELECT id FROM reference_values WHERE code = '{code}'")).await
}

fn refusal(e: crate::errors::AppError) -> String {
    match e {
        crate::errors::AppError::OrgValidationFailed(i) => i.first().map(|x| x.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// The nature is written by compare-and-set: a nature written by someone else between the read
/// and the write is never overwritten, the whole change is rolled back (no audit row).
#[tokio::test]
async fn a_nature_written_meanwhile_is_never_overwritten_and_nothing_is_recorded() {
    use crate::wo::types::{
        nature_codes as nc, set_work_order_type_nature, write_nature_if_unset, MaintenanceNature,
        SetWorkOrderTypeNatureInput, CONCURRENT_NATURE_WRITE_FOR,
    };
    let db = setup().await;
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let raced = plant_type(&db, "RACED", None, "{\"nature_review_required\":true}").await;

    *CONCURRENT_NATURE_WRITE_FOR.lock().expect("seam") = Some("RACED");
    let err = set_work_order_type_nature(
        &db,
        SetWorkOrderTypeNatureInput {
            type_id: raced,
            nature: "preventive_predetermined".into(),
            reason: "Plant type is planned maintenance".into(),
        },
        admin,
    )
    .await
    .expect_err("refused by the compare-and-set");
    *CONCURRENT_NATURE_WRITE_FOR.lock().expect("seam") = None;
    assert_eq!(refusal(err), nc::ALREADY_SET);
    assert_eq!(
        id_of(
            &db,
            &format!("SELECT COUNT(*) FROM reference_values WHERE id = {raced} AND semantic_tag IS NULL")
        )
        .await,
        1,
        "rolled back: neither nature written"
    );
    assert_eq!(
        id_of(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = 'wo_type.nature.set' AND target_id = '{}'",
                crate::wo::types::audit_type_ref(&db, raced).await.expect("type ref")
            )
        )
        .await,
        0,
        "a refused change leaves no audit row"
    );

    // The write itself refuses a type that has a nature by then.
    exec(
        &db,
        &format!("UPDATE reference_values SET semantic_tag = 'improvement' WHERE id = {raced}"),
    )
    .await;
    assert!(
        !write_nature_if_unset(&db, raced, MaintenanceNature::PreventivePredictive)
            .await
            .expect("write")
    );
    assert_eq!(
        id_of(
            &db,
            &format!("SELECT COUNT(*) FROM reference_values WHERE id = {raced} AND semantic_tag = 'improvement'")
        )
        .await,
        1
    );
}

/// Unknown type refused; a person (not an administrator) holding the reference-data manage right
/// may set it.
#[tokio::test]
async fn the_reference_manage_right_sets_a_nature_and_an_unknown_type_is_refused() {
    use crate::wo::types::{nature_codes as nc, set_work_order_type_nature, SetWorkOrderTypeNatureInput};
    let db = setup().await;
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let why = "Plant type is planned maintenance";
    let err = set_work_order_type_nature(
        &db,
        SetWorkOrderTypeNatureInput {
            type_id: 987_654,
            nature: "improvement".into(),
            reason: why.into(),
        },
        admin,
    )
    .await
    .expect_err("unknown type");
    assert_eq!(refusal(err), nc::TYPE_NOT_FOUND);

    let own = plant_type(&db, "REF_MANAGER_TYPE", None, "{}").await;
    let now = "2026-01-01T00:00:00Z";
    for sql in [
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) VALUES ('rm-user', 'refmgr', 'Ref manager', 'local', 1, 0, 0, \
         datetime('now'), datetime('now'))"
            .to_string(),
        format!(
            "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
             VALUES ('rm-role', 'ref-manager', 'custom', 'active', 0, '{now}', '{now}', 1)"
        ),
        format!(
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
             SELECT r.id, p.id, '{now}' FROM roles r, permissions p WHERE r.name = 'ref-manager' AND p.name = '{perm}'",
            perm = crate::rbac::permissions::REF_MANAGE
        ),
        format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at, row_version) \
             SELECT 'rm-usa', u.id, r.id, 'tenant', '{now}', '{now}', 1 FROM user_accounts u, roles r \
              WHERE u.username = 'refmgr' AND r.name = 'ref-manager'"
        ),
    ] {
        exec(&db, &sql).await;
    }
    let manager = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'refmgr'").await;
    let set = set_work_order_type_nature(
        &db,
        SetWorkOrderTypeNatureInput {
            type_id: own,
            nature: "corrective_deferred".into(),
            reason: why.into(),
        },
        manager,
    )
    .await
    .expect("ref.manage may set it");
    assert_eq!(set.nature.as_deref(), Some("corrective_deferred"));
    assert_eq!(
        id_of(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = 'wo_type.nature.set' \
                 AND target_id = '{}' AND actor_id = {manager}",
                crate::wo::types::audit_type_ref(&db, own).await.expect("type ref")
            )
        )
        .await,
        1
    );
}

/// One reading of the nature: a tag with spaces is the nature it names everywhere (not "without
/// nature", not to review); a flag left on a type that now has a nature is removed.
#[tokio::test]
async fn the_nature_is_read_trimmed_and_a_stale_review_flag_is_removed() {
    let db = setup().await;
    let now = fixed_now();
    let spaced = plant_type(&db, "SPACED", Some(" improvement "), "{}").await;
    let stale = plant_type(
        &db,
        "STALE_FLAG",
        Some("corrective_deferred"),
        "{\"origin\":\"migrated\",\"nature_review_required\":true}",
    )
    .await;
    insert_wo(
        &db,
        "T1",
        "SPACED",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -5, 0),
    )
    .await;

    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.without_nature, 0, "the spaced tag is a nature");
    assert_eq!(m.planned.current.numerator, 1.0);
    let review = crate::wo::types::list_types_without_nature(&db).await.expect("review");
    assert!(review.iter().all(|t| t.id != spaced && t.id != stale));

    assert_eq!(
        crate::wo::types::flag_types_without_nature(&db)
            .await
            .expect("sync flags"),
        1
    );
    assert_eq!(
        id_of(
            &db,
            &format!(
                "SELECT COUNT(*) FROM reference_values WHERE id = {stale} \
                 AND json_extract(metadata_json, '$.nature_review_required') IS NULL \
                 AND json_extract(metadata_json, '$.origin') = 'migrated'"
            )
        )
        .await,
        1,
        "stale flag removed, the rest kept"
    );
    assert_eq!(
        id_of(
            &db,
            &format!("SELECT COUNT(*) FROM reference_values WHERE id = {spaced} AND metadata_json = '{{}}'")
        )
        .await,
        1,
        "the spaced type is never flagged"
    );
    assert_eq!(
        crate::wo::types::flag_types_without_nature(&db).await.expect("again"),
        0
    );
}

/// A tag with a tab or a line break around it is the nature it names for every reader: the work
/// mix, the review list, the flag, the nature command and the type list (with its label).
#[tokio::test]
async fn a_tag_with_a_tab_or_a_line_break_is_its_nature_everywhere() {
    use crate::wo::types::{nature_codes as nc, set_work_order_type_nature, SetWorkOrderTypeNatureInput};
    let db = setup().await;
    let now = fixed_now();
    let admin = id_of(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let tab = plant_type(&db, "TABBED", Some("improvement\t"), "{}").await;
    let line = plant_type(&db, "LINED", Some("\r\ncorrective_deferred\n"), "{}").await;
    insert_wo(
        &db,
        "W1",
        "TABBED",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -5, 0),
    )
    .await;
    insert_wo(
        &db,
        "W2",
        "LINED",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -4, 0),
    )
    .await;

    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.without_nature, 0);
    assert_eq!((m.planned.current.numerator, m.reactive.current.numerator), (1.0, 1.0));
    let review = crate::wo::types::list_types_without_nature(&db).await.expect("review");
    assert!(
        review.iter().all(|t| t.id != tab && t.id != line),
        "not stuck on the review list"
    );
    assert_eq!(
        crate::wo::types::flag_types_without_nature(&db).await.expect("flags"),
        0
    );

    let err = set_work_order_type_nature(
        &db,
        SetWorkOrderTypeNatureInput {
            type_id: tab,
            nature: "corrective_deferred".into(),
            reason: "Plant type is planned maintenance".into(),
        },
        admin,
    )
    .await
    .expect_err("already a nature");
    assert_eq!(refusal(err), nc::ALREADY_SET);

    let types = crate::wo::types::list_work_order_types(&db).await.expect("types");
    let tabbed = types.iter().find(|t| t.id == tab).expect("tabbed");
    assert_eq!(tabbed.nature.as_deref(), Some("improvement"), "returned trimmed");
    assert!(tabbed.nature_label.is_some(), "the nature label is found");
    let lined = types.iter().find(|t| t.id == line).expect("lined");
    assert_eq!(lined.nature.as_deref(), Some("corrective_deferred"));
    assert!(lined.nature_label.is_some());

    // The other readers: W1 is improvement work, W2 corrective work without a failure mode.
    let eq = on_equipment(&db, "EQ-TRIM", &[("W1", 100.0), ("W2", 40.0)]).await;
    exec(
        &db,
        "INSERT INTO work_order_failure_details (work_order_id) SELECT id FROM work_orders WHERE code = 'W2'",
    )
    .await;
    // The padded type and the shipped deferred-corrective type share one backlog cell.
    let r = read_natures(&db, now, eq, &["LINED", "corrective"]).await;
    assert_eq!(
        r.detail_natures.get("W1"),
        Some(&Some("improvement".to_string())),
        "WO detail: trimmed"
    );
    assert_eq!(
        r.detail_natures.get("W2"),
        Some(&Some("corrective_deferred".to_string()))
    );
    assert!(r.detail_labels_found, "WO detail: the nature label is found");
    assert_eq!(r.list_natures, r.detail_natures, "WO list reads it like the detail");
    assert_eq!(
        (r.corrective_wos, r.corrective_cost, r.preventive_cost),
        (1, 40.0, 100.0),
        "asset cost"
    );
    assert!(r.quality_view_flags_missing_mode, "RAMS quality view");
    assert_eq!(r.missing_mode_wos, vec!["W2".to_string()]);
    assert_eq!(
        r.backlog_cell,
        Some(Some("corrective_deferred".to_string())),
        "supervision backlog: trimmed code"
    );
    assert_eq!(r.backlog_without_nature, 0);
}

/// A closed work order per `(code, total cost)` on a new equipment `asset_code`; its id.
async fn on_equipment(db: &DatabaseConnection, asset_code: &str, wos: &[(&str, f64)]) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('{asset_code}', '{asset_code}', 'Pump', 'active_in_service', 1, \
             datetime('now'), datetime('now'))"
        ),
    )
    .await;
    let eq = id_of(
        db,
        &format!("SELECT id FROM equipment WHERE asset_id_code = '{asset_code}'"),
    )
    .await;
    for (code, cost) in wos {
        exec(
            db,
            &format!("UPDATE work_orders SET equipment_id = {eq}, total_cost = {cost} WHERE code = '{code}'"),
        )
        .await;
    }
    eq
}

/// What the readers outside the work mix make of the natures of equipment `eq`'s work orders.
struct NatureReadings {
    detail_natures: std::collections::BTreeMap<String, Option<String>>,
    list_natures: std::collections::BTreeMap<String, Option<String>>,
    detail_labels_found: bool,
    corrective_wos: i64,
    corrective_cost: f64,
    preventive_cost: f64,
    quality_view_flags_missing_mode: bool,
    missing_mode_wos: Vec<String>,
    /// Supervision backlog: the nature of the one cell (no urgency) that all the OTs opened here
    /// (`OPEN-<type>` per type) fall in; `None` when they do not fall in one cell together.
    backlog_cell: Option<Option<String>>,
    backlog_without_nature: i64,
}

async fn read_natures(db: &DatabaseConnection, now: DateTime<Utc>, eq: i64, open_types: &[&str]) -> NatureReadings {
    let admin = id_of(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let page = crate::wo::queries::list_work_orders(
        db,
        crate::wo::queries::WoListFilter {
            equipment_id: Some(eq),
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .expect("WO list");
    let mut detail_natures = std::collections::BTreeMap::new();
    let mut detail_labels_found = true;
    for wo in &page.items {
        let detail = crate::wo::queries::get_work_order(db, wo.id)
            .await
            .expect("WO detail")
            .expect("row");
        detail_labels_found &= detail.nature_code.is_none() || detail.nature_label.is_some();
        detail_natures.insert(detail.code.clone(), detail.nature_code.clone());
    }
    let list_natures = page
        .items
        .iter()
        .map(|w| (w.code.clone(), w.nature_code.clone()))
        .collect();

    let costs = crate::finance::asset_costs::list_asset_cost_history(
        db,
        crate::finance::asset_costs::AssetCostFilter {
            only_with_cost: Some(false),
            limit: Some(2000),
            ..Default::default()
        },
    )
    .await
    .expect("asset costs");
    let row = costs.rows.iter().find(|r| r.equipment_id == eq).expect("asset row");

    let issues = crate::reliability::queries::list_ram_data_quality_issues(
        db,
        crate::reliability::domain::RamDataQualityIssuesFilter { equipment_id: Some(eq) },
        i32::try_from(admin).expect("user id"),
    )
    .await
    .expect("quality view");
    let missing = crate::reliability::queries::list_wos_missing_failure_mode(db, Some(eq), None)
        .await
        .expect("missing failure modes");
    let mut missing_mode_wos = Vec::new();
    for m in &missing {
        missing_mode_wos.push(
            crate::wo::queries::get_work_order(db, m.work_order_id)
                .await
                .expect("wo")
                .expect("row")
                .code,
        );
    }

    let before = crate::home::supervision::work_orders(db, now).await.expect("backlog");
    for t in open_types {
        insert_wo(db, &format!("OPEN-{t}"), t, "planning", &ts(now, -1, 0), "", "").await;
    }
    let after = crate::home::supervision::work_orders(db, now).await.expect("backlog");
    let count_of = |w: &crate::home::supervision::SupervisionWorkOrders, n: &Option<String>| {
        w.cells
            .iter()
            .filter(|c| &c.nature_code == n && c.urgency_code.is_none())
            .map(|c| c.count)
            .sum::<i64>()
    };
    let mut keys: Vec<_> = after
        .cells
        .iter()
        .map(|c| (c.nature_code.clone(), c.urgency_code.clone()))
        .collect();
    let cells = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), cells, "one backlog cell per nature and urgency");
    let grown: Vec<Option<String>> = keys
        .into_iter()
        .filter(|(_, u)| u.is_none())
        .map(|(n, _)| n)
        .filter(|n| count_of(&after, n) > count_of(&before, n))
        .collect();
    assert_eq!(grown.len(), 1, "the open OTs fall in one cell: {grown:?}");
    let backlog_growth = count_of(&after, &grown[0]) - count_of(&before, &grown[0]);
    let backlog_cell = (backlog_growth == i64::try_from(open_types.len()).expect("len")).then(|| grown[0].clone());

    NatureReadings {
        detail_natures,
        list_natures,
        detail_labels_found,
        corrective_wos: row.corrective_work_orders,
        corrective_cost: row.corrective_cost,
        preventive_cost: row.preventive_cost,
        quality_view_flags_missing_mode: issues
            .iter()
            .any(|i| i.equipment_id == eq && i.issue_code == "MISSING_FAILURE_MODE"),
        missing_mode_wos,
        backlog_cell,
        backlog_without_nature: after.without_nature - before.without_nature,
    }
}

/// The "for none" side: a tag padded with a no-break space or a vertical tab (characters the
/// readers do not strip) is a nature for no reader — outside every share of the work mix, on the
/// review list, without nature in the type list, the WO list and detail, the asset cost (preventive
/// side), the RAMS quality view and the supervision backlog.
#[tokio::test]
async fn a_tag_with_a_no_break_space_or_a_vertical_tab_is_no_nature_anywhere() {
    let db = setup().await;
    let now = fixed_now();
    let nbsp = plant_type(&db, "NBSP", Some("\u{a0}improvement"), "{}").await;
    let vtab = plant_type(&db, "VTAB", Some("corrective_deferred\u{0b}"), "{}").await;
    insert_wo(
        &db,
        "X1",
        "NBSP",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -5, 0),
    )
    .await;
    insert_wo(
        &db,
        "X2",
        "VTAB",
        "closed",
        &ts(now, -6, 0),
        "closed_at",
        &ts(now, -4, 0),
    )
    .await;

    let m = smrp::work_mix(&db, SmrpWindow::last_days(now, 30), None).await.unwrap();
    assert_eq!(m.without_nature, 2, "in no share");
    assert_eq!((m.planned.current.numerator, m.reactive.current.numerator), (0.0, 0.0));
    let review = crate::wo::types::list_types_without_nature(&db).await.expect("review");
    assert!(
        review.iter().any(|t| t.id == nbsp) && review.iter().any(|t| t.id == vtab),
        "to review"
    );
    let types = crate::wo::types::list_work_order_types(&db).await.expect("types");
    for id in [nbsp, vtab] {
        let t = types.iter().find(|t| t.id == id).expect("type");
        assert_eq!((t.nature.as_deref(), t.nature_label.as_deref()), (None, None));
    }

    let eq = on_equipment(&db, "EQ-NONE", &[("X1", 100.0), ("X2", 40.0)]).await;
    exec(
        &db,
        "INSERT INTO work_order_failure_details (work_order_id) SELECT id FROM work_orders WHERE code = 'X2'",
    )
    .await;
    let r = read_natures(&db, now, eq, &["NBSP", "VTAB"]).await;
    assert_eq!(r.detail_natures.get("X1"), Some(&None), "WO detail: no nature");
    assert_eq!(r.detail_natures.get("X2"), Some(&None));
    assert_eq!(r.list_natures, r.detail_natures);
    assert_eq!(
        (r.corrective_wos, r.corrective_cost, r.preventive_cost),
        (0, 0.0, 140.0),
        "asset cost: not corrective"
    );
    assert!(
        !r.quality_view_flags_missing_mode,
        "not corrective for the quality view"
    );
    assert!(r.missing_mode_wos.is_empty());
    assert_eq!(
        r.backlog_cell,
        Some(None),
        "supervision backlog: one cell without nature"
    );
    assert_eq!(r.backlog_without_nature, 2);
}
