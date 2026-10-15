//! Cost of one work order (OT): estimated vs planned vs committed vs actual, by cost category.
//!
//! Benchmark: SAP PM order cost report (value categories, estimated / planned / actual) and Maximo
//! work-order cost roll-up. Every figure is read from the record that carries it — nothing is
//! typed twice:
//!
//! | category | estimated            | planned                                          | actual                                                                     |
//! |----------|----------------------|--------------------------------------------------|----------------------------------------------------------------------------|
//! | labour   | `wo_cost_estimates`  | expected hours × regular rate of the responsible | Σ intervener hours × the rate stored on the entry (overtime split)         |
//! | parts    | `wo_cost_estimates`  | Σ planned qty × planned/known unit cost          | inventory ledger ISSUE − RETURN at weighted-average cost (+ non-stock parts used on the OT) |
//! | services | `wo_cost_estimates`  | Σ ordered qty × PO price on lines for this OT    | Σ accepted receipts × tenant unit cost on PO lines whose demand is this OT |
//! | other    | `wo_cost_estimates`  | —                                                | `work_orders.service_cost_input` (cost entered on the OT)                  |
//!
//! Committed = open quantity × price of approved PO lines raised for this OT, plus approved
//! requisition lines for this OT that are not on a PO yet.
//!
//! A purchase line whose demand is the OT itself (`WORK_ORDER`) is a service / contractor line:
//! its receipt is the actual. If that receipt is auto-issued to the OT through the PO line's
//! reservation, the issue is excluded from parts so the cost is never counted twice.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Source types that tie a stock reservation or ledger movement to a work order.
pub(crate) const WO_SOURCE_TYPES_SQL: &str = "'WO', 'PM_WO', 'WORK_ORDER', 'WORK_ORDER_PART', 'work_order'";

pub const BUCKET_LABOR: &str = "labor";
pub const BUCKET_PARTS: &str = "parts";
pub const BUCKET_SERVICES: &str = "services";
pub const BUCKET_OTHER: &str = "other";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WoCostFigures {
    /// From the OT estimate; None when no estimate was entered.
    pub estimated: Option<f64>,
    /// From the OT plan; None when the plan does not say (e.g. no expected hours or no rate).
    pub planned: Option<f64>,
    pub committed: f64,
    pub actual: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoLaborCostLine {
    pub intervener_entry_id: i64,
    pub personnel_id: Option<i64>,
    pub person_name: Option<String>,
    pub skill_label: Option<String>,
    pub hours: f64,
    pub hourly_rate: Option<f64>,
    /// `entry`: the rate stored when the time was booked; `rate_card`: no rate on the entry, the
    /// person's rate card in force on the work date (regular or overtime rate); None: no rate.
    /// On a settled OT an unchanged entry keeps the rate frozen by the settlement.
    pub rate_source: Option<String>,
    pub is_overtime: bool,
    pub amount: f64,
    /// Sync identity of the labour entry (settlement snapshot key).
    #[serde(skip)]
    pub entry_sync_id: String,
    /// Rate typed on the entry itself, if any.
    #[serde(skip)]
    pub entry_rate: Option<f64>,
}

/// Labour rate frozen by a posted settlement for one entry (`wo_cost_settlements.labor_snapshot_json`).
/// While the settlement stands, the entry is priced at `rate` as long as its hours, its own rate
/// and its overtime flag are unchanged: a later rate-card edit never reprices work already done.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaborRateSnapshot {
    pub entry: String,
    pub hours: f64,
    pub entry_rate: Option<f64>,
    pub is_overtime: bool,
    pub rate: f64,
    pub source: String,
}

/// Snapshot of the priced labour lines, for the settlement.
pub fn labor_rate_snapshot(lines: &[WoLaborCostLine]) -> Vec<LaborRateSnapshot> {
    lines
        .iter()
        .filter_map(|l| {
            Some(LaborRateSnapshot {
                entry: l.entry_sync_id.clone(),
                hours: l.hours,
                entry_rate: l.entry_rate,
                is_overtime: l.is_overtime,
                rate: l.hourly_rate?,
                source: l.rate_source.clone()?,
            })
        })
        .collect()
}

fn same_amount(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
        (None, None) => true,
        _ => false,
    }
}

