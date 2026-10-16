//! Supervisor / manager start panel: open requests against their SLA, the OT backlog by nature and
//! urgency, PM due and overdue, stock to reorder, permits and lockouts, critical assets down.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use super::{decode, one_row, period_first_day, period_window, urgency_rank_sql, ITEM_LIMIT};
use crate::analytics::as_of::instant;
use crate::analytics::smrp;
use crate::assets::taxonomy_reference::{CRITICAL_DOWN_SQL, DOMAIN_CRITICALITY, DOMAIN_STATUS, STATUS_TAG_DOWN};
use crate::di::queries::DiQueueCounts;
use crate::di::stats::{DiSlaClosingOutcome, DiSlaOpenSplit, DiStatsFilter};
use crate::errors::AppResult;
use crate::wo::backlog::{OPEN_STATUS_SQL, WO_OVERDUE_SQL};
use crate::wo::types::MaintenanceNature;

/// Open requests against their SLA (`di::stats::sla_open_split`), SLA compliance of the requests
/// closed in the period (`di::stats::sla_closing_outcome`) and the work queues.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SupervisionRequests {
    pub sla_open: DiSlaOpenSplit,
    pub period_days: i64,
    pub sla_closing: DiSlaClosingOutcome,
    pub queues: DiQueueCounts,
}

/// Open OTs of one nature (EN 13306, `None`: type without nature) and one urgency level.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WoBacklogCell {
    pub nature_code: Option<String>,
    pub urgency_code: Option<String>,
    pub count: i64,
}

/// OT backlog: work still ahead (draft to on hold, `wo::backlog::OPEN_STATUS_SQL`), by the nature
/// of the OT type and the shared urgency scale; SMRP planned / reactive / emergency by nature
/// (`MaintenanceNature::is_planned` / `is_corrective` / `is_emergency`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SupervisionWorkOrders {
    pub open: i64,
    /// Of `open`, the OTs past their planned end (`wo::backlog::WO_OVERDUE_SQL`, the OT
    /// dashboard's one overdue rule).
    pub overdue: i64,
    pub planned: i64,
    pub reactive: i64,
    /// Subset of reactive: immediate corrective maintenance.
    pub emergency: i64,
    pub without_nature: i64,
    pub cells: Vec<WoBacklogCell>,
}

/// PM due and overdue (`pm::queries::get_pm_due_metrics`, the PM page's figures) and SMRP PM
/// compliance over the period (`analytics::smrp::pm_compliance`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupervisionPm {
    pub overdue: i64,
    pub due_today: i64,
    pub due_next_7_days: i64,
    pub period_days: i64,
    pub compliance_done_on_time: f64,
    pub compliance_due: f64,
    pub compliance_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StockItem {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub warehouse_code: String,
    pub available_qty: f64,
    pub reorder_point: f64,
    pub min_stock: f64,
    /// `reorder_point` or `min_stock` (`inventory::queries::evaluate_reorder`).
    pub trigger_type: String,
}

/// Stock to reorder (`inventory::queries::evaluate_reorder`, the replenishment screen's rule).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupervisionStock {
    /// Article × warehouse lines at or below their reorder point (or under minimum stock).
    pub lines_to_reorder: i64,
    pub articles_to_reorder: i64,
    /// Of those lines, the ones with nothing available.
    pub lines_out_of_stock: i64,
    /// Out of stock first, then the largest shortfall against the threshold.
    pub items: Vec<StockItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PermitItem {
    pub permit_id: i64,
    pub code: String,
    pub permit_type: String,
    pub asset_code: String,
    pub expires_at: String,
}

/// Permits found by a check: how many, the first [`ITEM_LIMIT`] to open, and how many the check
/// found without a record to open (counted, never dropped).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PermitList {
    pub total: i64,
    pub items: Vec<PermitItem>,
    pub without_record: i64,
}

/// State of the "permit expiring" rule (Données de référence → Notifications).
pub const EXPIRING_RULE_ACTIVE: &str = "active";
/// The rule exists but is switched off: nothing is "expiring soon".
pub const EXPIRING_RULE_DISABLED: &str = "disabled";
/// The rule is on but has no lead time: nothing is "expiring soon".
pub const EXPIRING_RULE_NO_LEAD: &str = "no_lead";
/// No such rule.
pub const EXPIRING_RULE_MISSING: &str = "missing";

