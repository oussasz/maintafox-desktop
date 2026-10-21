//! Personnel availability computations and block mutations.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domain::PersonnelAvailabilityState;

use crate::errors::{AppError, AppResult};
use crate::personnel::capacity::PersonDayCapacity;

pub use super::domain::PersonnelAvailabilityBlock;

/// Returned in ValidationFailed when overlap exists and confirm_overlap was not set.
pub const OVERLAP_CONFIRM_CODE: &str = "AVAILABILITY_BLOCK_OVERLAP_CONFIRM";

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilityCalendarFilter {
    pub date_from: String,
    pub date_to: String,
    pub personnel_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailabilityCalendarEntry {
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub work_date: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    pub scheduled_minutes: i64,
    pub blocked_minutes: i64,
    pub available_minutes: i64,
    pub has_critical_block: bool,
    pub block_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilityBlockCreateInput {
    pub personnel_id: i64,
    pub block_type: String,
    pub start_at: String,
    pub end_at: String,
    pub reason_note: Option<String>,
    pub is_critical: Option<bool>,
    /// When true, skip hard overlap rejection (soft-confirm path).
    pub confirm_overlap: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilityBlockUpdateInput {
    pub id: i64,
    /// The new absence type; `None` keeps the stored one (a caller who may not read the type —
    /// `personnel::absence_access` — edits dates and the critical flag without seeing it).
    #[serde(default)]
    pub block_type: Option<String>,
    pub start_at: String,
    pub end_at: String,
    pub reason_note: Option<String>,
    /// Keeps the stored reason instead of `reason_note` (a caller the reason was withheld from).
    #[serde(default)]
    pub keep_reason_note: bool,
    pub is_critical: Option<bool>,
    pub confirm_overlap: Option<bool>,
}

async fn assert_no_overlapping_blocks(
    db: &DatabaseConnection,
    personnel_id: i64,
    start_at: &str,
    end_at: &str,
    exclude_id: Option<i64>,
    confirm_overlap: bool,
) -> AppResult<()> {
    let mut sql = String::from(
        "SELECT id FROM personnel_availability_blocks
         WHERE personnel_id = ?
           AND start_at < ?
           AND end_at > ?",
    );
    let mut values: Vec<sea_orm::Value> = vec![personnel_id.into(), end_at.into(), start_at.into()];
    if let Some(id) = exclude_id {
        sql.push_str(" AND id <> ?");
        values.push(id.into());
    }
    sql.push_str(" LIMIT 1");

    let overlap = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    if overlap.is_some() {
        if confirm_overlap {
            return Ok(());
        }
        return Err(AppError::ValidationFailed(vec![OVERLAP_CONFIRM_CODE.to_string()]));
    }
    Ok(())
}

const BLOCK_SELECT_SQL: &str = "SELECT
                b.id,
                b.personnel_id,
                b.block_type,
                b.start_at,
                b.end_at,
                b.reason_note,
                b.is_critical,
                b.created_by_id,
                COALESCE(NULLIF(TRIM(ua.display_name), ''), ua.username) AS created_by_label,
                b.created_at
             FROM personnel_availability_blocks b
             LEFT JOIN user_accounts ua ON ua.id = b.created_by_id";

fn map_block_row(row: sea_orm::QueryResult) -> PersonnelAvailabilityBlock {
    PersonnelAvailabilityBlock {
        id: row.try_get("", "id").unwrap_or_default(),
        personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
        block_type: row.try_get("", "block_type").unwrap_or_default(),
        start_at: row.try_get("", "start_at").unwrap_or_default(),
        end_at: row.try_get("", "end_at").unwrap_or_default(),
        reason_note: row.try_get("", "reason_note").unwrap_or(None),
        is_critical: row.try_get::<i64>("", "is_critical").unwrap_or(0) == 1,
        created_by_id: row.try_get("", "created_by_id").unwrap_or(None),
        created_by_label: row.try_get("", "created_by_label").unwrap_or(None),
        created_at: row.try_get("", "created_at").unwrap_or_default(),
        reason_masked: false,
        type_masked: false,
        critical_for_planning: false,
    }
}

async fn fetch_block_by_id(db: &DatabaseConnection, id: i64) -> AppResult<PersonnelAvailabilityBlock> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{BLOCK_SELECT_SQL} WHERE b.id = ? LIMIT 1"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "AvailabilityBlock".into(),
            id: id.to_string(),
        })?;
    Ok(map_block_row(row))
}

