//! Migration 194 — teams work to a Classe horaire (ORG.SCHEDULE_CLASS reference value).
//!
//! SAP PM work-centre capacity and Maximo shift calendars: capacity comes from a timetable
//! defined once in reference data, never from hours typed on a team.
//!
//! - `schedule_class_versions`: what a Classe horaire is from a date — its cycle (a week, or an
//!   N-day rotation such as 3×8 starting on a given date), each cycle day's shift and break, and
//!   the utilisation factor (share of shift time available for work orders, 1–100 %), and whether
//!   it runs continuously (3×8, 24/7: through the site's rest days and holidays). A change
//!   applies from its date; earlier versions stay, so past capacity is never rewritten.
//! - `team_schedule_assignments`: the Classe horaire a team works to, from a date (members inherit
//!   it).
//! - `personnel_schedule_exceptions`: dated per-person exceptions — another Classe horaire and/or a
//!   part-time percentage. Absences stay in `personnel_availability_blocks`.
//!
//! Existing Classes horaires get a first version from their weekly rows (`schedule_details`), with
//! an 85 % utilisation flagged for review. On a database that already has teams, one Classe horaire
//! "Standard 8 h (migrated)" (Sunday–Thursday 08:00–16:00, 85 %, flagged for review) is created and
//! every active team is linked to it, flagged for review: no team loses its capacity on upgrade.
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
/// Utilisation factor given to existing and migrated Classes horaires (flagged for review).
pub const UPGRADE_UTILISATION_PERCENT: f64 = 85.0;
/// Code of the Classe horaire created for existing teams.
pub const MIGRATED_CLASS_CODE: &str = "STD_8H_MIGRATED";

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

/// The Algerian working week, Sunday–Thursday 08:00–16:00, as cycle days 1 (Monday) … 7 (Sunday).
fn standard_week_json() -> String {
    let days: Vec<serde_json::Value> = (1..=7)
        .map(|d| {
            serde_json::json!({
                "cycle_day": d,
                "shift_start": "08:00",
                "shift_end": "16:00",
                "break_minutes": 0,
                "is_rest_day": d == 5 || d == 6,
            })
        })
        .collect();
    serde_json::Value::Array(days).to_string()
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
            insert_version(db, id, &code, &serde_json::Value::Array(days).to_string(), continuous).await?;
        }

        // Existing teams: one migrated Classe horaire, every active team linked to it.
        let teams = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT t.id, t.code FROM teams t \
                 WHERE t.status = 'active' AND t.deleted_at IS NULL \
                   AND NOT EXISTS (SELECT 1 FROM team_schedule_assignments a WHERE a.team_id = t.id) \
                 ORDER BY t.id"
                    .to_string(),
            ))
            .await?;
        if teams.is_empty() {
            return Ok(());
        }
        let class_id = ensure_migrated_class(db).await?;
        for team in teams {
            let team_id: i64 = team.try_get("", "id").map_err(|e| err("team id", e))?;
            let team_code: String = team.try_get("", "code").map_err(|e| err("team code", e))?;
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
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        format!(
            "INSERT INTO schedule_class_versions \
             (entity_sync_id, reference_value_id, effective_from, cycle_length_days, rotation_start_date, days_json, \
              is_continuous, utilisation_percent, review_status, created_at, updated_at) \
             VALUES (?, ?, ?, 7, NULL, ?, ?, ?, 'to_review', {now}, {now})",
            now = now_sql()
        ),
        [
            seeded_entity_sync_id("schedule_class_versions", &format!("{code}:{UPGRADE_FROM}")).into(),
            class_id.into(),
            UPGRADE_FROM.into(),
            days_json.into(),
            i64::from(continuous).into(),
            UPGRADE_UTILISATION_PERCENT.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The "Standard 8 h (migrated)" Classe horaire, named in the tenant's language, with its weekly
/// rows (read by the attendance views) and its first version.
async fn ensure_migrated_class(db: &SchemaManagerConnection<'_>) -> Result<i64, DbErr> {
    let find = || {
        Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.code = ? LIMIT 1",
            [MIGRATED_CLASS_CODE.into()],
        )
    };
    if let Some(row) = db.query_one(find()).await? {
        return row.try_get("", "id").map_err(|e| err("migrated class id", e));
    }
    let english = crate::qualification::gate::tenant_writes_english(db)
        .await
        .map_err(|e| err("tenant language", e))?;
    let label = if english {
        "Standard 8 h (migrated)"
    } else {
        "Standard 8 h (migrée)"
    };
    let has_sync_id = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA table_info(reference_values)".to_string(),
        ))
        .await?
        .iter()
        .any(|r| r.try_get::<String>("", "name").is_ok_and(|n| n == "entity_sync_id"));
    let metadata = format!(
        r#"{{"shift_pattern_code":"{MIGRATED_CLASS_CODE}","is_continuous":false,"nominal_hours_per_day":8.0}}"#
    );
    let (columns, placeholders) = if has_sync_id {
        (", entity_sync_id", ", ?")
    } else {
        ("", "")
    };
    let mut values: Vec<sea_orm::Value> = vec![MIGRATED_CLASS_CODE.into(), label.into(), metadata.into()];
    if has_sync_id {
        values.push(
            seeded_entity_sync_id("reference_values", &format!("ORG.SCHEDULE_CLASS:{MIGRATED_CLASS_CODE}")).into(),
        );
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
        return Err(err(
            "migrated class",
            "the Classe horaire reference list has no published set",
        ));
    }
    let id: i64 = db
        .query_one(find())
        .await?
        .ok_or_else(|| err("migrated class", "not found after insert"))?
        .try_get("", "id")
        .map_err(|e| err("migrated class id", e))?;
    for day in 1..=7_i64 {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
             VALUES (?, ?, '08:00', '16:00', ?)",
            [id.into(), day.into(), i64::from(day == 5 || day == 6).into()],
        ))
        .await?;
    }
    insert_version(db, id, MIGRATED_CLASS_CODE, &standard_week_json(), false).await?;
    Ok(id)
}
