//! Migration 194 — capacity comes from Classes horaires (ORG.SCHEDULE_CLASS reference values).
//!
//! SAP PM work-centre capacity and Maximo shift calendars: capacity comes from timetables
//! defined once in reference data, never from hours typed on a team or an assumed 8-hour day.
//!
//! - `schedule_class_versions`: what a Classe horaire is from a date — its cycle (a week, or an
//!   N-day rotation such as 3×8 starting on a given date), each cycle day's shift and break, the
//!   utilisation factor (share of shift time available for work orders, 1–100 %), and whether it
//!   runs continuously (3×8, 24/7: through the site's rest days and holidays). A change applies
//!   from its date; earlier versions stay, so past capacity is never rewritten.
//! - `team_schedule_assignments`: a team's default Classe horaire, from a date, for its members
//!   who have none of their own.
//! - `personnel_schedule_exceptions`: dated per-person exceptions — another Classe horaire and/or a
//!   part-time percentage. Absences stay in `personnel_availability_blocks`.
//!
//! Upgrade:
//! - every existing Classe horaire gets a first version from its weekly rows (`schedule_details`),
//!   with an 85 % utilisation flagged for review;
//! - a team that had its own capacity rule keeps that rule's hours as a Classe horaire built from
//!   it (that many hours every day, continuous, 100 %), set as the team's default and flagged for
//!   review. No other team gets hours: a member without a Classe horaire counts 0 h and is listed
//!   on the team page.
//!
//! Every device of a tenant performs the same upgrade on the same rows, so these rows carry seeded
//! identities (`seeded_entity_sync_id`) like other rows created by migrations.
//!
//! Synchronised entity types: `schedule_class_versions`, `team_schedule_assignments`,
//! `personnel_schedule_exceptions`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

/// Start date of the versions and links the upgrade creates.
pub const UPGRADE_FROM: &str = "2000-01-01";
/// Utilisation factor given to the first version of existing Classes horaires (flagged for review).
pub const UPGRADE_UTILISATION_PERCENT: f64 = 85.0;
/// A Classe horaire built from a capacity rule keeps the rule's hours whole.
pub const RULE_UTILISATION_PERCENT: f64 = 100.0;
/// Prefix of the codes of Classes horaires built from a team's capacity rule.
pub const RULE_CLASS_PREFIX: &str = "CAP_RULE_";
/// Shifts built from a capacity rule start at this time.
const RULE_SHIFT_START_MINUTES: i64 = 8 * 60;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000194_schedule_class_timetables"
    }
}

fn now_sql() -> &'static str {
    "strftime('%Y-%m-%dT%H:%M:%SZ','now')"
}

fn err(ctx: &str, e: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(format!("migration 194 {ctx}: {e}"))
}