/// Day-by-day availability of people: who, which team, and — from `personnel::capacity`, the
/// same calculation as planning — the shift of their Classe horaire, scheduled / absent /
/// available minutes and the absence types of the day.
pub async fn list_availability_calendar(
    db: &DatabaseConnection,
    filter: AvailabilityCalendarFilter,
) -> AppResult<Vec<AvailabilityCalendarEntry>> {
    let parse = |raw: &str, field: &str| {
        chrono::NaiveDate::parse_from_str(raw.get(..10).unwrap_or(raw), "%Y-%m-%d")
            .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be YYYY-MM-DD.")]))
    };
    let from = parse(&filter.date_from, "date_from")?;
    let to = parse(&filter.date_to, "date_to")?;
    if from > to {
        return Err(AppError::ValidationFailed(vec![
            "date_from must be <= date_to.".to_string()
        ]));
    }

    let mut personnel_where = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = vec![from.to_string().into(), to.to_string().into()];
    if let Some(entity_id) = filter.entity_id {
        personnel_where.push("p.primary_entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if let Some(personnel_id) = filter.personnel_id {
        personnel_where.push("p.id = ?".to_string());
        values.push(personnel_id.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        personnel_where.push("p.employment_status = 'active'".to_string());
    }
    let team_filter_sql = if filter.team_id.is_some() {
        "AND COALESCE(t.id, home.id, pd.primary_team_id) = ?"
    } else {
        ""
    };
    if let Some(team_id) = filter.team_id {
        values.push(team_id.into());
    }

    let sql = format!(
        "WITH RECURSIVE dates(work_date) AS (
            SELECT date(?)
            UNION ALL
            SELECT date(work_date, '+1 day') FROM dates WHERE work_date < date(?)
         ),
         person_days AS (
            SELECT p.id, p.employee_code, p.full_name, p.primary_entity_id, p.primary_team_id, d.work_date
            FROM personnel p CROSS JOIN dates d
            WHERE {}
         )
         SELECT
            pd.id AS personnel_id, pd.employee_code, pd.full_name,
            pd.primary_entity_id AS entity_id, e.name AS entity_name,
            COALESCE(t.id, home.id, pd.primary_team_id) AS team_id,
            COALESCE(t.name, home.name) AS team_name,
            pd.work_date
         FROM person_days pd
         LEFT JOIN org_nodes e ON e.id = pd.primary_entity_id
         LEFT JOIN teams home ON home.id = pd.primary_team_id
         LEFT JOIN teams t ON t.id = (
            SELECT pta.team_id
            FROM personnel_team_assignments pta
            WHERE pta.personnel_id = pd.id
              AND (pta.valid_from IS NULL OR date(pta.valid_from) <= pd.work_date)
              AND (pta.valid_to IS NULL OR date(pta.valid_to) >= pd.work_date)
            ORDER BY pta.is_lead DESC, pta.id DESC
            LIMIT 1
         )
         WHERE 1 = 1
         {}
         ORDER BY pd.full_name ASC, pd.work_date ASC",
        personnel_where.join(" AND "),
        team_filter_sql
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;

    // Each person's days, computed once.
    let mut days_of: HashMap<i64, HashMap<String, PersonDayCapacity>> = HashMap::new();
    let mut critical_of: HashMap<i64, Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)>> = HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let personnel_id: i64 = row.try_get("", "personnel_id").unwrap_or_default();
        if let std::collections::hash_map::Entry::Vacant(slot) = days_of.entry(personnel_id) {
            let days = crate::personnel::capacity::person_capacity_span(db, personnel_id, from, to).await?;
            slot.insert(days.into_iter().map(|d| (d.day.clone(), d)).collect());
            critical_of.insert(personnel_id, critical_blocks(db, personnel_id, from, to).await?);
        }
        let work_date: String = row.try_get("", "work_date").unwrap_or_default();
        let day = days_of.get(&personnel_id).and_then(|d| d.get(&work_date));
        let window = day.and_then(|d| {
            let date = chrono::NaiveDate::parse_from_str(&d.day, "%Y-%m-%d").ok()?;
            crate::personnel::capacity::shift_window(date, d.shift_start.as_deref()?, d.shift_end.as_deref()?)
        });
        let has_critical_block = window.is_some_and(|(s, e)| {
            critical_of
                .get(&personnel_id)
                .is_some_and(|blocks| blocks.iter().any(|(bs, be)| *bs < e && *be > s))
        });
        out.push(AvailabilityCalendarEntry {
            personnel_id,
            employee_code: row.try_get("", "employee_code").unwrap_or_default(),
            full_name: row.try_get("", "full_name").unwrap_or_default(),
            entity_id: row.try_get("", "entity_id").unwrap_or(None),
            entity_name: row.try_get("", "entity_name").unwrap_or(None),
            team_id: row.try_get("", "team_id").unwrap_or(None),
            team_name: row.try_get("", "team_name").unwrap_or(None),
            work_date,
            shift_start: day.and_then(|d| d.shift_start.clone()),
            shift_end: day.and_then(|d| d.shift_end.clone()),
            scheduled_minutes: day.map_or(0, |d| d.scheduled_minutes),
            blocked_minutes: day.map_or(0, |d| d.absent_minutes),
            available_minutes: day.map_or(0, |d| d.available_minutes),
            has_critical_block,
            block_types: day.map(|d| d.absence_types.clone()).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Critical absence blocks of a person that can touch a shift of `[from, to]`.
async fn critical_blocks(
    db: &DatabaseConnection,
    personnel_id: i64,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> AppResult<Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT start_at, end_at FROM personnel_availability_blocks WHERE personnel_id = ? AND is_critical = 1 \
             AND julianday(start_at) < julianday(?) AND julianday(end_at) > julianday(?)",
            [
                personnel_id.into(),
                (to + chrono::Duration::days(2)).to_string().into(),
                from.to_string().into(),
            ],
        ))
        .await?;
    let parse = |raw: &str| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|d| d.naive_utc())
            .ok()
            .or_else(|| chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S").ok())
            .or_else(|| chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S").ok())
    };
    Ok(rows
        .iter()
        .filter_map(|r| {
            let s: String = r.try_get("", "start_at").ok()?;
            let e: String = r.try_get("", "end_at").ok()?;
            Some((parse(&s)?, parse(&e)?))
        })
        .collect())
}

pub async fn create_availability_block(
    db: &DatabaseConnection,
    input: AvailabilityBlockCreateInput,
    actor_id: i64,
) -> AppResult<PersonnelAvailabilityBlock> {
    if input.start_at >= input.end_at {
        return Err(AppError::ValidationFailed(vec![
            "start_at must be before end_at.".to_string()
        ]));
    }
    // A value of the plant's absence-type list; its tag decides whether it is always critical.
    let (block_type, tag) = super::reference_lists::checked_absence_type(db, &input.block_type).await?;

    assert_no_overlapping_blocks(
        db,
        input.personnel_id,
        &input.start_at,
        &input.end_at,
        None,
        input.confirm_overlap.unwrap_or(false),
    )
    .await?;

    let is_critical = input.is_critical.unwrap_or(false) || tag == super::reference_lists::ABSENCE_TAG_CRITICAL;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_availability_blocks
            (personnel_id, block_type, start_at, end_at, reason_note, is_critical, created_by_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            input.personnel_id.into(),
            block_type.into(),
            input.start_at.clone().into(),
            input.end_at.clone().into(),
            input.reason_note.clone().into(),
            (if is_critical { 1 } else { 0 }).into(),
            actor_id.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("{BLOCK_SELECT_SQL} WHERE b.id = last_insert_rowid()"),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("availability block insert failed")))?;

    Ok(map_block_row(row))
}

/// Audit action of a change of a block's critical flag (who, block, person, before → after; never
/// the absence type or reason).
pub const AUDIT_BLOCK_CRITICAL_CHANGED: &str = "personnel.availability_block.critical_changed";

/// Refusal of a type change by a caller the block's type is withheld from (translated in
/// `training:errors`).
pub const CODE_ABSENCE_TYPE_HIDDEN: &str = "PERSONNEL_ABSENCE_TYPE_HIDDEN";

/// Edits a block. `access` is what the caller may read (`personnel::absence_access`): for a caller
/// the block's type and reason are withheld from, the server never takes them from the payload —
/// the stored reason is kept whatever is sent, and any type in the payload is refused
/// ([`CODE_ABSENCE_TYPE_HIDDEN`]); dates and the critical flag stay editable.
pub async fn update_availability_block(
    db: &DatabaseConnection,
    input: AvailabilityBlockUpdateInput,
    actor_id: i64,
    access: &crate::personnel::absence_access::AbsenceAccess,
) -> AppResult<PersonnelAvailabilityBlock> {
    if input.start_at >= input.end_at {
        return Err(AppError::ValidationFailed(vec![
            "start_at must be before end_at.".to_string()
        ]));
    }
    let existing = fetch_block_by_id(db, input.id).await?;
    let hidden =
        access.detail_for(existing.personnel_id) == crate::personnel::absence_access::AbsenceDetail::AvailabilityOnly;
    // Any type in the payload is refused, even the stored one: accepting a right guess would let
    // the caller probe the hidden type.
    if hidden && input.block_type.is_some() {
        return Err(crate::errors::org_validation_failed(
            CODE_ABSENCE_TYPE_HIDDEN,
            "The absence type of this block is sensitive data you cannot read: it cannot be changed.",
            HashMap::from([("block_id".to_string(), existing.id.to_string())]),
        ));
    }
    // Only a change of type is checked against the list: a block whose stored type was retired
    // or flagged stays editable and keeps that type's behaviour.
    // Unchanged type: the stored critical flag is kept (a legacy type adopted "à revoir" never
    // forces it, so a note edit cannot flip an old block); only a shipped or plant type tagged
    // critical forces it, as on creation.
    let requested_type = input.block_type.as_deref().unwrap_or(&existing.block_type);
    let (block_type, forced_critical, keep_critical) = if requested_type == existing.block_type {
        let forced = matches!(
            super::reference_lists::stored_absence_type(db, &existing.block_type).await?,
            Some((Some(ref tag), false)) if tag == super::reference_lists::ABSENCE_TAG_CRITICAL
        );
        (existing.block_type.clone(), forced, existing.is_critical)
    } else {
        let (code, tag) = super::reference_lists::checked_absence_type(db, requested_type).await?;
        (code, tag == super::reference_lists::ABSENCE_TAG_CRITICAL, false)
    };
    assert_no_overlapping_blocks(
        db,
        existing.personnel_id,
        &input.start_at,
        &input.end_at,
        Some(input.id),
        input.confirm_overlap.unwrap_or(false),
    )
    .await?;

    let is_critical = forced_critical || input.is_critical.unwrap_or(keep_critical);
    // A caller the reason is withheld from never overwrites it.
    let reason_note = if hidden || input.keep_reason_note {
        existing.reason_note.clone()
    } else {
        input.reason_note.clone()
    };

    // The change and its audit event commit together.
    let txn = db.begin().await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_availability_blocks
         SET block_type = ?, start_at = ?, end_at = ?, reason_note = ?, is_critical = ?
         WHERE id = ?",
        [
            block_type.into(),
            input.start_at.clone().into(),
            input.end_at.clone().into(),
            reason_note.into(),
            (if is_critical { 1 } else { 0 }).into(),
            input.id.into(),
        ],
    ))
    .await?;
    if is_critical != existing.is_critical {
        crate::audit::writer::write_audit_event_in(
            &txn,
            crate::audit::writer::AuditEventInput {
                action_code: AUDIT_BLOCK_CRITICAL_CHANGED.to_string(),
                target_type: Some("personnel_availability_block".to_string()),
                target_id: Some(existing.id.to_string()),
                actor_id: Some(actor_id),
                auth_context: "session".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                // Who (actor), which block and person, before → after; never the type or reason.
                details_json: Some(serde_json::json!({
                    "block_id": existing.id,
                    "personnel_id": existing.personnel_id,
                    "is_critical_before": existing.is_critical,
                    "is_critical_after": is_critical,
                })),
            },
        )
        .await?;
    }
    txn.commit().await?;

    fetch_block_by_id(db, input.id).await
}