/// Snapshot of the OT's posted settlement, by entry.
async fn frozen_labor_rates<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<BTreeMap<String, LaborRateSnapshot>> {
    if !table_has_column(db, "wo_cost_settlements", "labor_snapshot_json").await? {
        return Ok(BTreeMap::new());
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT labor_snapshot_json FROM wo_cost_settlements
             WHERE work_order_id = ? AND status = 'posted' ORDER BY id DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    let json: Option<String> = match row {
        Some(r) => decode(&r, "labor_snapshot_json")?,
        None => None,
    };
    let snapshot: Vec<LaborRateSnapshot> = match json {
        Some(j) => serde_json::from_str(&j)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("labour snapshot of work order {wo_id}: {e}")))?,
        None => Vec::new(),
    };
    Ok(snapshot.into_iter().map(|s| (s.entry.clone(), s)).collect())
}

/// Regular and overtime rates of the person's rate card in force on `date` (YYYY-MM-DD).
async fn rate_card_on<C: ConnectionTrait>(db: &C, personnel_id: i64, date: &str) -> AppResult<Option<(f64, f64)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT labor_rate, overtime_rate FROM personnel_rate_cards
             WHERE personnel_id = ? AND date(effective_from) <= date(?)
             ORDER BY effective_from DESC, id DESC LIMIT 1",
            [personnel_id.into(), date.into()],
        ))
        .await?;
    row.map(|r| Ok((decode(&r, "labor_rate")?, decode(&r, "overtime_rate")?)))
        .transpose()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoPartsCostLine {
    /// None for a free-text part used on the OT without a stock article.
    pub article_id: Option<i64>,
    /// True when the quantity left a store through the stock ledger; false for a part used on
    /// the OT outside stock (free text or no store location), valued at the cost entered on the OT.
    pub stock_backed: bool,
    pub article_code: String,
    pub article_name: String,
    pub issued_qty: f64,
    pub returned_qty: f64,
    pub net_qty: f64,
    /// Issues minus returns, at the cost stamped on each movement.
    pub amount: f64,
    /// Net quantity whose movements carry no cost (article never costed at that location).
    pub unvalued_qty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoPurchaseCostLine {
    pub purchase_order_id: i64,
    pub po_number: String,
    pub po_status: String,
    pub po_line_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub supplier_name: Option<String>,
    /// `WORK_ORDER` (service / contractor, bucket services) or `WORK_ORDER_PART` (material, bucket parts).
    pub demand_source_type: String,
    pub ordered_qty: f64,
    pub received_qty: f64,
    /// PO price converted to the tenant currency (None when no rate is known).
    pub unit_price_tenant: Option<f64>,
    pub ordered_amount: Option<f64>,
    /// Receipts accepted, at the tenant unit cost stamped on each receipt line.
    pub received_amount: f64,
    pub open_commitment: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WoCostEstimate {
    pub id: i64,
    pub work_order_id: i64,
    pub cost_center_id: Option<i64>,
    pub estimated_labor: f64,
    pub estimated_parts: f64,
    pub estimated_services: f64,
    pub estimated_other: f64,
    pub currency_code: String,
    pub notes: Option<String>,
    pub row_version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedCostCenter {
    pub cost_center_id: i64,
    pub code: String,
    pub name: String,
    /// `estimate_override` | `asset` | `asset_location` | `work_order_entity`
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoCostBreakdown {
    pub work_order_id: i64,
    pub work_order_code: String,
    pub status_code: String,
    pub equipment_id: Option<i64>,
    pub currency_code: String,
    pub labor: WoCostFigures,
    pub parts: WoCostFigures,
    pub services: WoCostFigures,
    pub other: WoCostFigures,
    pub total: WoCostFigures,
    /// Part of `labor.actual` worked as overtime.
    pub labor_overtime_amount: f64,
    /// False until labour entries carry an overtime flag (personnel migration); overtime is then
    /// shown as not captured rather than as zero.
    pub overtime_captured: bool,
    pub labor_hours: f64,
    /// Hours on entries without a rate: not costed, shown so they get a rate card.
    pub unpriced_labor_hours: f64,
    pub unvalued_parts_qty: f64,
    /// Why `labor.planned` is empty, when it is: `no_expected_hours` | `no_responsible` | `no_rate_card`.
    pub planned_labor_gap: Option<String>,
    pub labor_lines: Vec<WoLaborCostLine>,
    pub parts_lines: Vec<WoPartsCostLine>,
    pub purchase_lines: Vec<WoPurchaseCostLine>,
    pub estimate: Option<WoCostEstimate>,
    pub cost_center: Option<ResolvedCostCenter>,
}

pub(crate) fn decode<T: sea_orm::TryGetable>(row: &QueryResult, col: &str) -> AppResult<T> {
    row.try_get::<T>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("decode {col}: {e}")))
}

pub(crate) async fn table_has_column<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> AppResult<bool> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    Ok(rows
        .iter()
        .any(|r| r.try_get::<String>("", "name").map(|n| n == column).unwrap_or(false)))
}

pub(crate) fn round_money(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub async fn get_wo_cost_estimate<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Option<WoCostEstimate>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, cost_center_id, estimated_labor, estimated_parts, estimated_services,
                    estimated_other, currency_code, notes, row_version, updated_at
             FROM wo_cost_estimates WHERE work_order_id = ? ORDER BY id DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    row.map(|r| {
        Ok(WoCostEstimate {
            id: decode(&r, "id")?,
            work_order_id: decode(&r, "work_order_id")?,
            cost_center_id: decode(&r, "cost_center_id")?,
            estimated_labor: decode(&r, "estimated_labor")?,
            estimated_parts: decode(&r, "estimated_parts")?,
            estimated_services: decode(&r, "estimated_services")?,
            estimated_other: decode(&r, "estimated_other")?,
            currency_code: decode(&r, "currency_code")?,
            notes: decode(&r, "notes")?,
            row_version: decode(&r, "row_version")?,
            updated_at: decode(&r, "updated_at")?,
        })
    })
    .transpose()
}

async fn cost_center_by<C: ConnectionTrait>(
    db: &C,
    where_sql: &str,
    value: sea_orm::Value,
    source: &str,
) -> AppResult<Option<ResolvedCostCenter>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT id, code, name FROM cost_centers WHERE {where_sql} AND is_active = 1"),
            [value],
        ))
        .await?;
    row.map(|r| {
        Ok(ResolvedCostCenter {
            cost_center_id: decode(&r, "id")?,
            code: decode(&r, "code")?,
            name: decode(&r, "name")?,
            source: source.to_string(),
        })
    })
    .transpose()
}