/// Permits and lockout (PTW / LOTO). Open, expiring and expired permits share one status set
/// (`permit::queries::OPEN_PERMIT_STATUSES_SQL`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SupervisionSafety {
    /// Open and not expired (`permit::queries::list_open_permits_report`).
    pub open_permits: i64,
    /// One of the `EXPIRING_RULE_*` states.
    pub expiring_rule: String,
    /// Lead time of the rule when it is active.
    pub expiring_lead_minutes: Option<i64>,
    /// Open permits expiring within the lead time (`notifications::watchers`, rule
    /// `ptw_expiring`); empty unless the rule is active.
    pub expiring: PermitList,
    /// Open permits past their expiry (rule `ptw_critical`'s condition).
    pub expired_open: PermitList,
    /// Lockout points still fitted and the permits they belong to
    /// (`permit::queries::isolation_locks_in_place`).
    pub locks_in_place: i64,
    pub permits_with_locks: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AssetItem {
    pub id: i64,
    pub asset_code: String,
    pub name: String,
    pub status_label: Option<String>,
}

/// Critical assets down (`assets::taxonomy_reference::CRITICAL_DOWN_SQL`): the most critical
/// level of the criticality list and a status tagged `down`, both from Données de référence.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SupervisionAssets {
    pub critical_down: i64,
    /// Label of the criticality level counted (the first of the list), when the list has one.
    pub criticality_label: Option<String>,
    /// Labels of the statuses counted as down.
    pub down_status_labels: Vec<String>,
    pub items: Vec<AssetItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomeSupervision {
    pub as_of: String,
    pub requests: Option<SupervisionRequests>,
    pub work_orders: Option<SupervisionWorkOrders>,
    pub pm: Option<SupervisionPm>,
    pub stock: Option<SupervisionStock>,
    pub safety: Option<SupervisionSafety>,
    pub assets: Option<SupervisionAssets>,
}

/// Which sections the person may see (the module's own view permission, checked by the caller).
#[derive(Debug, Clone, Copy, Default)]
pub struct SupervisionAccess {
    pub requests: bool,
    pub work_orders: bool,
    pub pm: bool,
    pub stock: bool,
    pub safety: bool,
    pub assets: bool,
}

pub async fn requests(
    db: &DatabaseConnection,
    user_id: i64,
    period_days: i64,
    now: DateTime<Utc>,
) -> AppResult<SupervisionRequests> {
    // The panel's period convention (`home::period_first_day`): the last N days, today included.
    let from = period_first_day(now, period_days).to_string();
    let to = now.date_naive().to_string();
    Ok(SupervisionRequests {
        sla_open: crate::di::stats::sla_open_split(db, None).await?,
        period_days,
        sla_closing: crate::di::stats::sla_closing_outcome(
            db,
            &DiStatsFilter {
                date_from: Some(from),
                date_to: Some(to),
                entity_id: None,
            },
        )
        .await?,
        queues: crate::di::queries::count_di_queues(db, user_id).await?,
    })
}