fn hhmm(minutes: i64) -> String {
    let m = minutes.rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// `hours` every day (08:00 onwards, a full day at most), as cycle days 1 (Monday) … 7 (Sunday).
fn rule_week_json(hours: f64) -> String {
    let minutes = ((hours * 60.0).round() as i64).clamp(1, 24 * 60);
    let start = hhmm(RULE_SHIFT_START_MINUTES);
    let end = hhmm(RULE_SHIFT_START_MINUTES + minutes);
    let days: Vec<serde_json::Value> = (1..=7)
        .map(|d| {
            serde_json::json!({
                "cycle_day": d,
                "shift_start": start,
                "shift_end": end,
                "break_minutes": 0,
                "is_rest_day": false,
            })
        })
        .collect();
    serde_json::Value::Array(days).to_string()
}

/// Team code as a reference value code (letters, digits, underscores).
fn code_part(team_code: &str) -> String {
    team_code
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}

fn format_hours(hours: f64) -> String {
    let rounded = (hours * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded}")
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_class_versions (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id       TEXT NOT NULL UNIQUE,
                reference_value_id   INTEGER NOT NULL REFERENCES reference_values(id),
                effective_from       TEXT NOT NULL CHECK (length(effective_from) = 10),
                cycle_length_days    INTEGER NOT NULL DEFAULT 7 CHECK (cycle_length_days BETWEEN 1 AND 56),
                rotation_start_date  TEXT NULL CHECK (rotation_start_date IS NULL OR length(rotation_start_date) = 10),
                days_json            TEXT NOT NULL,
                is_continuous        INTEGER NOT NULL DEFAULT 0 CHECK (is_continuous IN (0, 1)),
                utilisation_percent  REAL NOT NULL CHECK (utilisation_percent >= 1 AND utilisation_percent <= 100),
                review_status        TEXT NULL CHECK (review_status IS NULL OR review_status = 'to_review'),
                note                 TEXT NULL,
                row_version          INTEGER NOT NULL DEFAULT 1,
                created_by_id        INTEGER NULL REFERENCES user_accounts(id),
                created_at           TEXT NOT NULL,
                updated_at           TEXT NOT NULL,
                CHECK (rotation_start_date IS NOT NULL OR cycle_length_days = 7)
            );
            CREATE INDEX IF NOT EXISTS idx_schedule_class_versions_class
                ON schedule_class_versions(reference_value_id, effective_from);
            CREATE TABLE IF NOT EXISTS team_schedule_assignments (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id               TEXT NOT NULL UNIQUE,
                team_id                      INTEGER NOT NULL REFERENCES teams(id),
                schedule_reference_value_id  INTEGER NULL REFERENCES reference_values(id),
                effective_from               TEXT NOT NULL CHECK (length(effective_from) = 10),
                review_status                TEXT NULL CHECK (review_status IS NULL OR review_status = 'to_review'),
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_by_id                INTEGER NULL REFERENCES user_accounts(id),
                created_at                   TEXT NOT NULL,
                updated_at                   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_team_schedule_assignments_team
                ON team_schedule_assignments(team_id, effective_from);
            CREATE TABLE IF NOT EXISTS personnel_schedule_exceptions (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id               TEXT NOT NULL UNIQUE,
                personnel_id                 INTEGER NOT NULL REFERENCES personnel(id),
                schedule_reference_value_id  INTEGER NULL REFERENCES reference_values(id),
                work_percent                 REAL NULL CHECK (work_percent IS NULL OR (work_percent > 0 AND work_percent <= 100)),
                valid_from                   TEXT NOT NULL CHECK (length(valid_from) = 10),
                valid_to                     TEXT NULL CHECK (valid_to IS NULL OR (length(valid_to) = 10 AND valid_to >= valid_from)),
                reason                       TEXT NULL,
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_by_id                INTEGER NULL REFERENCES user_accounts(id),
                created_at                   TEXT NOT NULL,
                updated_at                   TEXT NOT NULL,
                CHECK (schedule_reference_value_id IS NOT NULL OR work_percent IS NOT NULL)
            );
            CREATE INDEX IF NOT EXISTS idx_personnel_schedule_exceptions_person
                ON personnel_schedule_exceptions(personnel_id, valid_from);",
        )
        .await?;

        // First version of every existing Classe horaire, from its weekly rows.
        let classes = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT rv.id, rv.code, rv.metadata_json FROM reference_values rv \
                 JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                   AND NOT EXISTS (SELECT 1 FROM schedule_class_versions v WHERE v.reference_value_id = rv.id) \
                 ORDER BY rv.id"
                    .to_string(),
            ))
            .await?;
        for class in classes {
            let id: i64 = class.try_get("", "id").map_err(|e| err("class id", e))?;
            let code: String = class.try_get("", "code").map_err(|e| err("class code", e))?;
            let continuous = crate::personnel::timetable::metadata_is_continuous(
                class
                    .try_get::<Option<String>>("", "metadata_json")
                    .ok()
                    .flatten()
                    .as_deref(),
            );
            let lines = db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT day_of_week, shift_start, shift_end, is_rest_day FROM schedule_details \
                     WHERE reference_value_id = ? ORDER BY day_of_week",
                    [id.into()],
                ))
                .await?;
            if lines.is_empty() {
                continue;
            }
            let mut days = Vec::new();
            for l in &lines {
                days.push(serde_json::json!({
                    "cycle_day": l.try_get::<i64>("", "day_of_week").map_err(|e| err("day_of_week", e))?,
                    "shift_start": l.try_get::<String>("", "shift_start").map_err(|e| err("shift_start", e))?,
                    "shift_end": l.try_get::<String>("", "shift_end").map_err(|e| err("shift_end", e))?,
                    "break_minutes": 0,
                    "is_rest_day": l.try_get::<i64>("", "is_rest_day").map_err(|e| err("is_rest_day", e))? != 0,
                }));
            }
            insert_version(
                db,
                id,
                &code,
                &serde_json::Value::Array(days).to_string(),
                continuous,
                UPGRADE_UTILISATION_PERCENT,
                None,
            )
            .await?;
        }

        // Teams with their own capacity rule keep its hours as a Classe horaire to review.
        let teams = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT t.id, t.code, t.name, cr.available_hours_per_day AS hours, \
                        cr.max_overtime_hours_per_day AS overtime, cr.effective_start \
                 FROM teams t \
                 JOIN capacity_rules cr ON cr.id = ( \
                     SELECT r.id FROM capacity_rules r WHERE r.team_id = t.id \
                     ORDER BY (r.effective_end IS NULL OR r.effective_end >= date('now')) DESC, \
                              r.effective_start DESC, r.id DESC LIMIT 1) \
                 WHERE t.status = 'active' AND t.deleted_at IS NULL AND cr.available_hours_per_day > 0 \
                   AND NOT EXISTS (SELECT 1 FROM team_schedule_assignments a WHERE a.team_id = t.id) \
                 ORDER BY t.id"
                    .to_string(),
            ))
            .await?;
        if teams.is_empty() {
            return Ok(());
        }
        let english = crate::qualification::gate::tenant_writes_english(db)
            .await
            .map_err(|e| err("tenant language", e))?;
        for team in teams {
            let team_id: i64 = team.try_get("", "id").map_err(|e| err("team id", e))?;
            let team_code: String = team.try_get("", "code").map_err(|e| err("team code", e))?;
            let team_name: String = team.try_get("", "name").map_err(|e| err("team name", e))?;
            let hours: f64 = team.try_get("", "hours").map_err(|e| err("rule hours", e))?;
            let overtime: f64 = team.try_get("", "overtime").unwrap_or(0.0);
            let since: String = team.try_get("", "effective_start").unwrap_or_default();
            let code = format!("{RULE_CLASS_PREFIX}{}", code_part(&team_code));
            let h = format_hours(hours);
            let (label, note) = if english {
                (
                    format!("{team_name} — {h} h/day (former capacity rule)"),
                    format!(
                        "Built by the update from the team's capacity rule ({h} h per day, {} h overtime, since {since}). \
                         It applies to each member of the team who has no Classe horaire of their own: review it.",
                        format_hours(overtime)
                    ),
                )
            } else {
                (
                    format!("{team_name} — {h} h/jour (ancienne règle de capacité)"),
                    format!(
                        "Créée par la mise à jour à partir de la règle de capacité de l'équipe ({h} h par jour, \
                         {} h sup., depuis le {since}). Elle s'applique à chaque membre de l'équipe sans classe \
                         horaire propre : à revoir.",
                        format_hours(overtime)
                    ),
                )
            };
            let class_id = ensure_class(db, &code, &label).await?;
            insert_version(
                db,
                class_id,
                &code,
                &rule_week_json(hours),
                true,
                RULE_UTILISATION_PERCENT,
                Some(&note),
            )
            .await?;
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO team_schedule_assignments \
                     (entity_sync_id, team_id, schedule_reference_value_id, effective_from, review_status, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, 'to_review', {now}, {now})",
                    now = now_sql()
                ),
                [
                    seeded_entity_sync_id("team_schedule_assignments", &format!("migrated:{team_code}")).into(),
                    team_id.into(),
                    class_id.into(),
                    UPGRADE_FROM.into(),
                ],
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TABLE IF EXISTS personnel_schedule_exceptions; \
                 DROP TABLE IF EXISTS team_schedule_assignments; \
                 DROP TABLE IF EXISTS schedule_class_versions;",
            )
            .await?;
        Ok(())
    }
}

