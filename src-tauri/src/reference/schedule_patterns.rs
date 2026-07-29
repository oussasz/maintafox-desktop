//! Schedule pattern extension for ORG.SCHEDULE_CLASS reference values.
//!
//! Identity (code/label/active) lives in `reference_values`.
//! Weekday shift rows live in `schedule_details` keyed by `reference_value_id`.
//! Class-level attributes are stored in `reference_values.metadata_json`.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::{domains, sets, values};

pub const SCHEDULE_CLASS_DOMAIN_CODE: &str = "ORG.SCHEDULE_CLASS";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleDayPattern {
    pub day_of_week: i64,
    pub shift_start: String,
    pub shift_end: String,
    pub is_rest_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulePattern {
    pub reference_value_id: i64,
    pub code: String,
    pub label: String,
    pub is_active: bool,
    pub shift_pattern_code: String,
    pub is_continuous: bool,
    pub nominal_hours_per_day: f64,
    pub details: Vec<ScheduleDayPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertSchedulePatternPayload {
    pub reference_value_id: i64,
    pub shift_pattern_code: Option<String>,
    pub is_continuous: Option<bool>,
    pub nominal_hours_per_day: Option<f64>,
    pub details: Option<Vec<ScheduleDayPattern>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScheduleMetadata {
    #[serde(default)]
    shift_pattern_code: Option<String>,
    #[serde(default)]
    is_continuous: Option<bool>,
    #[serde(default)]
    nominal_hours_per_day: Option<f64>,
}

fn decode_err(field: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("decode {field}: {e}"))
}

fn parse_metadata(raw: Option<&str>, fallback_code: &str) -> ScheduleMetadata {
    let parsed = raw
        .and_then(|s| serde_json::from_str::<ScheduleMetadata>(s).ok())
        .unwrap_or(ScheduleMetadata {
            shift_pattern_code: None,
            is_continuous: None,
            nominal_hours_per_day: None,
        });
    ScheduleMetadata {
        shift_pattern_code: Some(
            parsed
                .shift_pattern_code
                .filter(|c| !c.trim().is_empty())
                .unwrap_or_else(|| fallback_code.to_string()),
        ),
        is_continuous: Some(parsed.is_continuous.unwrap_or(false)),
        nominal_hours_per_day: Some(parsed.nominal_hours_per_day.unwrap_or(8.0)),
    }
}

fn serialize_metadata(meta: &ScheduleMetadata) -> String {
    serde_json::to_string(&serde_json::json!({
        "shift_pattern_code": meta.shift_pattern_code.as_deref().unwrap_or(""),
        "is_continuous": meta.is_continuous.unwrap_or(false),
        "nominal_hours_per_day": meta.nominal_hours_per_day.unwrap_or(8.0),
    }))
    .unwrap_or_else(|_| {
        r#"{"shift_pattern_code":"","is_continuous":false,"nominal_hours_per_day":8.0}"#.into()
    })
}

fn validate_time(raw: &str) -> AppResult<()> {
    let ok = chrono::NaiveTime::parse_from_str(raw, "%H:%M:%S").is_ok()
        || chrono::NaiveTime::parse_from_str(raw, "%H:%M").is_ok();
    if !ok {
        return Err(AppError::ValidationFailed(vec![format!(
            "Heure invalide '{raw}' (attendu HH:MM)."
        )]));
    }
    Ok(())
}

fn validate_details(details: &[ScheduleDayPattern]) -> AppResult<()> {
    if details.len() != 7 {
        return Err(AppError::ValidationFailed(vec![
            "Le modèle horaire doit contenir exactement 7 jours (lundi–dimanche).".into(),
        ]));
    }
    let mut seen = [false; 8];
    for d in details {
        if !(1..=7).contains(&d.day_of_week) {
            return Err(AppError::ValidationFailed(vec![format!(
                "day_of_week invalide: {} (attendu 1–7).",
                d.day_of_week
            )]));
        }
        if seen[d.day_of_week as usize] {
            return Err(AppError::ValidationFailed(vec![format!(
                "Jour dupliqué: {}.",
                d.day_of_week
            )]));
        }
        seen[d.day_of_week as usize] = true;
        validate_time(d.shift_start.trim())?;
        validate_time(d.shift_end.trim())?;
    }
    Ok(())
}

/// Assert that `reference_value_id` is an active ORG.SCHEDULE_CLASS value.
pub async fn assert_schedule_reference_value_active(
    db: &impl ConnectionTrait,
    reference_value_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.is_active AS is_active, UPPER(TRIM(d.code)) AS domain_code \
             FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE rv.id = ?",
            [reference_value_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "Classe horaire {reference_value_id} introuvable."
            )])
        })?;

    let domain_code: String = row
        .try_get("", "domain_code")
        .map_err(|e| decode_err("domain_code", e))?;
    if domain_code != SCHEDULE_CLASS_DOMAIN_CODE {
        return Err(AppError::ValidationFailed(vec![
            "La valeur sélectionnée n'appartient pas au domaine Classes horaires.".into(),
        ]));
    }
    let is_active: i64 = row
        .try_get("", "is_active")
        .map_err(|e| decode_err("is_active", e))?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Classe horaire {reference_value_id} inactive."
        )]));
    }
    Ok(())
}

