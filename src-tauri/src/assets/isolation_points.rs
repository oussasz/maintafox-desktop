//! Isolation points of an equipment (L4-12, migration 290): where its energies are isolated, kept
//! once on the equipment instead of being typed again on every permit.
//!
//! A point has a code (unique on its equipment, upper case), a label, the energy it isolates
//! (`PERMIT.ENERGY_TYPES`) and how (`PERMIT.ISOLATION_METHODS`, the plant's devices and
//! procedures), both from Données de référence, and its normal state (free text: "fermé",
//! "sous tension"…). Points are managed on the equipment (`eq.manage`) and deactivated, never
//! deleted. A permit on the equipment proposes its active points ([`list_proposed_isolation_points`]):
//! the permit screen pre-fills its isolation form from one, still editable, and the isolation is
//! recorded as any other (`permit::queries::upsert_permit_isolation`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::permit::energy_reference::{require_active_value, ENERGY_TYPES_DOMAIN, ISOLATION_METHODS_DOMAIN};

/// Longest code of a point.
pub const MAX_POINT_CODE_CHARS: usize = 40;
/// Longest label of a point.
pub const MAX_POINT_LABEL_CHARS: usize = 200;
/// Longest normal state of a point.
pub const MAX_NORMAL_STATE_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentIsolationPoint {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub point_code: String,
    pub label: String,
    pub energy_type_id: i64,
    pub energy_type_code: Option<String>,
    pub energy_type_label: Option<String>,
    pub isolation_method_id: i64,
    pub isolation_method_label: Option<String>,
    pub normal_state: Option<String>,
    pub is_active: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentIsolationPointUpsertInput {
    /// `None`: a new point.
    #[serde(default)]
    pub id: Option<i64>,
    pub equipment_id: i64,
    /// Required to edit a point.
    #[serde(default)]
    pub expected_row_version: Option<i64>,
    pub point_code: String,
    pub label: String,
    #[serde(default)]
    pub energy_type_id: Option<i64>,
    #[serde(default)]
    pub isolation_method_id: Option<i64>,
    #[serde(default)]
    pub normal_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentIsolationPointActiveInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub is_active: bool,
}

/// An active point of a permit's equipment, as the permit proposes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedIsolationPoint {
    pub point: EquipmentIsolationPoint,
    /// The isolation point text the permit's isolation gets ("CODE — label").
    pub isolation_point_text: String,
    /// A live isolation of the permit already has this text.
    pub already_on_permit: bool,
}

const SELECT: &str = "SELECT p.id, p.entity_sync_id, p.equipment_id, p.point_code, p.label, p.energy_type_id, et.code AS energy_type_code, et.label AS energy_type_label, p.isolation_method_id, im.label AS isolation_method_label, p.normal_state, p.is_active, p.row_version FROM equipment_isolation_points p LEFT JOIN reference_values et ON et.id = p.energy_type_id LEFT JOIN reference_values im ON im.id = p.isolation_method_id";

fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

fn now_ts() -> String {
    crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn map_point(r: &sea_orm::QueryResult) -> Result<EquipmentIsolationPoint, sea_orm::DbErr> {
    Ok(EquipmentIsolationPoint {
        id: r.try_get("", "id")?,
        entity_sync_id: r.try_get("", "entity_sync_id")?,
        equipment_id: r.try_get("", "equipment_id")?,
        point_code: r.try_get("", "point_code")?,
        label: r.try_get("", "label")?,
        energy_type_id: r.try_get("", "energy_type_id")?,
        energy_type_code: r.try_get("", "energy_type_code")?,
        energy_type_label: r.try_get("", "energy_type_label")?,
        isolation_method_id: r.try_get("", "isolation_method_id")?,
        isolation_method_label: r.try_get("", "isolation_method_label")?,
        normal_state: r.try_get("", "normal_state")?,
        is_active: r.try_get::<i64>("", "is_active")? != 0,
        row_version: r.try_get("", "row_version")?,
    })
}

/// The text a permit's isolation gets from a point.
pub fn isolation_point_text(point: &EquipmentIsolationPoint) -> String {
    format!("{} — {}", point.point_code, point.label)
}

/// The points of an equipment, by code (`include_inactive`: deactivated ones too).
pub async fn list_equipment_isolation_points(
    db: &impl ConnectionTrait,
    equipment_id: i64,
    include_inactive: bool,
) -> AppResult<Vec<EquipmentIsolationPoint>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SELECT} WHERE p.equipment_id = ? AND (? OR p.is_active = 1) ORDER BY p.point_code"),
            [equipment_id.into(), include_inactive.into()],
        ))
        .await?;
    rows.iter()
        .map(map_point)
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

async fn get_point(db: &impl ConnectionTrait, id: i64) -> AppResult<EquipmentIsolationPoint> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SELECT} WHERE p.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "EquipmentIsolationPoint".into(),
            id: id.to_string(),
        })?;
    map_point(&row).map_err(AppError::from)
}

fn checked(field: &str, value: &str, max: usize, required_code: &str) -> AppResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(refused(required_code, format!("{field} is required."), &[]));
    }
    if v.chars().count() > max {
        return Err(refused(
            "GATE_ASSET_ISOLATION_POINT_TOO_LONG",
            format!("{field} is longer than {max} characters."),
            &[("max", max.to_string())],
        ));
    }
    Ok(v.to_string())
}

