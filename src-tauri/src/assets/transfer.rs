//! "Transférer" (L2-04): an asset moves to another org node (another line, workshop or site).
//!
//! The transfer is the asset's `MOVED` lifecycle event (from node, to node, effective date, person,
//! reason) with its audit entry, written in one transaction through the governed lifecycle path
//! (`lifecycle::record_lifecycle_event_in`). Before confirming, the screen shows what stays attached
//! to the asset and may need attention at the new place: active preventive plans, spare-parts lines
//! and open work orders. Nothing is stopped or reassigned: the schema has no rule for it, the
//! counts are a warning only.

use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::auth::scope::{check_anchor, scope_filter_sql, AccessScope, Anchor};
use crate::errors::{AppError, AppResult};

use super::identity;
use super::lifecycle::{self, RecordLifecycleEventPayload};

/// Right needed on both the source and the destination node.
pub const TRANSFER_PERMISSION: &str = crate::rbac::permissions::EQ_MANAGE;

/// Payload of "Transférer".
#[derive(Debug, Clone, Deserialize)]
pub struct TransferAssetPayload {
    pub asset_id: i64,
    pub to_org_node_id: i64,
    /// Effective date `YYYY-MM-DD`; `None` is today. Never in the future.
    pub effective_date: Option<String>,
    /// Why the asset moves (free text: no reference list of transfer reasons exists). Required.
    pub reason: String,
    /// The asset's row version the person saw.
    pub expected_row_version: i64,
}

/// What stays attached to the asset when it moves (counts of live records only).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetTransferImpact {
    /// Preventive plans on this asset with lifecycle status `active`.
    pub active_pm_plan_count: i64,
    /// Active spare-parts lines of the asset (its BOM).
    pub spare_part_line_count: i64,
    /// Of those, the lines marked as critical spares.
    pub critical_spare_part_count: i64,
    /// Work orders on the asset not yet closed (`wo::backlog::NOT_CLOSED_SQL`).
    pub open_wo_count: i64,
}

/// An org node the asset can be transferred to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferDestination {
    pub id: i64,
    pub code: String,
    pub name: String,
    /// The node's level in the plant structure (Site, Ligne, Atelier…), from the structure model.
    pub node_type_label: String,
}

/// What the "Transférer" dialog shows before confirming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTransferPreview {
    pub asset_id: i64,
    pub from_org_node_id: Option<i64>,
    pub impact: AssetTransferImpact,
    /// Active nodes of the active structure that host assets, where the person holds the right,
    /// other than the current one.
    pub destinations: Vec<TransferDestination>,
}

/// Result of a transfer.
#[derive(Debug, Clone, Serialize)]
pub struct TransferAssetResult {
    pub asset: identity::Asset,
    pub event: lifecycle::AssetLifecycleEvent,
    pub impact: AssetTransferImpact,
}

fn refuse(message: String) -> AppError {
    AppError::ValidationFailed(vec![message])
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "asset transfer decode failed for column '{column}': {e}"
    ))
}

struct AssetState {
    row_version: i64,
    status: String,
    node_id: Option<i64>,
}

async fn asset_state(conn: &impl ConnectionTrait, asset_id: i64) -> AppResult<AssetState> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, lifecycle_status, installed_at_node_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    Ok(AssetState {
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        status: row
            .try_get("", "lifecycle_status")
            .map_err(|e| decode_err("lifecycle_status", e))?,
        node_id: row
            .try_get("", "installed_at_node_id")
            .map_err(|e| decode_err("installed_at_node_id", e))?,
    })
}

/// Counts of what stays attached to the asset.
pub async fn transfer_impact(conn: &impl ConnectionTrait, asset_id: i64) -> AppResult<AssetTransferImpact> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT (SELECT COUNT(*) FROM pm_plans WHERE LOWER(asset_scope_type) = 'equipment' AND asset_scope_id = ? AND lifecycle_status = 'active') AS pm_cnt, (SELECT COUNT(*) FROM equipment_spare_parts WHERE equipment_id = ? AND is_active = 1) AS sp_cnt, (SELECT COUNT(*) FROM equipment_spare_parts WHERE equipment_id = ? AND is_active = 1 AND is_critical_spare = 1) AS sp_crit_cnt, (SELECT COUNT(*) FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id WHERE wo.equipment_id = ? AND {}) AS wo_cnt",
                crate::wo::backlog::NOT_CLOSED_SQL
            ),
            [asset_id.into(), asset_id.into(), asset_id.into(), asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("transfer impact counts returned no row")))?;
    let get = |column: &str| row.try_get::<i64>("", column).map_err(|e| decode_err(column, e));
    Ok(AssetTransferImpact {
        active_pm_plan_count: get("pm_cnt")?,
        spare_part_line_count: get("sp_cnt")?,
        critical_spare_part_count: get("sp_crit_cnt")?,
        open_wo_count: get("wo_cnt")?,
    })
}