async fn assert_is_schedule_class_value(
    db: &DatabaseConnection,
    reference_value_id: i64,
) -> AppResult<values::ReferenceValue> {
    let value = values::get_value(db, reference_value_id).await?;
    let set = sets::get_reference_set(db, value.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    if domain.code.to_ascii_uppercase() != SCHEDULE_CLASS_DOMAIN_CODE {
        return Err(AppError::ValidationFailed(vec![
            "Le modèle horaire n'est disponible que pour ORG.SCHEDULE_CLASS.".into(),
        ]));
    }
    Ok(value)
}

/// Default Mon–Fri 08:00–16:00, Sat–Sun rest.
pub fn default_weekday_details() -> Vec<ScheduleDayPattern> {
    (1..=7)
        .map(|day| {
            let is_rest = day >= 6;
            ScheduleDayPattern {
                day_of_week: day,
                shift_start: "08:00".into(),
                shift_end: "16:00".into(),
                is_rest_day: is_rest,
            }
        })
        .collect()
}

/// Seed default weekday rows when a new ORG.SCHEDULE_CLASS value is created.
pub async fn seed_default_details_if_schedule_class(
    db: &DatabaseConnection,
    value: &values::ReferenceValue,
) -> AppResult<()> {
    let set = sets::get_reference_set(db, value.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    if domain.code.to_ascii_uppercase() != SCHEDULE_CLASS_DOMAIN_CODE {
        return Ok(());
    }

    // Ensure metadata defaults exist.
    if value.metadata_json.is_none() {
        let meta = ScheduleMetadata {
            shift_pattern_code: Some(value.code.clone()),
            is_continuous: Some(false),
            nominal_hours_per_day: Some(8.0),
        };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET metadata_json = ?, semantic_tag = COALESCE(semantic_tag, 'schedule_class') \
             WHERE id = ?",
            [serialize_metadata(&meta).into(), value.id.into()],
        ))
        .await?;
    }

    let existing: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM schedule_details WHERE reference_value_id = ?",
            [value.id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "c").ok())
        .unwrap_or(0);
    if existing > 0 {
        return Ok(());
    }

    for d in default_weekday_details() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO schedule_details \
             (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
             VALUES (?, ?, ?, ?, ?)",
            [
                value.id.into(),
                d.day_of_week.into(),
                d.shift_start.into(),
                d.shift_end.into(),
                (if d.is_rest_day { 1_i64 } else { 0_i64 }).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

async fn load_details(
    db: &DatabaseConnection,
    reference_value_id: i64,
) -> AppResult<Vec<ScheduleDayPattern>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT day_of_week, shift_start, shift_end, is_rest_day \
             FROM schedule_details WHERE reference_value_id = ? ORDER BY day_of_week ASC",
            [reference_value_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let is_rest: i64 = r
            .try_get("", "is_rest_day")
            .map_err(|e| decode_err("is_rest_day", e))?;
        out.push(ScheduleDayPattern {
            day_of_week: r
                .try_get("", "day_of_week")
                .map_err(|e| decode_err("day_of_week", e))?,
            shift_start: r
                .try_get("", "shift_start")
                .map_err(|e| decode_err("shift_start", e))?,
            shift_end: r
                .try_get("", "shift_end")
                .map_err(|e| decode_err("shift_end", e))?,
            is_rest_day: is_rest != 0,
        });
    }
    Ok(out)
}

pub async fn get_schedule_pattern(
    db: &DatabaseConnection,
    reference_value_id: i64,
) -> AppResult<SchedulePattern> {
    let value = assert_is_schedule_class_value(db, reference_value_id).await?;
    let meta = parse_metadata(value.metadata_json.as_deref(), &value.code);
    let mut details = load_details(db, reference_value_id).await?;
    if details.is_empty() {
        seed_default_details_if_schedule_class(db, &value).await?;
        details = load_details(db, reference_value_id).await?;
    }
    Ok(SchedulePattern {
        reference_value_id: value.id,
        code: value.code.clone(),
        label: value.label.clone(),
        is_active: value.is_active,
        shift_pattern_code: meta
            .shift_pattern_code
            .unwrap_or_else(|| value.code.clone()),
        is_continuous: meta.is_continuous.unwrap_or(false),
        nominal_hours_per_day: meta.nominal_hours_per_day.unwrap_or(8.0),
        details,
    })
}

pub async fn upsert_schedule_pattern(
    db: &DatabaseConnection,
    payload: UpsertSchedulePatternPayload,
    _actor_id: i64,
) -> AppResult<SchedulePattern> {
    let value = assert_is_schedule_class_value(db, payload.reference_value_id).await?;
    let set = sets::get_reference_set(db, value.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    let mut meta = parse_metadata(value.metadata_json.as_deref(), &value.code);
    if let Some(code) = payload.shift_pattern_code {
        let trimmed = code.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Le code de modèle de poste est obligatoire.".into(),
            ]));
        }
        meta.shift_pattern_code = Some(trimmed);
    }
    if let Some(v) = payload.is_continuous {
        meta.is_continuous = Some(v);
    }
    if let Some(v) = payload.nominal_hours_per_day {
        if !v.is_finite() || v <= 0.0 || v > 24.0 {
            return Err(AppError::ValidationFailed(vec![
                "Les heures nominales par jour doivent être entre 0 et 24.".into(),
            ]));
        }
        meta.nominal_hours_per_day = Some(v);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET metadata_json = ?, semantic_tag = COALESCE(semantic_tag, 'schedule_class') \
         WHERE id = ?",
        [
            serialize_metadata(&meta).into(),
            payload.reference_value_id.into(),
        ],
    ))
    .await?;

    if let Some(details) = payload.details {
        validate_details(&details)?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM schedule_details WHERE reference_value_id = ?",
            [payload.reference_value_id.into()],
        ))
        .await?;
        for d in details {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO schedule_details \
                 (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
                 VALUES (?, ?, ?, ?, ?)",
                [
                    payload.reference_value_id.into(),
                    d.day_of_week.into(),
                    d.shift_start.trim().to_string().into(),
                    d.shift_end.trim().to_string().into(),
                    (if d.is_rest_day { 1_i64 } else { 0_i64 }).into(),
                ],
            ))
            .await?;
        }
    }

    get_schedule_pattern(db, payload.reference_value_id).await
}
