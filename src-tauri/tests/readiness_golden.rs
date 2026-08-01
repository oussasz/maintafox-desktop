use chrono::{DateTime, Utc};
use maintafox_lib::reliability::readiness::{
    evaluate_asset, evaluate_asset_dual, evaluate_iso14224_completeness, EvaluateAssetInput, EvaluationProfile,
    ReadinessEvent,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct IsoFixtureEvent {
    id: i64,
    equipment_identified: bool,
    interval_complete: bool,
    failure_mode_coded: bool,
    corrective_documented: bool,
}

#[derive(Deserialize)]
struct IsoExpected {
    event_count: i64,
    completeness_percent: f64,
    dim_equipment_id_pct: f64,
    dim_failure_interval_pct: f64,
    dim_failure_mode_pct: f64,
    dim_corrective_closure_pct: f64,
}

#[derive(Deserialize)]
struct IsoTolerance {
    abs: f64,
}

#[derive(Deserialize)]
struct IsoFixture {
    events: Vec<IsoFixtureEvent>,
    expected: IsoExpected,
    tolerance: IsoTolerance,
}

#[derive(Deserialize)]
struct Period {
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct ReadinessFixtureEvent {
    id: i64,
    failed_at: String,
    eligible: bool,
    equipment_identified: bool,
    interval_complete: bool,
    failure_mode_coded: bool,
    corrective_documented: bool,
}

#[derive(Deserialize)]
struct ReadinessExpected {
    data_quality_score: Option<f64>,
    badge: Option<String>,
    documentary_ready: Option<bool>,
    completeness_percent: Option<f64>,
    blocking_issue_codes: Option<Vec<String>>,
    documentary_badge: Option<String>,
    strict_badge: Option<String>,
    strict_ready: Option<bool>,
    strict_blocking_includes: Option<String>,
}

#[derive(Deserialize)]
struct RelTolerance {
    rel: f64,
}

#[derive(Deserialize)]
struct ReadinessFixture {
    asset_key: String,
    period: Period,
    exposure_hours: f64,
    min_sample_n: i64,
    profile: Option<String>,
    events: Vec<ReadinessFixtureEvent>,
    expected: ReadinessExpected,
    tolerance: RelTolerance,
}

fn parse_utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|d| d.with_timezone(&Utc))
        .expect("rfc3339")
}

fn to_readiness_events_iso(events: &[IsoFixtureEvent]) -> Vec<ReadinessEvent> {
    events
        .iter()
        .map(|e| ReadinessEvent {
            id: e.id,
            asset_key: "thesis-asset".into(),
            university_id: None,
            event_ts: Utc::now(),
            eligible: true,
            equipment_identified: e.equipment_identified,
            interval_complete: e.interval_complete,
            failure_mode_coded: e.failure_mode_coded,
            corrective_documented: e.corrective_documented,
            eligible_flags_json: ReadinessEvent::eligible_flags_json(true),
            failure_mode_id: if e.failure_mode_coded { Some(e.id) } else { None },
            downtime_duration_hours: 0.0,
            active_repair_hours: 0.0,
        })
        .collect()
}

fn to_readiness_events_rf(events: &[ReadinessFixtureEvent]) -> Vec<ReadinessEvent> {
    events
        .iter()
        .map(|e| ReadinessEvent {
            id: e.id,
            asset_key: "fixture".into(),
            university_id: None,
            event_ts: parse_utc(&e.failed_at),
            eligible: e.eligible,
            equipment_identified: e.equipment_identified,
            interval_complete: e.interval_complete,
            failure_mode_coded: e.failure_mode_coded,
            corrective_documented: e.corrective_documented,
            eligible_flags_json: ReadinessEvent::eligible_flags_json(e.eligible),
            failure_mode_id: if e.failure_mode_coded { Some(e.id) } else { None },
            downtime_duration_hours: 0.0,
            active_repair_hours: 0.0,
        })
        .collect()
}

fn assert_iso_fixture(json: &str) {
    let f: IsoFixture = serde_json::from_str(json).expect("iso fixture");
    let events = to_readiness_events_iso(&f.events);
    let r = evaluate_iso14224_completeness(&events);
    let tol = f.tolerance.abs;
    assert_eq!(r.event_count, f.expected.event_count);
    assert!((r.completeness_percent - f.expected.completeness_percent).abs() <= tol);
    assert!((r.dim_equipment_id_pct - f.expected.dim_equipment_id_pct).abs() <= tol);
    assert!((r.dim_failure_interval_pct - f.expected.dim_failure_interval_pct).abs() <= tol);
    assert!((r.dim_failure_mode_pct - f.expected.dim_failure_mode_pct).abs() <= tol);
    assert!((r.dim_corrective_closure_pct - f.expected.dim_corrective_closure_pct).abs() <= tol);
}

fn profile_from(s: Option<&str>) -> EvaluationProfile {
    match s.unwrap_or("cmms_documentary") {
        "strict_ram" => EvaluationProfile::StrictRam,
        _ => EvaluationProfile::CmmsDocumentary,
    }
}