async fn cost_center_by_code<C: ConnectionTrait>(
    db: &C,
    code: Option<String>,
    source: &str,
) -> AppResult<Option<ResolvedCostCenter>> {
    match code.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
        Some(code) => cost_center_by(db, "code = ?", code.into(), source).await,
        None => Ok(None),
    }
}

/// Cost centre an OT settles to, in order: the OT estimate's override, the asset's own cost
/// centre, the nearest org node above the asset's location carrying one, then the nearest org
/// node above the OT's entity carrying one (or a cost centre attached to that node).
pub async fn resolve_wo_cost_center<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Option<ResolvedCostCenter>> {
    if let Some(cc) = get_wo_cost_estimate(db, wo_id).await?.and_then(|e| e.cost_center_id) {
        if let Some(found) = cost_center_by(db, "id = ?", cc.into(), "estimate_override").await? {
            return Ok(Some(found));
        }
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.cost_center_code AS asset_cc,
                    (SELECT a.cost_center_code FROM org_nodes a, org_nodes n
                      WHERE n.id = e.installed_at_node_id
                        AND n.ancestor_path LIKE a.ancestor_path || '%'
                        AND a.deleted_at IS NULL
                        AND TRIM(COALESCE(a.cost_center_code, '')) <> ''
                      ORDER BY a.depth DESC LIMIT 1) AS location_cc,
                    (SELECT a.cost_center_code FROM org_nodes a, org_nodes n
                      WHERE n.id = wo.entity_id
                        AND n.ancestor_path LIKE a.ancestor_path || '%'
                        AND a.deleted_at IS NULL
                        AND TRIM(COALESCE(a.cost_center_code, '')) <> ''
                      ORDER BY a.depth DESC LIMIT 1) AS entity_cc,
                    (SELECT c.id FROM cost_centers c, org_nodes a, org_nodes n
                      WHERE n.id = wo.entity_id
                        AND n.ancestor_path LIKE a.ancestor_path || '%'
                        AND c.entity_id = a.id AND c.is_active = 1
                      ORDER BY a.depth DESC, c.id ASC LIMIT 1) AS entity_cc_id
             FROM work_orders wo
             LEFT JOIN equipment e ON e.id = wo.equipment_id
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    if let Some(found) = cost_center_by_code(db, decode(&row, "asset_cc")?, "asset").await? {
        return Ok(Some(found));
    }
    if let Some(found) = cost_center_by_code(db, decode(&row, "location_cc")?, "asset_location").await? {
        return Ok(Some(found));
    }
    if let Some(found) = cost_center_by_code(db, decode(&row, "entity_cc")?, "work_order_entity").await? {
        return Ok(Some(found));
    }
    if let Some(id) = decode::<Option<i64>>(&row, "entity_cc_id")? {
        return cost_center_by(db, "id = ?", id.into(), "work_order_entity").await;
    }
    Ok(None)
}

async fn labor_lines<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<(Vec<WoLaborCostLine>, bool)> {
    let has_overtime = table_has_column(db, "work_order_interveners", "is_overtime").await?;
    let has_personnel = table_has_column(db, "work_order_interveners", "personnel_id").await?;
    let overtime_expr = if has_overtime {
        "COALESCE(woi.is_overtime, 0)"
    } else {
        "0"
    };
    let personnel_expr = if has_personnel {
        "COALESCE(woi.personnel_id, ua.personnel_id)"
    } else {
        "ua.personnel_id"
    };
    let sql = format!(
        "SELECT woi.id, COALESCE(woi.entity_sync_id, 'woi-' || woi.id) AS entry_sync_id,
                COALESCE(woi.hours_worked, 0.0) AS hours, woi.hourly_rate,
                substr(COALESCE(woi.started_at, woi.ended_at), 1, 10) AS work_date,
                {overtime_expr} AS is_overtime, {personnel_expr} AS personnel_id,
                p.full_name AS person_name, rv.label AS skill_label
         FROM work_order_interveners woi
         LEFT JOIN user_accounts ua ON ua.id = woi.intervener_id
         LEFT JOIN personnel p ON p.id = {personnel_expr}
         LEFT JOIN reference_values rv ON rv.id = woi.skill_id
         WHERE woi.work_order_id = ?
         ORDER BY woi.id"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [wo_id.into()]))
        .await?;
    let frozen = frozen_labor_rates(db, wo_id).await?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut lines = Vec::with_capacity(rows.len());
    for r in rows {
        let entry_sync_id: String = decode(&r, "entry_sync_id")?;
        let hours: f64 = decode(&r, "hours")?;
        let is_overtime = decode::<i64>(&r, "is_overtime")? != 0;
        let personnel_id: Option<i64> = decode(&r, "personnel_id")?;
        let entry_rate: Option<f64> = decode::<Option<f64>>(&r, "hourly_rate")?.filter(|v| *v > 0.0);
        let work_date: Option<String> = decode(&r, "work_date")?;
        // Standard costing: an entry of a settled OT keeps the rate frozen by the settlement while
        // it is unchanged. Otherwise the rate typed on the entry, else the person's rate card in
        // force on the work date (never a card created or edited later); else unpriced.
        let snapshot = frozen.get(&entry_sync_id).filter(|s| {
            (s.hours - hours).abs() < 1e-9 && same_amount(s.entry_rate, entry_rate) && s.is_overtime == is_overtime
        });
        let (rate, rate_source) = match (snapshot, entry_rate) {
            (Some(s), _) => (Some(s.rate), Some(s.source.clone())),
            (None, Some(rate)) => (Some(rate), Some("entry".to_string())),
            (None, None) => match personnel_id {
                Some(pid) => match rate_card_on(db, pid, work_date.as_deref().unwrap_or(&today)).await? {
                    Some((regular, overtime)) => {
                        let rate = if is_overtime { overtime } else { regular };
                        if rate > 0.0 {
                            (Some(rate), Some("rate_card".to_string()))
                        } else {
                            (None, None)
                        }
                    }
                    None => (None, None),
                },
                None => (None, None),
            },
        };
        lines.push(WoLaborCostLine {
            intervener_entry_id: decode(&r, "id")?,
            personnel_id,
            person_name: decode(&r, "person_name")?,
            skill_label: decode(&r, "skill_label")?,
            hours,
            hourly_rate: rate,
            rate_source,
            is_overtime,
            amount: round_money(hours * rate.unwrap_or(0.0)),
            entry_sync_id,
            entry_rate,
        });
    }
    Ok((lines, has_overtime))
}

async fn parts_lines<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Vec<WoPartsCostLine>> {
    let sql = format!(
        "SELECT t.article_id, a.article_code, a.article_name, t.movement_type,
                ABS(t.quantity) AS qty, t.extended_cost
         FROM inventory_transactions t
         JOIN articles a ON a.id = t.article_id
         WHERE t.movement_type IN ('ISSUE', 'RETURN')
           AND ((t.source_type IN ({WO_SOURCE_TYPES_SQL}) AND t.source_id = ?1)
                OR t.reservation_id IN (SELECT id FROM stock_reservations
                                        WHERE source_type IN ({WO_SOURCE_TYPES_SQL}) AND source_id = ?1))
           AND (t.reservation_id IS NULL OR t.reservation_id NOT IN (
                SELECT source_reservation_id FROM purchase_order_lines
                 WHERE demand_source_type = 'WORK_ORDER' AND demand_source_id = ?1
                   AND source_reservation_id IS NOT NULL))
         ORDER BY t.id"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [wo_id.into()]))
        .await?;
    let mut by_article: BTreeMap<i64, WoPartsCostLine> = BTreeMap::new();
    for r in rows {
        let article_id: i64 = decode(&r, "article_id")?;
        let movement: String = decode(&r, "movement_type")?;
        let qty: f64 = decode(&r, "qty")?;
        let cost: Option<f64> = decode(&r, "extended_cost")?;
        let sign = if movement == "ISSUE" { 1.0 } else { -1.0 };
        let line = match by_article.entry(article_id) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert(WoPartsCostLine {
                article_id: Some(article_id),
                stock_backed: true,
                article_code: decode(&r, "article_code")?,
                article_name: decode(&r, "article_name")?,
                issued_qty: 0.0,
                returned_qty: 0.0,
                net_qty: 0.0,
                amount: 0.0,
                unvalued_qty: 0.0,
            }),
        };
        if sign > 0.0 {
            line.issued_qty += qty;
        } else {
            line.returned_qty += qty;
        }
        line.net_qty += sign * qty;
        match cost {
            Some(c) => line.amount += sign * c.abs(),
            None => line.unvalued_qty += sign * qty,
        }
    }
    let mut lines: Vec<WoPartsCostLine> = by_article
        .into_values()
        .map(|mut l| {
            l.amount = round_money(l.amount);
            l.unvalued_qty = l.unvalued_qty.max(0.0);
            l
        })
        .collect();

    // Parts used on the OT that never went through a store (free-text part, or article without
    // a store location): no ledger movement exists, the OT's used quantity × unit cost is the cost.
    let direct = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wop.article_id, COALESCE(a.article_code, wop.article_ref, '') AS article_code,
                    COALESCE(a.article_name, wop.article_ref, '') AS article_name,
                    wop.quantity_used AS qty, wop.unit_cost
             FROM work_order_parts wop
             LEFT JOIN articles a ON a.id = wop.article_id
             WHERE wop.work_order_id = ?
               AND wop.reservation_id IS NULL
               AND (wop.article_id IS NULL OR wop.stock_location_id IS NULL)
               AND COALESCE(wop.quantity_used, 0) > 0
             ORDER BY wop.id",
            [wo_id.into()],
        ))
        .await?;
    for r in direct {
        let qty: f64 = decode(&r, "qty")?;
        let unit_cost: Option<f64> = decode(&r, "unit_cost")?;
        lines.push(WoPartsCostLine {
            article_id: decode(&r, "article_id")?,
            stock_backed: false,
            article_code: decode(&r, "article_code")?,
            article_name: decode(&r, "article_name")?,
            issued_qty: qty,
            returned_qty: 0.0,
            net_qty: qty,
            amount: round_money(qty * unit_cost.unwrap_or(0.0)),
            unvalued_qty: if unit_cost.is_some() { 0.0 } else { qty },
        });
    }
    Ok(lines)
}