pub async fn work_orders(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<SupervisionWorkOrders> {
    let rank = urgency_rank_sql("ul.code");
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT t.semantic_tag AS nature_code, ul.code AS urgency_code, COUNT(*) AS cnt \
                 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 LEFT JOIN reference_values t ON t.id = wo.type_id \
                 LEFT JOIN reference_values ul ON ul.id = wo.urgency_id \
                 WHERE s.code IN {OPEN_STATUS_SQL} \
                 GROUP BY t.semantic_tag, ul.code \
                 ORDER BY t.semantic_tag, MIN({rank})"
            ),
        ))
        .await?;
    let mut out = SupervisionWorkOrders {
        open: 0,
        overdue: decode(
            &one_row(
                db.query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT COUNT(*) AS cnt FROM work_orders wo \
                         JOIN work_order_statuses s ON s.id = wo.status_id WHERE {WO_OVERDUE_SQL}"
                    ),
                    [instant(now).into()],
                ))
                .await?,
                "overdue work orders",
            )?,
            "cnt",
        )?,
        planned: 0,
        reactive: 0,
        emergency: 0,
        without_nature: 0,
        cells: Vec::with_capacity(rows.len()),
    };
    for r in &rows {
        let nature_code: Option<String> = decode(r, "nature_code")?;
        let count: i64 = decode(r, "cnt")?;
        out.open += count;
        match nature_code.as_deref().and_then(MaintenanceNature::parse) {
            Some(n) => {
                if n.is_planned() {
                    out.planned += count;
                }
                if n.is_corrective() {
                    out.reactive += count;
                }
                if n.is_emergency() {
                    out.emergency += count;
                }
            }
            None => out.without_nature += count,
        }
        out.cells.push(WoBacklogCell {
            nature_code,
            urgency_code: decode(r, "urgency_code")?,
            count,
        });
    }
    Ok(out)
}

pub async fn pm(db: &DatabaseConnection, period_days: i64, now: DateTime<Utc>) -> AppResult<SupervisionPm> {
    let due = crate::pm::queries::get_pm_due_metrics(db).await?;
    let compliance = smrp::pm_compliance(db, period_window(now, period_days), now, None).await?;
    Ok(SupervisionPm {
        overdue: due.overdue_count,
        due_today: due.due_today_count,
        due_next_7_days: due.due_next_7d_count,
        period_days,
        compliance_done_on_time: compliance.numerator,
        compliance_due: compliance.denominator,
        compliance_pct: compliance.value_pct,
    })
}