/// Creates or edits a point of an equipment.
pub async fn upsert_equipment_isolation_point(
    db: &DatabaseConnection,
    input: EquipmentIsolationPointUpsertInput,
) -> AppResult<EquipmentIsolationPoint> {
    let txn = db.begin().await?;
    let out = upsert_equipment_isolation_point_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn upsert_equipment_isolation_point_in(
    db: &impl ConnectionTrait,
    input: EquipmentIsolationPointUpsertInput,
) -> AppResult<EquipmentIsolationPoint> {
    let equipment = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [input.equipment_id.into()],
        ))
        .await?;
    if equipment.is_none() {
        return Err(AppError::NotFound {
            entity: "Equipment".into(),
            id: input.equipment_id.to_string(),
        });
    }
    let code = checked(
        "point_code",
        &input.point_code,
        MAX_POINT_CODE_CHARS,
        "GATE_ASSET_ISOLATION_POINT_CODE_REQUIRED",
    )?
    .to_uppercase();
    let label = checked(
        "label",
        &input.label,
        MAX_POINT_LABEL_CHARS,
        "GATE_ASSET_ISOLATION_POINT_LABEL_REQUIRED",
    )?;
    let normal_state = match input.normal_state.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.chars().count() > MAX_NORMAL_STATE_CHARS => {
            return Err(refused(
                "GATE_ASSET_ISOLATION_POINT_TOO_LONG",
                format!("normal_state is longer than {MAX_NORMAL_STATE_CHARS} characters."),
                &[("max", MAX_NORMAL_STATE_CHARS.to_string())],
            ))
        }
        other => other.map(str::to_string),
    };
    let energy_id = require_active_value(db, ENERGY_TYPES_DOMAIN, "energy_type_id", input.energy_type_id).await?;
    let method_id = require_active_value(
        db,
        ISOLATION_METHODS_DOMAIN,
        "isolation_method_id",
        input.isolation_method_id,
    )
    .await?;
    let taken = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_isolation_points WHERE equipment_id = ? AND point_code = ? AND id <> ?",
            [
                input.equipment_id.into(),
                code.clone().into(),
                input.id.unwrap_or(0).into(),
            ],
        ))
        .await?;
    if taken.is_some() {
        return Err(refused(
            "GATE_ASSET_ISOLATION_POINT_CODE_TAKEN",
            format!("Isolation point {code} already exists on this equipment."),
            &[("code", code)],
        ));
    }
    let now = now_ts();
    if let Some(id) = input.id {
        let current = get_point(db, id).await?;
        if current.equipment_id != input.equipment_id {
            return Err(AppError::NotFound {
                entity: "EquipmentIsolationPoint".into(),
                id: id.to_string(),
            });
        }
        if input.expected_row_version != Some(current.row_version) {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on equipment_isolation_points.".into(),
            ]));
        }
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE equipment_isolation_points SET point_code = ?, label = ?, energy_type_id = ?, isolation_method_id = ?, normal_state = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
                [
                    code.into(),
                    label.into(),
                    energy_id.into(),
                    method_id.into(),
                    normal_state.into(),
                    now.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: Concurrent update on equipment_isolation_points.".into(),
            ]));
        }
        return get_point(db, id).await;
    }
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO equipment_isolation_points (entity_sync_id, equipment_id, point_code, label, energy_type_id, isolation_method_id, normal_state, is_active, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 1, 1, ?, ?)",
            [
                Uuid::new_v4().to_string().into(),
                input.equipment_id.into(),
                code.into(),
                label.into(),
                energy_id.into(),
                method_id.into(),
                normal_state.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    get_point(db, crate::db::inserted_id(&inserted)?).await
}

/// Deactivates (or reactivates) a point: it is no longer proposed on permits, its history stays.
pub async fn set_equipment_isolation_point_active(
    db: &impl ConnectionTrait,
    input: EquipmentIsolationPointActiveInput,
) -> AppResult<EquipmentIsolationPoint> {
    let current = get_point(db, input.id).await?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on equipment_isolation_points.".into(),
        ]));
    }
    if current.is_active == input.is_active {
        return Ok(current);
    }
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_isolation_points SET is_active = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
            [
                i64::from(input.is_active).into(),
                now_ts().into(),
                input.id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Concurrent update on equipment_isolation_points.".into(),
        ]));
    }
    get_point(db, input.id).await
}

/// The active points of the permit's equipment, each with the text its isolation gets and whether
/// a live isolation of the permit already has it (a cancelled line never applied does not count).
pub async fn list_proposed_isolation_points(
    db: &impl ConnectionTrait,
    permit_id: i64,
) -> AppResult<Vec<ProposedIsolationPoint>> {
    let permit = crate::permit::queries::get_work_permit(db, permit_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: permit_id.to_string(),
        })?;
    let points = list_equipment_isolation_points(db, permit.asset_id, false).await?;
    let existing: Vec<String> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT isolation_point FROM permit_isolations WHERE permit_id = ? AND {}",
                crate::permit::queries::COUNTED_ISOLATION_SQL
            ),
            [permit_id.into()],
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<String>("", "isolation_point"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(points
        .into_iter()
        .map(|point| {
            let text = isolation_point_text(&point);
            let already_on_permit = existing.iter().any(|e| e.trim() == text);
            ProposedIsolationPoint {
                point,
                isolation_point_text: text,
                already_on_permit,
            }
        })
        .collect())
}