/// PO lines raised for this OT, with what was received and what is still open.
async fn purchase_lines<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Vec<WoPurchaseCostLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pol.id AS po_line_id, po.id AS po_id, po.po_number, po.status AS po_status,
                    po.currency_code AS po_currency, a.article_code, a.article_name,
                    s.name AS supplier_name,
                    pol.demand_source_type, pol.ordered_qty, pol.received_qty, pol.unit_price,
                    (SELECT COALESCE(SUM(grl.accepted_qty * COALESCE(grl.unit_cost_tenant, 0.0)), 0.0)
                       FROM goods_receipt_lines grl WHERE grl.po_line_id = pol.id) AS received_amount,
                    (SELECT grl.unit_cost_tenant / grl.unit_price FROM goods_receipt_lines grl
                      WHERE grl.po_line_id = pol.id AND grl.unit_cost_tenant IS NOT NULL
                        AND COALESCE(grl.unit_price, 0) > 0
                      ORDER BY grl.id DESC LIMIT 1) AS receipt_rate
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             JOIN articles a ON a.id = pol.article_id
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             WHERE pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND pol.demand_source_id = ?
               AND po.status <> 'CANCELLED'
             ORDER BY po.id, pol.id",
            [wo_id.into()],
        ))
        .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let tenant = crate::inventory::fx::tenant_currency(db).await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut lines = Vec::with_capacity(rows.len());
    for r in rows {
        let po_currency: Option<String> = decode(&r, "po_currency")?;
        let receipt_rate: Option<f64> = decode(&r, "receipt_rate")?;
        let rate = match po_currency.as_deref() {
            None => Some(1.0),
            Some(c) if c == tenant => Some(1.0),
            Some(c) => match receipt_rate {
                Some(r) if r > 0.0 => Some(r),
                _ => crate::inventory::fx::rate_to_tenant(db, Some(c), &today).await?,
            },
        };
        let unit_price: Option<f64> = decode(&r, "unit_price")?;
        let unit_price_tenant = match (unit_price, rate) {
            (Some(p), Some(rt)) => Some(p * rt),
            _ => None,
        };
        let ordered_qty: f64 = decode(&r, "ordered_qty")?;
        let received_qty: f64 = decode(&r, "received_qty")?;
        let po_status: String = decode(&r, "po_status")?;
        let committed_status = po_status == "APPROVED" || po_status == "PARTIALLY_RECEIVED";
        let open_qty = (ordered_qty - received_qty).max(0.0);
        lines.push(WoPurchaseCostLine {
            purchase_order_id: decode(&r, "po_id")?,
            po_number: decode(&r, "po_number")?,
            po_status,
            po_line_id: decode(&r, "po_line_id")?,
            article_code: decode(&r, "article_code")?,
            article_name: decode(&r, "article_name")?,
            supplier_name: decode(&r, "supplier_name")?,
            demand_source_type: decode(&r, "demand_source_type")?,
            ordered_qty,
            received_qty,
            unit_price_tenant,
            ordered_amount: unit_price_tenant.map(|p| round_money(p * ordered_qty)),
            received_amount: round_money(decode(&r, "received_amount")?),
            open_commitment: if committed_status {
                round_money(open_qty * unit_price_tenant.unwrap_or(0.0))
            } else {
                0.0
            },
        });
    }
    Ok(lines)
}

