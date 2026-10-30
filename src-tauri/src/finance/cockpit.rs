//! Budget cockpit: budget vs committed vs actual vs forecast by cost centre and category, with the
//! documents behind every figure (benchmark: SAP CO cost-centre report "plan / actual /
//! commitment" and Maximo budget monitoring).
//!
//! - Budget: planned amounts of the version's budget lines.
//! - Actual: net posted actuals of the version (`posted` + `reversed` rows, so a reversal nets to 0).
//! - Committed: open manual commitments of the version, plus — for the current fiscal year, in its
//!   posting version only (so a what-if or a later year never counts them twice) — the live open
//!   value of approved purchase orders raised for OTs and of approved requisitions
//!   for OTs not yet on a PO, at today's exchange rate, on the OT's cost centre. Purchase documents
//!   are read directly (never copied), so every device computes the same figure.
//! - Forecast (estimate at completion): past year = actual; current year = actual + committed +
//!   budget of the remaining months (annual lines pro rata); future year = max(budget, committed).
//! - Variance = budget − forecast (negative = overrun).

use std::collections::{BTreeMap, HashMap};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::finance::fiscal;
use crate::finance::settlement::posting_budget_version;
use crate::finance::wo_costing::{decode, resolve_wo_cost_center, round_money, BUCKET_PARTS, BUCKET_SERVICES};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetCockpitFilter {
    pub fiscal_year: i64,
    /// None = the year's posting version (approved / frozen), else its latest version.
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockpitVersion {
    pub id: i64,
    pub title: Option<String>,
    pub status: String,
    pub scenario_type: String,
    pub version_no: i64,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CockpitFigures {
    pub budget: f64,
    pub committed: f64,
    pub actual: f64,
    pub forecast: f64,
    pub variance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCockpitRow {
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub bucket: String,
    pub budget: f64,
    pub committed: f64,
    pub actual: f64,
    pub forecast: f64,
    pub variance: f64,
    /// actual / budget × 100; None without budget.
    pub consumed_pct: Option<f64>,
    pub open_reviews: i64,
    pub latest_driver_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCockpitMonth {
    pub period_month: i64,
    /// Calendar month of the fiscal period (fiscal year start month setting).
    pub calendar_month: i64,
    pub budget: f64,
    pub actual: f64,
    /// Running totals for the burn chart.
    pub cumulative_budget: f64,
    pub cumulative_actual: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CockpitSetup {
    pub cost_centers: i64,
    pub versions_for_year: i64,
    pub budget_lines: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCockpit {
    pub fiscal_year: i64,
    pub version: Option<CockpitVersion>,
    pub currency_code: Option<String>,
    /// Current month when the year is the current year (forecast boundary).
    pub as_of_month: Option<i64>,
    /// Fiscal-year start month (Settings › Localization); period 1 is this calendar month.
    pub fiscal_start_month: i64,
    pub totals: CockpitFigures,
    pub rows: Vec<BudgetCockpitRow>,
    pub months: Vec<BudgetCockpitMonth>,
    pub pending_settlements: i64,
    pub pending_settlement_amount: f64,
    /// Open purchase commitments for OTs whose cost centre cannot be resolved (shown, not hidden).
    pub unassigned_commitments: f64,
    pub setup: CockpitSetup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetCockpitDrilldownFilter {
    pub fiscal_year: i64,
    pub budget_version_id: Option<i64>,
    pub cost_center_id: i64,
    pub bucket: Option<String>,
    pub period_month: Option<i64>,
    /// Page of actual lines (1..=[`DRILLDOWN_PAGE_MAX`]; [`DRILLDOWN_PAGE_DEFAULT`] when absent).
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Lines per drill-down page when the caller does not say.
pub const DRILLDOWN_PAGE_DEFAULT: i64 = 200;
/// Largest drill-down page.
pub const DRILLDOWN_PAGE_MAX: i64 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrilldownActual {
    pub actual_id: i64,
    pub period_month: Option<i64>,
    pub bucket: String,
    pub amount: f64,
    pub posting_status: String,
    pub source_type: String,
    pub source_id: String,
    pub rate_card_lane: Option<String>,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub work_order_title: Option<String>,
    pub equipment_code: Option<String>,
    pub event_at: Option<String>,
    pub is_reversal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrilldownCommitment {
    /// `purchase_order` | `requisition` | `manual`
    pub kind: String,
    pub reference: String,
    pub document_id: Option<i64>,
    pub supplier_name: Option<String>,
    pub bucket: String,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub amount: f64,
    pub due_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCockpitDrilldown {
    /// One page of the actual lines, newest first.
    pub actuals: Vec<DrilldownActual>,
    /// Every actual line matching the filter (not only this page).
    pub actuals_total: i64,
    /// Sum of the amounts of every matching actual line (tenant currency), so the drill-down's
    /// total equals the cockpit figure whatever the page.
    pub actuals_amount: f64,
    pub limit: i64,
    pub offset: i64,
    pub commitments: Vec<DrilldownCommitment>,
}

/// One open purchase commitment raised for an OT (PO line or requisition), tenant currency.
#[derive(Debug, Clone)]
struct LiveCommitment {
    kind: &'static str,
    reference: String,
    document_id: i64,
    supplier_name: Option<String>,
    bucket: &'static str,
    work_order_id: i64,
    work_order_code: String,
    amount: f64,
    due_at: Option<String>,
}

fn bucket_of_demand(demand: &str) -> &'static str {
    if demand == "WORK_ORDER" {
        BUCKET_SERVICES
    } else {
        BUCKET_PARTS
    }
}

async fn live_commitments<C: ConnectionTrait>(db: &C) -> AppResult<Vec<LiveCommitment>> {
    let po_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT po.id AS po_id, po.po_number, po.currency_code, po.expected_delivery_date,
                    s.name AS supplier_name, pol.demand_source_type, pol.demand_source_id AS wo_id,
                    wo.code AS wo_code,
                    (pol.ordered_qty - pol.received_qty) * COALESCE(pol.unit_price, 0) AS open_amount
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             JOIN work_orders wo ON wo.id = pol.demand_source_id
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             WHERE pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND po.status IN ('APPROVED', 'PARTIALLY_RECEIVED')
               AND pol.ordered_qty > pol.received_qty"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::new();
    if !po_rows.is_empty() {
        let today = crate::clock::now().format("%Y-%m-%d").to_string();
        let tenant = crate::inventory::fx::tenant_currency(db).await?;
        let mut rates: HashMap<String, Option<f64>> = HashMap::new();
        for r in po_rows {
            let currency: Option<String> = decode(&r, "currency_code")?;
            let rate = match currency.filter(|c| *c != tenant) {
                None => Some(1.0),
                Some(c) => match rates.get(&c) {
                    Some(rate) => *rate,
                    None => {
                        let rate = crate::inventory::fx::rate_to_tenant(db, Some(&c), &today).await?;
                        rates.insert(c, rate);
                        rate
                    }
                },
            };
            // Without a known exchange rate the amount cannot be stated in the tenant currency; it
            // is left out rather than counted at par.
            let Some(rate) = rate else { continue };
            let demand: String = decode(&r, "demand_source_type")?;
            out.push(LiveCommitment {
                kind: "purchase_order",
                reference: decode(&r, "po_number")?,
                document_id: decode(&r, "po_id")?,
                supplier_name: decode(&r, "supplier_name")?,
                bucket: bucket_of_demand(&demand),
                work_order_id: decode(&r, "wo_id")?,
                work_order_code: decode(&r, "wo_code")?,
                amount: decode::<f64>(&r, "open_amount")? * rate,
                due_at: decode(&r, "expected_delivery_date")?,
            });
        }
    }
    let req_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT r.id AS req_id, r.req_number, r.demand_source_type, r.demand_source_id AS wo_id, wo.code AS wo_code,
                    SUM(COALESCE(rl.projected_extended_cost, rl.requested_qty * rl.projected_unit_cost, 0)) AS amount
             FROM procurement_requisition_lines rl
             JOIN procurement_requisitions r ON r.id = rl.requisition_id
             JOIN work_orders wo ON wo.id = r.demand_source_id
             WHERE r.status = 'APPROVED'
               AND r.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND NOT EXISTS (SELECT 1 FROM purchase_order_lines pol WHERE pol.requisition_line_id = rl.id)
             GROUP BY r.id, r.req_number, r.demand_source_type, r.demand_source_id, wo.code"
                .to_string(),
        ))
        .await?;
    for r in req_rows {
        let amount: f64 = decode(&r, "amount")?;
        if amount.abs() < 0.005 {
            continue;
        }
        let demand: String = decode(&r, "demand_source_type")?;
        out.push(LiveCommitment {
            kind: "requisition",
            reference: decode(&r, "req_number")?,
            document_id: decode(&r, "req_id")?,
            supplier_name: None,
            bucket: bucket_of_demand(&demand),
            work_order_id: decode(&r, "wo_id")?,
            work_order_code: decode(&r, "wo_code")?,
            amount,
            due_at: None,
        });
    }
    Ok(out)
}

/// Live commitments with the cost centre of their OT (None = unresolved).
async fn live_commitments_by_cost_center<C: ConnectionTrait>(db: &C) -> AppResult<Vec<(Option<i64>, LiveCommitment)>> {
    let mut cache: HashMap<i64, Option<i64>> = HashMap::new();
    let mut out = Vec::new();
    for c in live_commitments(db).await? {
        let cc = match cache.get(&c.work_order_id) {
            Some(v) => *v,
            None => {
                let v = resolve_wo_cost_center(db, c.work_order_id)
                    .await?
                    .map(|r| r.cost_center_id);
                cache.insert(c.work_order_id, v);
                v
            }
        };
        out.push((cc, c));
    }
    Ok(out)
}

/// Live purchase commitments belong to one place only: the current fiscal year, in the version its
/// actuals post to.
async fn shows_live_commitments<C: ConnectionTrait>(
    db: &C,
    fiscal_year: i64,
    current_fiscal_year: i64,
    version_id: i64,
) -> AppResult<bool> {
    if fiscal_year != current_fiscal_year {
        return Ok(false);
    }
    Ok(posting_budget_version(db, fiscal_year).await?.map(|(id, _)| id) == Some(version_id))
}

async fn load_version<C: ConnectionTrait>(
    db: &C,
    fiscal_year: i64,
    version_id: Option<i64>,
) -> AppResult<Option<CockpitVersion>> {
    let id = match version_id {
        Some(id) => Some(id),
        None => match posting_budget_version(db, fiscal_year).await? {
            Some((id, _)) => Some(id),
            // No approved version yet: show the latest version of the year so its budget is visible.
            None => db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM budget_versions WHERE fiscal_year = ? ORDER BY version_no DESC, id DESC LIMIT 1",
                    [fiscal_year.into()],
                ))
                .await?
                .map(|r| decode::<i64>(&r, "id"))
                .transpose()?,
        },
    };
    let Some(id) = id else { return Ok(None) };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, title, status, scenario_type, version_no, currency_code FROM budget_versions WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| {
        Ok(CockpitVersion {
            id: decode(&r, "id")?,
            title: decode(&r, "title")?,
            status: decode(&r, "status")?,
            scenario_type: decode(&r, "scenario_type")?,
            version_no: decode(&r, "version_no")?,
            currency_code: decode(&r, "currency_code")?,
        })
    })
    .transpose()
}

#[derive(Default, Clone)]
struct Acc {
    budget: f64,
    remaining_budget: f64,
    committed: f64,
    actual: f64,
}

/// Estimate at completion of one cell (see module doc).
pub(crate) fn forecast_at_completion(
    fiscal_year: i64,
    this_year: i64,
    budget: f64,
    remaining_budget: f64,
    committed: f64,
    actual: f64,
) -> f64 {
    match fiscal_year.cmp(&this_year) {
        std::cmp::Ordering::Less => actual,
        std::cmp::Ordering::Greater => budget.max(actual + committed),
        std::cmp::Ordering::Equal => actual + committed + remaining_budget,
    }
}

pub async fn get_budget_cockpit<C: ConnectionTrait>(db: &C, filter: BudgetCockpitFilter) -> AppResult<BudgetCockpit> {
    let fiscal_start_month = fiscal::fiscal_start_month(db).await?;
    let (this_year, this_month) = fiscal::current_period(fiscal_start_month);
    let fy = filter.fiscal_year;
    let as_of_month = (fy == this_year).then_some(this_month);

    let setup = match db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT (SELECT COUNT(*) FROM cost_centers WHERE is_active = 1) AS cost_centers,
                    (SELECT COUNT(*) FROM budget_versions WHERE fiscal_year = ?1) AS versions,
                    (SELECT COUNT(*) FROM budget_lines bl JOIN budget_versions bv ON bv.id = bl.budget_version_id
                      WHERE bv.fiscal_year = ?1) AS lines",
            [fy.into()],
        ))
        .await?
    {
        Some(r) => CockpitSetup {
            cost_centers: decode(&r, "cost_centers")?,
            versions_for_year: decode(&r, "versions")?,
            budget_lines: decode(&r, "lines")?,
        },
        None => CockpitSetup::default(),
    };

    let (pending_settlements, pending_settlement_amount) = match db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS n, COALESCE(SUM(total_amount), 0.0) AS amount FROM wo_cost_settlements
                 WHERE status = 'pending' AND COALESCE(fiscal_year, {}) = ?1",
                fiscal::sql_fiscal_year("closed_at", "?2")
            ),
            [fy.into(), fiscal_start_month.into()],
        ))
        .await?
    {
        Some(r) => (decode::<i64>(&r, "n")?, decode::<f64>(&r, "amount")?),
        None => (0, 0.0),
    };

    let version = load_version(db, fy, filter.budget_version_id).await?;
    let currency_code = match &version {
        Some(v) => Some(v.currency_code.clone()),
        None => crate::inventory::fx::tenant_currency(db).await.ok(),
    };

    let mut acc: BTreeMap<(i64, String), Acc> = BTreeMap::new();
    let mut months: BTreeMap<i64, (f64, f64)> = (1..=12).map(|m| (m, (0.0, 0.0))).collect();
    let mut unassigned_commitments = 0.0;
    let mut review_map: HashMap<(i64, String), (i64, Option<String>)> = HashMap::new();

    if let Some(v) = &version {
        let cc_filter = filter.cost_center_id;
        let lines = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT cost_center_id, budget_bucket, period_month, SUM(planned_amount) AS amount
                 FROM budget_lines WHERE budget_version_id = ? AND (? IS NULL OR cost_center_id = ?)
                 GROUP BY cost_center_id, budget_bucket, period_month",
                [v.id.into(), cc_filter.into(), cc_filter.into()],
            ))
            .await?;
        for r in lines {
            let month: Option<i64> = decode(&r, "period_month")?;
            let amount: f64 = decode(&r, "amount")?;
            let a = acc
                .entry((decode(&r, "cost_center_id")?, decode(&r, "budget_bucket")?))
                .or_default();
            a.budget += amount;
            match month {
                Some(m) => {
                    months.entry(m).or_default().0 += amount;
                    if as_of_month.is_some_and(|cur| m > cur) {
                        a.remaining_budget += amount;
                    }
                }
                None => {
                    for m in 1..=12 {
                        months.entry(m).or_default().0 += amount / 12.0;
                    }
                    if let Some(cur) = as_of_month {
                        a.remaining_budget += amount * (12 - cur) as f64 / 12.0;
                    }
                }
            }
        }

        let actuals = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT cost_center_id, budget_bucket, period_month, SUM(amount_base) AS amount
                 FROM budget_actuals
                 WHERE budget_version_id = ? AND posting_status IN ('posted', 'reversed')
                   AND (? IS NULL OR cost_center_id = ?)
                 GROUP BY cost_center_id, budget_bucket, period_month",
                [v.id.into(), cc_filter.into(), cc_filter.into()],
            ))
            .await?;
        for r in actuals {
            let month: Option<i64> = decode(&r, "period_month")?;
            let amount: f64 = decode(&r, "amount")?;
            acc.entry((decode(&r, "cost_center_id")?, decode(&r, "budget_bucket")?))
                .or_default()
                .actual += amount;
            if let Some(m) = month {
                months.entry(m).or_default().1 += amount;
            }
        }

        let manual = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT cost_center_id, budget_bucket, SUM(base_amount) AS amount
                 FROM budget_commitments
                 WHERE budget_version_id = ? AND commitment_status = 'open'
                   AND (? IS NULL OR cost_center_id = ?)
                 GROUP BY cost_center_id, budget_bucket",
                [v.id.into(), cc_filter.into(), cc_filter.into()],
            ))
            .await?;
        for r in manual {
            acc.entry((decode(&r, "cost_center_id")?, decode(&r, "budget_bucket")?))
                .or_default()
                .committed += decode::<f64>(&r, "amount")?;
        }

        if shows_live_commitments(db, fy, this_year, v.id).await? {
            for (cc, c) in live_commitments_by_cost_center(db).await? {
                match cc {
                    Some(cc) if cc_filter.is_none_or(|f| f == cc) => {
                        acc.entry((cc, c.bucket.to_string())).or_default().committed += c.amount;
                    }
                    Some(_) => {}
                    None => unassigned_commitments += c.amount,
                }
            }
        }

        let reviews = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT r.cost_center_id, r.budget_bucket,
                        SUM(CASE WHEN r.review_status IN ('open', 'in_review', 'actioned') THEN 1 ELSE 0 END) AS open_reviews,
                        (SELECT r2.driver_code FROM budget_variance_reviews r2
                          WHERE r2.budget_version_id = r.budget_version_id AND r2.cost_center_id = r.cost_center_id
                            AND r2.budget_bucket = r.budget_bucket
                          ORDER BY r2.id DESC LIMIT 1) AS latest_driver
                 FROM budget_variance_reviews r
                 WHERE r.budget_version_id = ?
                 GROUP BY r.cost_center_id, r.budget_bucket",
                [v.id.into()],
            ))
            .await?;
        for r in reviews {
            review_map.insert(
                (decode(&r, "cost_center_id")?, decode(&r, "budget_bucket")?),
                (decode(&r, "open_reviews")?, decode(&r, "latest_driver")?),
            );
        }
    }

    let mut cc_names: HashMap<i64, (String, String)> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name FROM cost_centers".to_string(),
        ))
        .await?
    {
        cc_names.insert(decode(&r, "id")?, (decode(&r, "code")?, decode(&r, "name")?));
    }

    let mut totals = CockpitFigures::default();
    let mut rows = Vec::with_capacity(acc.len());
    for ((cc, bucket), a) in acc {
        let forecast = forecast_at_completion(fy, this_year, a.budget, a.remaining_budget, a.committed, a.actual);
        let (budget, committed, actual, forecast) = (
            round_money(a.budget),
            round_money(a.committed),
            round_money(a.actual),
            round_money(forecast),
        );
        if budget == 0.0 && committed == 0.0 && actual == 0.0 {
            continue;
        }
        let variance = round_money(budget - forecast);
        totals.budget += budget;
        totals.committed += committed;
        totals.actual += actual;
        totals.forecast += forecast;
        totals.variance += variance;
        let (code, name) = cc_names
            .get(&cc)
            .cloned()
            .unwrap_or_else(|| (cc.to_string(), String::new()));
        let (open_reviews, latest_driver_code) = review_map.remove(&(cc, bucket.clone())).unwrap_or((0, None));
        rows.push(BudgetCockpitRow {
            cost_center_id: cc,
            cost_center_code: code,
            cost_center_name: name,
            bucket,
            budget,
            committed,
            actual,
            forecast,
            variance,
            consumed_pct: (budget > 0.0).then(|| round_money(actual / budget * 100.0)),
            open_reviews,
            latest_driver_code,
        });
    }
    let totals = CockpitFigures {
        budget: round_money(totals.budget),
        committed: round_money(totals.committed),
        actual: round_money(totals.actual),
        forecast: round_money(totals.forecast),
        variance: round_money(totals.variance),
    };

    let mut cum_budget = 0.0;
    let mut cum_actual = 0.0;
    let months = months
        .into_iter()
        .map(|(m, (budget, actual))| {
            cum_budget += budget;
            cum_actual += actual;
            BudgetCockpitMonth {
                period_month: m,
                calendar_month: fiscal::calendar_month(fy, m, fiscal_start_month).1,
                budget: round_money(budget),
                actual: round_money(actual),
                cumulative_budget: round_money(cum_budget),
                cumulative_actual: round_money(cum_actual),
            }
        })
        .collect();

    Ok(BudgetCockpit {
        fiscal_year: fy,
        version,
        currency_code,
        as_of_month,
        fiscal_start_month,
        totals,
        rows,
        months,
        pending_settlements,
        pending_settlement_amount: round_money(pending_settlement_amount),
        unassigned_commitments: round_money(unassigned_commitments),
        setup,
    })
}