async fn insert_version(
    db: &SchemaManagerConnection<'_>,
    class_id: i64,
    code: &str,
    days_json: &str,
    continuous: bool,
    utilisation: f64,
    note: Option<&str>,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        format!(
            "INSERT OR IGNORE INTO schedule_class_versions \
             (entity_sync_id, reference_value_id, effective_from, cycle_length_days, rotation_start_date, days_json, \
              is_continuous, utilisation_percent, review_status, note, created_at, updated_at) \
             VALUES (?, ?, ?, 7, NULL, ?, ?, ?, 'to_review', ?, {now}, {now})",
            now = now_sql()
        ),
        [
            seeded_entity_sync_id("schedule_class_versions", &format!("{code}:{UPGRADE_FROM}")).into(),
            class_id.into(),
            UPGRADE_FROM.into(),
            days_json.into(),
            i64::from(continuous).into(),
            utilisation.into(),
            note.map(str::to_string).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// A Classe horaire (reference value) with this code, created in the published set if missing.
async fn ensure_class(db: &SchemaManagerConnection<'_>, code: &str, label: &str) -> Result<i64, DbErr> {
    let find = || {
        Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.code = ? LIMIT 1",
            [code.into()],
        )
    };
    if let Some(row) = db.query_one(find()).await? {
        return row.try_get("", "id").map_err(|e| err("class id", e));
    }
    let has_sync_id = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA table_info(reference_values)".to_string(),
        ))
        .await?
        .iter()
        .any(|r| r.try_get::<String>("", "name").is_ok_and(|n| n == "entity_sync_id"));
    let metadata = serde_json::json!({ "shift_pattern_code": code, "is_continuous": true }).to_string();
    let (columns, placeholders) = if has_sync_id {
        (", entity_sync_id", ", ?")
    } else {
        ("", "")
    };
    let mut values: Vec<sea_orm::Value> = vec![code.into(), label.into(), metadata.into()];
    if has_sync_id {
        values.push(seeded_entity_sync_id("reference_values", &format!("ORG.SCHEDULE_CLASS:{code}")).into());
    }
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO reference_values \
                 (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, \
                  external_code, is_active, metadata_json{columns}) \
                 SELECT rs.id, NULL, ?, ?, NULL, \
                        (SELECT COALESCE(MAX(x.sort_order), 0) + 1 FROM reference_values x WHERE x.set_id = rs.id), \
                        NULL, NULL, 'schedule_class', NULL, 1, ?{placeholders} \
                   FROM reference_domains d \
                   JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
                  WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                  ORDER BY rs.version_no DESC LIMIT 1"
            ),
            values,
        ))
        .await?;
    if inserted.rows_affected() == 0 {
        return Err(err("class", "the Classe horaire reference list has no published set"));
    }
    db.query_one(find())
        .await?
        .ok_or_else(|| err("class", "not found after insert"))?
        .try_get("", "id")
        .map_err(|e| err("class id", e))
}

#[cfg(test)]
mod tests {
    use super::{code_part, format_hours, rule_week_json};

    #[test]
    fn a_capacity_rule_becomes_that_many_hours_every_day() {
        let days: serde_json::Value = serde_json::from_str(&rule_week_json(7.5)).expect("json");
        let days = days.as_array().expect("array");
        assert_eq!(days.len(), 7);
        assert!(days.iter().all(|d| d["shift_start"] == "08:00" && d["shift_end"] == "15:30"));
        assert!(days.iter().all(|d| d["is_rest_day"] == false));
        // A team total above a day (30 h for four people) is kept as a full-day shift to review.
        let full: serde_json::Value = serde_json::from_str(&rule_week_json(30.0)).expect("json");
        assert_eq!(full[0]["shift_end"], "08:00");
        assert_eq!(code_part("eq-méca 1"), "EQ_M_CA_1");
        assert_eq!(format_hours(8.0), "8");
        assert_eq!(format_hours(7.5), "7.5");
    }
}