fn assert_readiness_fixture(json: &str) {
    let f: ReadinessFixture = serde_json::from_str(json).expect("readiness fixture");
    let events = to_readiness_events_rf(&f.events);
    let p0 = parse_utc(&f.period.start);
    let p1 = parse_utc(&f.period.end);

    if f.expected.documentary_badge.is_some() || f.expected.strict_badge.is_some() {
        let base = EvaluateAssetInput {
            asset_key: f.asset_key.clone(),
            university_id: None,
            events: events.clone(),
            period_start: p0,
            period_end: p1,
            exposure_hours: f.exposure_hours,
            min_sample_n: f.min_sample_n,
            profile: EvaluationProfile::CmmsDocumentary,
            inspection_coverage_ratio: None,
        };
        let (doc, strict) = evaluate_asset_dual(&base);
        if let Some(ref b) = f.expected.documentary_badge {
            assert_eq!(doc.badge.badge, *b);
        }
        if let Some(ref b) = f.expected.strict_badge {
            assert_eq!(strict.badge.badge, *b);
        }
        if let Some(v) = f.expected.documentary_ready {
            assert_eq!(doc.documentary_ready, v);
        }
        if let Some(v) = f.expected.strict_ready {
            assert_eq!(strict.strict_ready, v);
        }
        if let Some(ref code) = f.expected.strict_blocking_includes {
            assert!(strict.badge.blocking_issue_codes.iter().any(|c| c == code));
        }
        return;
    }

    let input = EvaluateAssetInput {
        asset_key: f.asset_key,
        university_id: None,
        events,
        period_start: p0,
        period_end: p1,
        exposure_hours: f.exposure_hours,
        min_sample_n: f.min_sample_n,
        profile: profile_from(f.profile.as_deref()),
        inspection_coverage_ratio: None,
    };
    let r = evaluate_asset(&input);
    let tol = f.tolerance.rel;
    if let Some(exp) = f.expected.data_quality_score {
        let d = (r.data_quality_score - exp).abs();
        assert!(d <= tol * exp.max(1.0), "dq got {} want {}", r.data_quality_score, exp);
    }
    if let Some(ref b) = f.expected.badge {
        assert_eq!(r.badge.badge, *b);
    }
    if let Some(v) = f.expected.documentary_ready {
        assert_eq!(r.documentary_ready, v);
    }
    if let Some(exp) = f.expected.completeness_percent {
        assert!((r.completeness.completeness_percent - exp).abs() <= tol);
    }
    if let Some(ref codes) = f.expected.blocking_issue_codes {
        for c in codes {
            assert!(r.badge.blocking_issue_codes.contains(c), "missing blocking {c}");
        }
    }
}

#[test]
fn readiness_golden_iso_thesis_20() {
    assert_iso_fixture(include_str!("fixtures/rams_golden/v1/g_iso14224_thesis_20.json"));
}

#[test]
fn readiness_golden_badge_green() {
    assert_readiness_fixture(include_str!("fixtures/rams_golden/v1/g_readiness_badge_green.json"));
}

#[test]
fn readiness_golden_badge_red() {
    assert_readiness_fixture(include_str!("fixtures/rams_golden/v1/g_readiness_badge_red.json"));
}

#[test]
fn readiness_golden_dual_profile() {
    assert_readiness_fixture(include_str!("fixtures/rams_golden/v1/g_readiness_dual_profile.json"));
}

#[test]
fn readiness_golden_decomposition_waterfall() {
    use maintafox_lib::reliability::readiness::analysis::{
        assign_waterfall_reason, compute_decomposition, WaterfallReason,
    };
    use maintafox_lib::reliability::readiness::blocking::ReadinessIssue;
    use maintafox_lib::reliability::readiness::completeness::IsoCompletenessResult;

    fn report(
        eligible: i64,
        dq: f64,
        c: f64,
        dim_int: f64,
        doc_ready: bool,
        issues: Vec<ReadinessIssue>,
    ) -> maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport {
        maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport {
            asset_key: "a".into(),
            university_id: None,
            profile: EvaluationProfile::CmmsDocumentary,
            completeness: IsoCompletenessResult {
                event_count: eligible.max(1),
                completeness_percent: c,
                dim_equipment_id_pct: 100.0,
                dim_failure_interval_pct: dim_int,
                dim_failure_mode_pct: 100.0,
                dim_corrective_closure_pct: 100.0,
            },
            data_quality_score: dq,
            eligible_event_count: eligible,
            exposure_hours: 0.0,
            analysis_ready: false,
            badge: maintafox_lib::reliability::readiness::badge::BadgeResult {
                badge: "yellow".into(),
                blocking_issue_codes: vec![],
            },
            issues,
            documentary_ready: doc_ready,
            strict_ready: false,
        }
    }

    let r_dq = report(2, 0.2, 90.0, 90.0, false, vec![]);
    assert_eq!(assign_waterfall_reason(&r_dq), WaterfallReason::LowDq);

    let r_int = report(10, 1.0, 70.0, 40.0, false, vec![]);
    assert_eq!(assign_waterfall_reason(&r_int), WaterfallReason::IntervalGap);

    let reports = vec![report(10, 1.0, 95.0, 90.0, true, vec![]), r_dq, r_int];
    let out = compute_decomposition(&reports);
    let pct_sum: f64 = out.waterfall.iter().map(|b| b.pct_of_eligible).sum();
    assert!((pct_sum - 100.0).abs() < 0.01);
}