/// Approved requisition lines for this OT that are not on a PO yet (pre-commitment):
/// `(services, parts)`.
async fn requisition_commitments<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<(f64, f64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                COALESCE(SUM(CASE WHEN r.demand_source_type = 'WORK_ORDER'
                    THEN COALESCE(rl.projected_extended_cost, rl.requested_qty * rl.projected_unit_cost, 0) END), 0.0) AS services,
                COALESCE(SUM(CASE WHEN r.demand_source_type = 'WORK_ORDER_PART'
                    THEN COALESCE(rl.projected_extended_cost, rl.requested_qty * rl.projected_unit_cost, 0) END), 0.0) AS parts
             FROM procurement_requisition_lines rl
             JOIN procurement_requisitions r ON r.id = rl.requisition_id
             WHERE r.status = 'APPROVED'
               AND r.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND r.demand_source_id = ?
               AND NOT EXISTS (SELECT 1 FROM purchase_order_lines pol WHERE pol.requisition_line_id = rl.id)",
            [wo_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => (decode(&r, "services")?, decode(&r, "parts")?),
        None => (0.0, 0.0),
    })
}

/// Planned labour = expected hours × regular rate of the OT's responsible technician.
async fn planned_labor<C: ConnectionTrait>(
    db: &C,
    expected_hours: Option<f64>,
    responsible_user_id: Option<i64>,
) -> AppResult<(Option<f64>, Option<String>)> {
    let Some(hours) = expected_hours.filter(|h| *h > 0.0) else {
        return Ok((None, Some("no_expected_hours".to_string())));
    };
    let Some(user_id) = responsible_user_id else {
        return Ok((None, Some("no_responsible".to_string())));
    };
    let personnel_id: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "personnel_id").ok().flatten());
    let Some(personnel_id) = personnel_id else {
        return Ok((None, Some("no_rate_card".to_string())));
    };
    match crate::personnel::queries::resolve_active_labor_rate(db, personnel_id).await? {
        Some(rate) if rate > 0.0 => Ok((Some(round_money(hours * rate)), None)),
        _ => Ok((None, Some("no_rate_card".to_string()))),
    }
}

