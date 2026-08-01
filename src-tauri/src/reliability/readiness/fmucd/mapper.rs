use anyhow::Result;
use chrono::{DateTime, Utc};

use super::config::{build_asset_key, FmucdMappingConfig};
use super::schema::{parse_fmucd_datetime, FmucdRow};
use crate::reliability::readiness::event::ReadinessEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapRowOutcome {
    Mapped,
    SkippedEmptyAsset,
    SkippedParseError,
}

/// Map one FMUCD row to a `ReadinessEvent` per mapping config.
pub fn map_row_to_event(
    config: &FmucdMappingConfig,
    row: &FmucdRow,
    row_index: i64,
) -> Result<(ReadinessEvent, MapRowOutcome)> {
    let asset_key = match build_asset_key(&config.asset_key, &row.fields) {
        Some(k) => k,
        None => return Ok((dummy_event(), MapRowOutcome::SkippedEmptyAsset)),
    };
    let ppm_upm = row.get("PPM/UPM").unwrap_or("").trim();
    let eligible = ppm_upm == config.eligibility.eligible_when_ppm_upm_equals;

    let start_raw = row.get(&config.dimensions.interval_start_field).unwrap_or("");
    let end_raw = row.get(&config.dimensions.interval_end_field).unwrap_or("");

    let start_parsed = parse_fmucd_datetime(start_raw).ok();
    let end_ts: Option<DateTime<Utc>> = if end_raw.trim().is_empty() {
        None
    } else {
        parse_fmucd_datetime(end_raw).ok()
    };

    let event_ts = start_parsed
        .or(end_ts)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap_or_else(Utc::now));

    let equipment_identified = config
        .dimensions
        .equipment_identified_requires
        .iter()
        .all(|f| row.get(f).map(|s| !s.trim().is_empty()).unwrap_or(false));

    let interval_complete = if config.mapping_mode.interval_any_date() {
        start_parsed.is_some() || end_ts.is_some()
    } else {
        start_parsed.is_some() && end_ts.is_some()
    };

    let mode_primary = row
        .get(&config.dimensions.failure_mode_primary_field)
        .unwrap_or("")
        .trim();
    let mode_fallback = row
        .get(&config.dimensions.failure_mode_fallback_field)
        .unwrap_or("")
        .trim();
    let corrective = row
        .get(&config.dimensions.corrective_evidence_field)
        .unwrap_or("")
        .trim();

    let failure_mode_coded = if config.mapping_mode.failure_mode_strict() {
        !mode_primary.is_empty()
    } else if config.mapping_mode.failure_mode_relaxed() {
        !mode_primary.is_empty() || !mode_fallback.is_empty() || !corrective.is_empty()
    } else {
        !mode_primary.is_empty() || !mode_fallback.is_empty()
    };

    let corrective_documented = end_ts.is_some() || !corrective.is_empty();

    let university_id = row.get("UniversityID").map(ToString::to_string);

    let event = ReadinessEvent {
        id: row_index,
        asset_key,
        university_id,
        event_ts,
        eligible,
        equipment_identified,
        interval_complete,
        failure_mode_coded,
        corrective_documented,
        eligible_flags_json: ReadinessEvent::eligible_flags_json(eligible),
        failure_mode_id: if failure_mode_coded { Some(row_index) } else { None },
        downtime_duration_hours: 0.0,
        active_repair_hours: 0.0,
    };

    Ok((event, MapRowOutcome::Mapped))
}

fn dummy_event() -> ReadinessEvent {
    ReadinessEvent {
        id: -1,
        asset_key: String::new(),
        university_id: None,
        event_ts: Utc::now(),
        eligible: false,
        equipment_identified: false,
        interval_complete: false,
        failure_mode_coded: false,
        corrective_documented: false,
        eligible_flags_json: ReadinessEvent::eligible_flags_json(false),
        failure_mode_id: None,
        downtime_duration_hours: 0.0,
        active_repair_hours: 0.0,
    }
}