/// Whether the node's level can host equipment (`org_node_types.can_host_assets`), the rule the
/// organisation screen applies when it assigns equipment to a node.
async fn node_hosts_assets(conn: &impl ConnectionTrait, node_id: i64) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.can_host_assets AS can_host_assets FROM org_nodes n JOIN org_node_types t ON t.id = n.node_type_id WHERE n.id = ? AND n.deleted_at IS NULL",
            [node_id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(r
            .try_get::<i64>("", "can_host_assets")
            .map_err(|e| decode_err("can_host_assets", e))?
            != 0),
        None => Ok(false),
    }
}

/// The nodes `scope` allows transferring to (see [`AssetTransferPreview::destinations`]).
async fn destinations(
    conn: &impl ConnectionTrait,
    scope: &AccessScope,
    current: Option<i64>,
) -> AppResult<Vec<TransferDestination>> {
    let (scope_sql, scope_values) = scope_filter_sql(scope, "n.id");
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT n.id, n.code, n.name, t.label AS node_type_label FROM org_nodes n JOIN org_node_types t ON t.id = n.node_type_id WHERE n.deleted_at IS NULL AND n.status = 'active' AND t.can_host_assets = 1 AND n.structure_model_id = (SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1) AND {scope_sql} ORDER BY n.ancestor_path ASC, n.name ASC"
            ),
            scope_values,
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        if Some(id) == current {
            continue;
        }
        out.push(TransferDestination {
            id,
            code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
            name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
            node_type_label: row
                .try_get("", "node_type_label")
                .map_err(|e| decode_err("node_type_label", e))?,
        });
    }
    Ok(out)
}

/// What the dialog shows: the counts and the destinations. The person must hold the right where
/// the asset is now.
pub async fn preview_asset_transfer(
    db: &DatabaseConnection,
    asset_id: i64,
    scope: &AccessScope,
) -> AppResult<AssetTransferPreview> {
    let asset = asset_state(db, asset_id).await?;
    check_anchor(db, scope, TRANSFER_PERMISSION, Anchor::of(asset.node_id)).await?;
    Ok(AssetTransferPreview {
        asset_id,
        from_org_node_id: asset.node_id,
        impact: transfer_impact(db, asset_id).await?,
        destinations: destinations(db, scope, asset.node_id).await?,
    })
}

/// "Transférer": checks, then the `MOVED` lifecycle event (move + audit) in one transaction.
pub async fn transfer_asset(
    db: &DatabaseConnection,
    payload: TransferAssetPayload,
    scope: &AccessScope,
    actor_id: i32,
) -> AppResult<TransferAssetResult> {
    let txn = db.begin().await?;
    let (event, impact) = transfer_asset_in(&txn, payload, scope, actor_id).await?;
    let sql = format!(
        "SELECT {} {} WHERE e.id = ?",
        identity::ASSET_SELECT,
        identity::ASSET_FROM
    );
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [event.asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("equipment {} not found after transfer", event.asset_id)))?;
    let asset = identity::map_asset(&row)?;
    txn.commit().await?;
    tracing::info!(asset_id = asset.id, event_id = event.id, "asset transferred");
    Ok(TransferAssetResult { asset, event, impact })
}