async fn planned_parts<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n,
                    SUM(COALESCE(wop.quantity_planned, 0) *
                        COALESCE(wop.planned_unit_cost, wop.unit_cost,
                                 (SELECT AVG(sb.moving_avg_unit_cost) FROM stock_balances sb
                                   WHERE sb.article_id = wop.article_id AND sb.moving_avg_unit_cost IS NOT NULL), 0)) AS amount
             FROM work_order_parts wop
             WHERE wop.work_order_id = ? AND COALESCE(wop.origin, 'planned') = 'planned'",
            [wo_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) if decode::<i64>(&r, "n")? > 0 => decode::<Option<f64>>(&r, "amount")?.map(round_money),
        _ => None,
    })
}

fn sum_optional(values: [Option<f64>; 4]) -> Option<f64> {
    if values.iter().all(Option::is_none) {
        None
    } else {
        Some(round_money(values.iter().flatten().sum()))
    }
}

/// Full cost picture of one OT. Read-only; usable inside a transaction.
pub async fn compute_wo_cost_breakdown<C: ConnectionTrait>(db: &C, wo_id: i64) -> AppResult<WoCostBreakdown> {
    let head = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.code, COALESCE(s.code, '') AS status_code, wo.equipment_id,
                    wo.expected_duration_hours, wo.primary_responsible_id,
                    COALESCE(wo.service_cost_input, 0.0) AS other_actual
             FROM work_orders wo LEFT JOIN work_order_statuses s ON s.id = wo.status_id
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let currency_code = crate::inventory::fx::tenant_currency(db).await?;
    let (labor_lines, overtime_captured) = labor_lines(db, wo_id).await?;
    let parts_lines = parts_lines(db, wo_id).await?;
    let purchase_lines = purchase_lines(db, wo_id).await?;
    let (req_services, req_parts) = requisition_commitments(db, wo_id).await?;
    let estimate = get_wo_cost_estimate(db, wo_id).await?;
    let (planned_labor, planned_labor_gap) = planned_labor(
        db,
        decode(&head, "expected_duration_hours")?,
        decode(&head, "primary_responsible_id")?,
    )
    .await?;
    let planned_parts = planned_parts(db, wo_id).await?;
    let cost_center = resolve_wo_cost_center(db, wo_id).await?;

    let labor_actual = round_money(labor_lines.iter().map(|l| l.amount).sum());
    let labor_overtime_amount = round_money(labor_lines.iter().filter(|l| l.is_overtime).map(|l| l.amount).sum());
    let labor_hours = labor_lines.iter().map(|l| l.hours).sum();
    let unpriced_labor_hours = labor_lines
        .iter()
        .filter(|l| l.hourly_rate.is_none())
        .map(|l| l.hours)
        .sum();

    let parts_actual = round_money(parts_lines.iter().map(|l| l.amount).sum());
    let unvalued_parts_qty = parts_lines.iter().map(|l| l.unvalued_qty).sum();

    let service_lines: Vec<&WoPurchaseCostLine> = purchase_lines
        .iter()
        .filter(|l| l.demand_source_type == "WORK_ORDER")
        .collect();
    let material_lines: Vec<&WoPurchaseCostLine> = purchase_lines
        .iter()
        .filter(|l| l.demand_source_type == "WORK_ORDER_PART")
        .collect();
    let services_planned = if service_lines.is_empty() {
        None
    } else {
        Some(round_money(service_lines.iter().filter_map(|l| l.ordered_amount).sum()))
    };

    let labor = WoCostFigures {
        estimated: estimate.as_ref().map(|e| e.estimated_labor),
        planned: planned_labor,
        committed: 0.0,
        actual: labor_actual,
    };
    let parts = WoCostFigures {
        estimated: estimate.as_ref().map(|e| e.estimated_parts),
        planned: planned_parts,
        committed: round_money(material_lines.iter().map(|l| l.open_commitment).sum::<f64>() + req_parts),
        actual: parts_actual,
    };
    let services = WoCostFigures {
        estimated: estimate.as_ref().map(|e| e.estimated_services),
        planned: services_planned,
        committed: round_money(service_lines.iter().map(|l| l.open_commitment).sum::<f64>() + req_services),
        actual: round_money(service_lines.iter().map(|l| l.received_amount).sum()),
    };
    let other = WoCostFigures {
        estimated: estimate.as_ref().map(|e| e.estimated_other),
        planned: None,
        committed: 0.0,
        actual: round_money(decode(&head, "other_actual")?),
    };
    let total = WoCostFigures {
        estimated: sum_optional([labor.estimated, parts.estimated, services.estimated, other.estimated]),
        planned: sum_optional([labor.planned, parts.planned, services.planned, other.planned]),
        committed: round_money(labor.committed + parts.committed + services.committed + other.committed),
        actual: round_money(labor.actual + parts.actual + services.actual + other.actual),
    };

    Ok(WoCostBreakdown {
        work_order_id: wo_id,
        work_order_code: decode(&head, "code")?,
        status_code: decode(&head, "status_code")?,
        equipment_id: decode(&head, "equipment_id")?,
        currency_code,
        labor,
        parts,
        services,
        other,
        total,
        labor_overtime_amount,
        overtime_captured,
        labor_hours,
        unpriced_labor_hours,
        unvalued_parts_qty,
        planned_labor_gap,
        labor_lines,
        parts_lines,
        purchase_lines,
        estimate,
        cost_center,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertWoCostEstimateInput {
    pub work_order_id: i64,
    /// Settlement cost-centre override; None = resolved from the asset / organisation.
    pub cost_center_id: Option<i64>,
    pub estimated_labor: f64,
    pub estimated_parts: f64,
    pub estimated_services: f64,
    pub estimated_other: f64,
    pub notes: Option<String>,
    /// Row version of the estimate being edited; None when creating it.
    pub expected_row_version: Option<i64>,
}

/// Creates or updates the OT's estimate (one per OT). Amounts are in the tenant currency.
pub async fn upsert_wo_cost_estimate<C: ConnectionTrait>(
    db: &C,
    actor_id: i64,
    input: UpsertWoCostEstimateInput,
) -> AppResult<WoCostEstimate> {
    let amounts = [
        input.estimated_labor,
        input.estimated_parts,
        input.estimated_services,
        input.estimated_other,
    ];
    if amounts.iter().any(|a| !a.is_finite() || *a < 0.0) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_ESTIMATE_NEGATIVE: estimated amounts must be zero or positive".to_string(),
        ]));
    }
    let status: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT s.code FROM work_orders wo LEFT JOIN work_order_statuses s ON s.id = wo.status_id WHERE wo.id = ?",
            [input.work_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.work_order_id.to_string(),
        })?
        .try_get("", "code")
        .ok();
    if matches!(status.as_deref(), Some("closed" | "cancelled")) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_ESTIMATE_LOCKED: the estimate of a closed or cancelled work order can no longer be changed"
                .to_string(),
        ]));
    }
    if let Some(cc) = input.cost_center_id {
        let active = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM cost_centers WHERE id = ? AND is_active = 1",
                [cc.into()],
            ))
            .await?;
        if active.is_none() {
            return Err(AppError::ValidationFailed(vec![
                "GATE_FIN_COST_CENTER_INACTIVE: unknown or inactive cost centre".to_string(),
            ]));
        }
    }
    let currency = crate::inventory::fx::tenant_currency(db).await?;
    let notes = input.notes.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    match get_wo_cost_estimate(db, input.work_order_id).await? {
        Some(existing) => {
            let expected = input.expected_row_version.unwrap_or(existing.row_version);
            let result = db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE wo_cost_estimates SET cost_center_id = ?, estimated_labor = ?, estimated_parts = ?,
                            estimated_services = ?, estimated_other = ?, currency_code = ?, notes = ?,
                            updated_by_id = ?, row_version = row_version + 1, updated_at = ?
                     WHERE id = ? AND row_version = ?",
                    [
                        input.cost_center_id.into(),
                        input.estimated_labor.into(),
                        input.estimated_parts.into(),
                        input.estimated_services.into(),
                        input.estimated_other.into(),
                        currency.into(),
                        notes.into(),
                        actor_id.into(),
                        now.into(),
                        existing.id.into(),
                        expected.into(),
                    ],
                ))
                .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::ValidationFailed(vec![
                    "GATE_FIN_ESTIMATE_CHANGED: the estimate was changed meanwhile, reload it".to_string(),
                ]));
            }
        }
        None => {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO wo_cost_estimates
                    (entity_sync_id, work_order_id, cost_center_id, estimated_labor, estimated_parts,
                     estimated_services, estimated_other, currency_code, notes, updated_by_id, row_version,
                     created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    uuid::Uuid::new_v4().to_string().into(),
                    input.work_order_id.into(),
                    input.cost_center_id.into(),
                    input.estimated_labor.into(),
                    input.estimated_parts.into(),
                    input.estimated_services.into(),
                    input.estimated_other.into(),
                    currency.into(),
                    notes.into(),
                    actor_id.into(),
                    now.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        }
    }
    get_wo_cost_estimate(db, input.work_order_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("estimate row missing")))
}