pub async fn delete_availability_block(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let existing = fetch_block_by_id(db, id).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM personnel_availability_blocks WHERE id = ?",
        [existing.id.into()],
    ))
    .await?;
    Ok(())
}

/// Truncate an active block so `end_at = now` (soft end). Rejects if now is outside [start, end).
pub async fn end_availability_block(db: &DatabaseConnection, id: i64) -> AppResult<PersonnelAvailabilityBlock> {
    let existing = fetch_block_by_id(db, id).await?;
    let now = Utc::now();
    let now_str = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let start = chrono::DateTime::parse_from_rfc3339(&existing.start_at)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&existing.start_at, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|n| n.and_utc())
        });
    let end = chrono::DateTime::parse_from_rfc3339(&existing.end_at)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&existing.end_at, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|n| n.and_utc())
        });

    let (Some(start_dt), Some(end_dt)) = (start, end) else {
        return Err(AppError::ValidationFailed(vec![
            "Block start_at/end_at could not be parsed.".to_string(),
        ]));
    };

    if !(start_dt < now && now < end_dt) {
        return Err(AppError::ValidationFailed(vec![
            "Block can only be ended while it is currently active (start < now < end).".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel_availability_blocks SET end_at = ? WHERE id = ?",
        [now_str.into(), id.into()],
    ))
    .await?;

    fetch_block_by_id(db, id).await
}

// ─────────────────────────────────────────────────────────────────────────────
// Computed availability state (point-in-time)
// ─────────────────────────────────────────────────────────────────────────────

/// Reason codes of [`compute_personnel_availability_state`]; the screen shows them in the user's
/// language (`personnel` namespace, `availability.inspector.reason.<code>`). A block reason is the
/// block's absence type code instead.
pub const REASON_EMPLOYMENT_TERMINATED: &str = "employment_terminated";
pub const REASON_BLOCKED_OVERRIDE: &str = "blocked_override";
/// The person is on open work orders; their number is `open_work_order_count`.
pub const REASON_OPEN_WORK_ORDERS: &str = "open_work_orders";
/// The program's own reason codes (any other reason is an absence type code).
pub const PROGRAM_REASON_CODES: &[&str] = &[
    REASON_EMPLOYMENT_TERMINATED,
    REASON_BLOCKED_OVERRIDE,
    REASON_OPEN_WORK_ORDERS,
];

/// Compute the current availability state for a single personnel.
///
/// Priority:
/// 1. `blocked_override = 1` → blocked
/// 2. Active `personnel_availability_blocks` (leave / training / medical) → on_leave / in_training / blocked
/// 3. Open WOs the person is on (responsible or booked labour, `queries::PERSON_ON_WO`) → assigned
/// 4. Otherwise → available
pub async fn compute_personnel_availability_state(
    db: &DatabaseConnection,
    personnel_id: i64,
) -> AppResult<PersonnelAvailabilityState> {
    let as_of = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Fetch blocked_override + employment_status
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT blocked_override, employment_status FROM personnel WHERE id = ? LIMIT 1",
            [personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: personnel_id.to_string(),
        })?;

    let blocked_override: i64 = row.try_get("", "blocked_override").unwrap_or(0);
    let employment_status: String = row
        .try_get("", "employment_status")
        .unwrap_or_else(|_| "active".to_string());

    if employment_status == "terminated" {
        return Ok(PersonnelAvailabilityState {
            personnel_id,
            status: "inactive".to_string(),
            blocked_override: false,
            reasons: vec![REASON_EMPLOYMENT_TERMINATED.to_string()],
            open_work_order_count: None,
            as_of,
        });
    }

    if blocked_override != 0 {
        return Ok(PersonnelAvailabilityState {
            personnel_id,
            status: "blocked".to_string(),
            blocked_override: true,
            reasons: vec![REASON_BLOCKED_OVERRIDE.to_string()],
            open_work_order_count: None,
            as_of,
        });
    }

    // Check active blocks
    // The status follows the behaviour tag of each block's absence type
    // (`personnel::reference_lists`, PERSONNEL.ABSENCE_TYPES): training -> in training, leave -> on
    // leave, otherwise (critical, or a type no longer in the list) blocked.
    let block_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT b.block_type, {tag} AS tag FROM personnel_availability_blocks b \
                 WHERE b.personnel_id = ? AND b.start_at <= ? AND b.end_at > ? \
                 ORDER BY b.is_critical DESC, b.id ASC LIMIT 10",
                tag = super::reference_lists::BLOCK_TAG_SQL
            ),
            [personnel_id.into(), as_of.clone().into(), as_of.clone().into()],
        ))
        .await?;

    if !block_rows.is_empty() {
        let mut block_types: Vec<String> = Vec::with_capacity(block_rows.len());
        let mut tags: Vec<Option<String>> = Vec::with_capacity(block_rows.len());
        for r in &block_rows {
            block_types.push(r.try_get("", "block_type")?);
            tags.push(r.try_get("", "tag")?);
        }
        let has = |wanted: &str| tags.iter().any(|t| t.as_deref() == Some(wanted));

        let status = if has(super::reference_lists::ABSENCE_TAG_TRAINING) {
            "in_training"
        } else if has(super::reference_lists::ABSENCE_TAG_LEAVE) {
            "on_leave"
        } else {
            "blocked"
        };

        return Ok(PersonnelAvailabilityState {
            personnel_id,
            status: status.to_string(),
            blocked_override: false,
            reasons: block_types,
            open_work_order_count: None,
            as_of,
        });
    }

    // Open work the person is on: "Open (work to do)" OTs (`wo::backlog::OPEN_STATUS_SQL`) where
    // the person is responsible or booked as labour — the same rule as their workload
    // (`queries::PERSON_ON_WO`). A failed read is an error, never "free".
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS n FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE {person_on_wo}
                   AND s.code IN {open}",
                person_on_wo = super::queries::PERSON_ON_WO,
                open = crate::wo::backlog::OPEN_STATUS_SQL
            ),
            [personnel_id.into(), personnel_id.into(), personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("open work order count returned no row")))?;
    let count: i64 = wo_row
        .try_get("", "n")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("open work order count decode: {e}")))?;
    if count > 0 {
        return Ok(PersonnelAvailabilityState {
            personnel_id,
            status: "assigned".to_string(),
            blocked_override: false,
            reasons: vec![REASON_OPEN_WORK_ORDERS.to_string()],
            open_work_order_count: Some(count),
            as_of,
        });
    }

    Ok(PersonnelAvailabilityState {
        personnel_id,
        status: "available".to_string(),
        blocked_override: false,
        reasons: vec![],
        open_work_order_count: Some(0),
        as_of,
    })
}