pub async fn stock(db: &DatabaseConnection) -> AppResult<SupervisionStock> {
    let lines = crate::inventory::queries::evaluate_reorder(db, None).await?;
    let mut articles: Vec<i64> = lines.iter().map(|l| l.article_id).collect();
    articles.sort_unstable();
    articles.dedup();
    let threshold = |l: &crate::inventory::domain::InventoryReorderRecommendation| {
        if l.trigger_type == "min_stock" {
            l.min_stock
        } else {
            l.reorder_point
        }
    };
    let mut sorted: Vec<_> = lines.iter().collect();
    sorted.sort_by(|a, b| {
        let out_a = a.available_qty <= 0.0;
        let out_b = b.available_qty <= 0.0;
        out_b
            .cmp(&out_a)
            .then_with(|| {
                let short_a = threshold(a) - a.available_qty;
                let short_b = threshold(b) - b.available_qty;
                short_b.partial_cmp(&short_a).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.article_code.cmp(&b.article_code))
    });
    Ok(SupervisionStock {
        lines_to_reorder: lines.len() as i64,
        articles_to_reorder: articles.len() as i64,
        lines_out_of_stock: lines.iter().filter(|l| l.available_qty <= 0.0).count() as i64,
        items: sorted
            .into_iter()
            .take(ITEM_LIMIT as usize)
            .map(|l| StockItem {
                article_id: l.article_id,
                article_code: l.article_code.clone(),
                article_name: l.article_name.clone(),
                warehouse_code: l.warehouse_code.clone(),
                available_qty: l.available_qty,
                reorder_point: l.reorder_point,
                min_stock: l.min_stock,
                trigger_type: l.trigger_type.clone(),
            })
            .collect(),
    })
}

pub fn permit_list(candidates: Vec<crate::notifications::watchers::Candidate>) -> PermitList {
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let total = candidates.len() as i64;
    let without_record = candidates.iter().filter(|c| c.record_id.is_none()).count() as i64;
    let items = candidates
        .into_iter()
        .filter_map(|c| {
            c.record_id.map(|permit_id| PermitItem {
                permit_id,
                code: text(&c.params, "code"),
                permit_type: text(&c.params, "title"),
                asset_code: text(&c.params, "asset"),
                expires_at: text(&c.params, "expires_at"),
            })
        })
        .take(ITEM_LIMIT as usize)
        .collect();
    PermitList {
        total,
        items,
        without_record,
    }
}

pub async fn safety(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<SupervisionSafety> {
    use crate::notifications::watchers;
    let open_permits = crate::permit::queries::list_open_permits_report(db).await?.len() as i64;
    let (expiring_rule, lead) = match watchers::rule_of(db, "ptw_expiring").await? {
        None => (EXPIRING_RULE_MISSING, None),
        Some((false, _)) => (EXPIRING_RULE_DISABLED, None),
        Some((true, lead)) => match lead.filter(|m| *m > 0) {
            Some(minutes) => (EXPIRING_RULE_ACTIVE, Some(minutes)),
            None => (EXPIRING_RULE_NO_LEAD, None),
        },
    };
    let expiring = match lead {
        Some(minutes) => permit_list(watchers::candidates(db, "ptw_expiring", now, Some(minutes), false).await?),
        None => PermitList {
            total: 0,
            items: Vec::new(),
            without_record: 0,
        },
    };
    let expired_open = permit_list(watchers::candidates(db, "ptw_critical", now, None, false).await?);
    let (permits_with_locks, locks_in_place) = crate::permit::queries::isolation_locks_in_place(db).await?;
    Ok(SupervisionSafety {
        open_permits,
        expiring_rule: expiring_rule.to_string(),
        expiring_lead_minutes: lead,
        expiring,
        expired_open,
        locks_in_place,
        permits_with_locks,
    })
}

pub async fn assets(db: &DatabaseConnection) -> AppResult<SupervisionAssets> {
    let count_row = one_row(
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS cnt FROM equipment e WHERE {CRITICAL_DOWN_SQL}"),
        ))
        .await?,
        "critical assets down",
    )?;
    let item_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT e.id, e.asset_id_code, e.name, \
                        (SELECT st.label FROM reference_values st WHERE st.id = e.equipment_status_ref_id) AS status_label \
                 FROM equipment e WHERE {CRITICAL_DOWN_SQL} \
                 ORDER BY e.asset_id_code ASC, e.id ASC LIMIT {ITEM_LIMIT}"
            ),
        ))
        .await?;
    let published = "FROM reference_values rv \
         JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
         JOIN reference_domains d ON d.id = rs.domain_id \
         WHERE d.code = ? AND rv.is_active = 1";
    let criticality_label = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT rv.label {published} ORDER BY rv.sort_order ASC, rv.id ASC LIMIT 1"),
            [DOMAIN_CRITICALITY.into()],
        ))
        .await?
        .map(|r| decode::<String>(&r, "label"))
        .transpose()?;
    let down_status_labels = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT rv.label {published} AND rv.semantic_tag = ? ORDER BY rv.sort_order ASC, rv.id ASC"),
            [DOMAIN_STATUS.into(), STATUS_TAG_DOWN.into()],
        ))
        .await?
        .iter()
        .map(|r| decode::<String>(r, "label"))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(SupervisionAssets {
        critical_down: decode(&count_row, "cnt")?,
        criticality_label,
        down_status_labels,
        items: item_rows
            .iter()
            .map(|r| {
                Ok(AssetItem {
                    id: decode(r, "id")?,
                    asset_code: decode(r, "asset_id_code")?,
                    name: decode(r, "name")?,
                    status_label: decode(r, "status_label")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
    })
}

pub async fn build_supervision(
    db: &DatabaseConnection,
    user_id: i32,
    access: SupervisionAccess,
    period_days: i64,
    now: DateTime<Utc>,
) -> AppResult<HomeSupervision> {
    Ok(HomeSupervision {
        as_of: instant(now),
        requests: if access.requests {
            Some(requests(db, i64::from(user_id), period_days, now).await?)
        } else {
            None
        },
        work_orders: if access.work_orders {
            Some(work_orders(db, now).await?)
        } else {
            None
        },
        pm: if access.pm {
            Some(pm(db, period_days, now).await?)
        } else {
            None
        },
        stock: if access.stock { Some(stock(db).await?) } else { None },
        safety: if access.safety {
            Some(safety(db, now).await?)
        } else {
            None
        },
        assets: if access.assets { Some(assets(db).await?) } else { None },
    })
}