async fn transfer_asset_in(
    txn: &DatabaseTransaction,
    payload: TransferAssetPayload,
    scope: &AccessScope,
    actor_id: i32,
) -> AppResult<(lifecycle::AssetLifecycleEvent, AssetTransferImpact)> {
    let asset_id = payload.asset_id;
    let to_node = payload.to_org_node_id;

    // ── The asset: in the person's sites, version seen, not retired, has a location ──
    let asset = asset_state(txn, asset_id).await?;
    check_anchor(txn, scope, TRANSFER_PERMISSION, Anchor::of(asset.node_id)).await?;
    if asset.row_version != payload.expected_row_version {
        return Err(refuse(format!(
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : version attendue {}, version actuelle {}.",
            payload.expected_row_version, asset.row_version
        )));
    }
    let status = asset.status;
    if identity::is_retired_status_code(txn, &status).await? {
        return Err(refuse(format!(
            "GATE_ASSET_TRANSFER_RETIRED:status={status}: L'équipement a le statut '{status}' et ne peut pas être transféré."
        )));
    }
    let Some(from_node) = asset.node_id.filter(|id| *id > 0) else {
        return Err(refuse(
            "GATE_ASSET_TRANSFER_NO_ORIGIN: L'équipement n'a pas d'emplacement : affectez-le depuis l'écran Organisation."
                .into(),
        ));
    };

    // ── Reason ───────────────────────────────────────────────────────────
    let reason = payload.reason.trim().to_string();
    if reason.is_empty() {
        return Err(refuse(
            "GATE_ASSET_TRANSFER_REASON_REQUIRED: Le motif du transfert est obligatoire.".into(),
        ));
    }

    // ── Destination: another node, in the person's sites, active, hosts assets ──
    if to_node == from_node {
        return Err(refuse(
            "GATE_ASSET_TRANSFER_SAME_NODE: L'équipement est déjà à cet emplacement.".into(),
        ));
    }
    check_anchor(txn, scope, TRANSFER_PERMISSION, Anchor::of(Some(to_node))).await?;
    identity::assert_org_node_active(txn, to_node).await?;
    if !node_hosts_assets(txn, to_node).await? {
        return Err(refuse(
            "GATE_ASSET_TRANSFER_TARGET_CANNOT_HOST: Ce niveau de l'organisation ne peut pas recevoir d'équipement."
                .into(),
        ));
    }

    // ── Effective date: readable, not in the future, not before the last move ──
    let now = crate::clock::now();
    let today = crate::clock::now_local().date_naive();
    let date = match payload.effective_date.as_deref().map(str::trim) {
        None | Some("") => today,
        Some(raw) => chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
            refuse(format!(
                "GATE_ASSET_TRANSFER_DATE_INVALID: Date d'effet illisible '{raw}' (attendu AAAA-MM-JJ)."
            ))
        })?,
    };
    if date > today {
        return Err(refuse(format!(
            "GATE_ASSET_TRANSFER_DATE_FUTURE: La date d'effet {date} est dans le futur."
        )));
    }
    let mut effective_at = if date == today {
        now
    } else {
        date.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
    };
    let last_move: Option<String> = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MAX(occurred_at) AS last_at FROM equipment_lifecycle_events WHERE equipment_id = ? AND event_type = 'MOVED'",
            [asset_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<Option<String>>("", "last_at"))
        .transpose()
        .map_err(|e| decode_err("last_at", e))?
        .flatten();
    if let Some(last_at) = last_move.as_deref() {
        let last_on = last_at.get(..10).unwrap_or(last_at);
        if let Ok(last_date) = chrono::NaiveDate::parse_from_str(last_on, "%Y-%m-%d") {
            if date < last_date {
                return Err(refuse(format!(
                    "GATE_ASSET_TRANSFER_DATE_BEFORE_LAST_MOVE:moved_on={last_date}: La date d'effet {date} précède le dernier déplacement de l'équipement, le {last_date}."
                )));
            }
        }
        // Same day as the last move: this one is not stamped before it.
        if let Ok(last) = chrono::DateTime::parse_from_rfc3339(last_at) {
            let last = last.with_timezone(&chrono::Utc);
            if effective_at < last {
                effective_at = last;
            }
        }
    }

    // ── What stays attached (returned with the result, as shown before confirming) ──
    let impact = transfer_impact(txn, asset_id).await?;

    // ── The move: lifecycle event (from, to, date, person, reason) + audit ──
    let event = lifecycle::record_lifecycle_event_in(
        txn,
        RecordLifecycleEventPayload {
            asset_id,
            event_type: "MOVED".into(),
            event_at: Some(effective_at.to_rfc3339()),
            to_org_node_id: Some(to_node),
            notes: Some(reason),
            expected_row_version: Some(payload.expected_row_version),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok((event, impact))
}