pub async fn get_budget_cockpit_drilldown<C: ConnectionTrait>(
    db: &C,
    filter: BudgetCockpitDrilldownFilter,
) -> AppResult<BudgetCockpitDrilldown> {
    let limit = filter
        .limit
        .unwrap_or(DRILLDOWN_PAGE_DEFAULT)
        .clamp(1, DRILLDOWN_PAGE_MAX);
    let offset = filter.offset.unwrap_or(0).max(0);
    let Some(version) = load_version(db, filter.fiscal_year, filter.budget_version_id).await? else {
        return Ok(BudgetCockpitDrilldown {
            actuals: Vec::new(),
            actuals_total: 0,
            actuals_amount: 0.0,
            limit,
            offset,
            commitments: Vec::new(),
        });
    };
    let bucket = filter.bucket.clone().filter(|b| !b.trim().is_empty());
    let scope_binds = || -> Vec<sea_orm::Value> {
        vec![
            version.id.into(),
            filter.cost_center_id.into(),
            bucket.clone().into(),
            bucket.clone().into(),
            filter.period_month.into(),
            filter.period_month.into(),
        ]
    };
    const SCOPE: &str = "FROM budget_actuals ba
             WHERE ba.budget_version_id = ? AND ba.cost_center_id = ?
               AND ba.posting_status IN ('posted', 'reversed')
               AND (? IS NULL OR ba.budget_bucket = ?)
               AND (? IS NULL OR ba.period_month = ?)";
    // Count and sum over every matching line: the page never hides the total.
    let totals = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS n, COALESCE(SUM(ba.amount_base), 0.0) AS amount {SCOPE}"),
            scope_binds(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("drill-down total returned no row")))?;
    let actuals_total: i64 = decode(&totals, "n")?;
    let actuals_amount: f64 = decode(&totals, "amount")?;
    let mut page_binds = scope_binds();
    page_binds.push(limit.into());
    page_binds.push(offset.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ba.id, ba.period_month, ba.budget_bucket, ba.amount_base, ba.posting_status, ba.source_type,
                    ba.source_id, ba.rate_card_lane, ba.work_order_id, wo.code AS wo_code, wo.title AS wo_title,
                    e.asset_id_code AS equipment_code, ba.event_at, ba.reversal_of_actual_id
             FROM budget_actuals ba
             LEFT JOIN work_orders wo ON wo.id = ba.work_order_id
             LEFT JOIN equipment e ON e.id = COALESCE(ba.equipment_id, wo.equipment_id)
             WHERE ba.budget_version_id = ? AND ba.cost_center_id = ?
               AND ba.posting_status IN ('posted', 'reversed')
               AND (? IS NULL OR ba.budget_bucket = ?)
               AND (? IS NULL OR ba.period_month = ?)
             ORDER BY COALESCE(ba.event_at, ba.created_at) DESC, ba.id DESC
             LIMIT ? OFFSET ?",
            page_binds,
        ))
        .await?;
    let mut actuals = Vec::with_capacity(rows.len());
    for r in rows {
        actuals.push(DrilldownActual {
            actual_id: decode(&r, "id")?,
            period_month: decode(&r, "period_month")?,
            bucket: decode(&r, "budget_bucket")?,
            amount: decode(&r, "amount_base")?,
            posting_status: decode(&r, "posting_status")?,
            source_type: decode(&r, "source_type")?,
            source_id: decode(&r, "source_id")?,
            rate_card_lane: decode(&r, "rate_card_lane")?,
            work_order_id: decode(&r, "work_order_id")?,
            work_order_code: decode(&r, "wo_code")?,
            work_order_title: decode(&r, "wo_title")?,
            equipment_code: decode(&r, "equipment_code")?,
            event_at: decode(&r, "event_at")?,
            is_reversal: decode::<Option<i64>>(&r, "reversal_of_actual_id")?.is_some(),
        });
    }

    let mut commitments = Vec::new();
    // Commitments are open obligations, not dated in a month: listed for the year view only.
    if filter.period_month.is_none() {
        let manual = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT c.source_type, c.source_id, c.budget_bucket, c.base_amount, c.work_order_id,
                        wo.code AS wo_code, c.purchase_order_id, c.due_at
                 FROM budget_commitments c LEFT JOIN work_orders wo ON wo.id = c.work_order_id
                 WHERE c.budget_version_id = ? AND c.cost_center_id = ? AND c.commitment_status = 'open'
                   AND (? IS NULL OR c.budget_bucket = ?)
                 ORDER BY c.id DESC",
                [
                    version.id.into(),
                    filter.cost_center_id.into(),
                    bucket.clone().into(),
                    bucket.clone().into(),
                ],
            ))
            .await?;
        for r in manual {
            let source_type: String = decode(&r, "source_type")?;
            let source_id: String = decode(&r, "source_id")?;
            commitments.push(DrilldownCommitment {
                kind: "manual".to_string(),
                reference: format!("{source_type} {source_id}"),
                document_id: decode(&r, "purchase_order_id")?,
                supplier_name: None,
                bucket: decode(&r, "budget_bucket")?,
                work_order_id: decode(&r, "work_order_id")?,
                work_order_code: decode(&r, "wo_code")?,
                amount: round_money(decode(&r, "base_amount")?),
                due_at: decode(&r, "due_at")?,
            });
        }
        let (current_fy, _) = fiscal::current_period(fiscal::fiscal_start_month(db).await?);
        if shows_live_commitments(db, filter.fiscal_year, current_fy, version.id).await? {
            for (cc, c) in live_commitments_by_cost_center(db).await? {
                if cc != Some(filter.cost_center_id) || bucket.as_deref().is_some_and(|b| b != c.bucket) {
                    continue;
                }
                commitments.push(DrilldownCommitment {
                    kind: c.kind.to_string(),
                    reference: c.reference,
                    document_id: Some(c.document_id),
                    supplier_name: c.supplier_name,
                    bucket: c.bucket.to_string(),
                    work_order_id: Some(c.work_order_id),
                    work_order_code: Some(c.work_order_code),
                    amount: round_money(c.amount),
                    due_at: c.due_at,
                });
            }
        }
    }
    Ok(BudgetCockpitDrilldown {
        actuals,
        actuals_total,
        actuals_amount: round_money(actuals_amount),
        limit,
        offset,
        commitments,
    })
}
