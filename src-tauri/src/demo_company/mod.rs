//! Demo-company generator (developer-only, feature `demo-company`).
//!
//! Fills the EMPTY database of a licensed company with one year of plausible history of a
//! fictional plant ([`catalog::COMPANY_NAME`]): reference data, organisation, assets, people,
//! stock, suppliers, budgets, then a day-by-day simulation of preventive plans, breakdowns
//! (DI -> OT -> close-out), inspections, permits / LOTO, purchases and stock counts.
//!
//! Every business row is written by the app's own services, under a simulated clock
//! ([`crate::clock::simulated`]) so each record carries the date it happened on. Raw SQL is only
//! used to read ids and to check the database is empty. Never reachable from the shipped app: no
//! command, no menu; only the `demo-company` binary calls [`generate`].
//!
//! It only runs on the database of the demo company itself ([`guard`]): the licence activated on
//! the PC is [`catalog::COMPANY_NAME`]'s, nothing was ever received by synchronisation, and no
//! table holds company data yet.
//!
//! Audit chain: the events written by the activation and the first sign-in carry the real date and
//! come first in `chain_seq`; the generated history, dated back over the past year, follows them.
//! Dates are therefore not in chain order. Accepted for a demonstration database only (the
//! generator refuses any other one).

pub mod catalog;
pub mod guard;
pub mod history;
pub mod kpis;
pub mod master;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use sea_orm_migration::MigratorTrait;

use crate::clock::simulated::SimClock;
use crate::errors::{AppError, AppResult};

/// Name of the SQLite VFS whose clock is the generator's simulated clock.
pub const SIM_VFS_NAME: &str = "maintafox-demo-sim";

/// Default seed of the random draws (same seed + same `today` => same company).
pub const DEFAULT_SEED: u64 = 20_261_030;

#[derive(Debug, Clone)]
pub struct GenerationOptions {
    pub seed: u64,
    /// Last day of the history (the history ends at the current time of that day at the latest).
    pub today: NaiveDate,
    /// Days of history before `today` (365 for the real run; tests may shorten it).
    pub history_days: i64,
}

impl GenerationOptions {
    pub fn new(seed: u64, today: NaiveDate) -> Self {
        Self {
            seed,
            today,
            history_days: 365,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenerationReport {
    /// What was created, by kind (fixed order).
    pub counts: BTreeMap<String, i64>,
    pub kpis: kpis::KpiSummary,
    /// Steps a service refused, or stopped at the end of the history (the generator went on
    /// without them).
    pub warnings: Vec<String>,
    /// End of the history: no record is dated after it.
    pub limit: Option<DateTime<Utc>>,
}

/// Generation context shared by the master-data and history phases.
pub struct Ctx<'a> {
    pub db: &'a DatabaseConnection,
    pub clock: &'a SimClock,
    /// Last moment written, so every action is strictly later than the previous one.
    pub last: DateTime<Utc>,
    /// Latest moment allowed (the end of the history).
    pub limit: DateTime<Utc>,
    pub report: GenerationReport,
}

impl<'a> Ctx<'a> {
    /// Moves the simulated clock to `at` (or just after the previous action when `at` is earlier)
    /// and returns the moment used. Refuses a moment past [`Ctx::limit`]: the step that asked for it
    /// stops there (the agenda records it as a warning) rather than write a record dated after the
    /// end of the history.
    pub fn at(&mut self, at: DateTime<Utc>) -> AppResult<DateTime<Utc>> {
        let min = self.last + Duration::seconds(2);
        let t = if at < min { min } else { at };
        if t > self.limit {
            return Err(AppError::ValidationFailed(vec![format!(
                "{} est après la fin de l'historique ({}): étape arrêtée",
                master::rfc(t),
                master::rfc(self.limit)
            )]));
        }
        self.last = t;
        self.clock.set(t);
        Ok(t)
    }

    /// Advances the clock by a few seconds (two documents never share a millisecond).
    pub fn tick(&mut self) -> AppResult<DateTime<Utc>> {
        let t = self.last + Duration::seconds(3);
        self.at(t)
    }

    pub fn count(&mut self, kind: &str, n: i64) {
        *self.report.counts.entry(kind.to_string()).or_insert(0) += n;
    }

    pub fn warn(&mut self, what: impl Into<String>) {
        self.report.warnings.push(what.into());
    }
}

/// Generates the demo company in `db`, opened with the [`SIM_VFS_NAME`] VFS bound to `clock`
/// (installed on the current thread by the caller).
pub async fn generate(
    db: &DatabaseConnection,
    clock: &SimClock,
    opts: &GenerationOptions,
) -> AppResult<GenerationReport> {
    preflight(db).await?;

    // The history stops in the middle of the morning shift of `today`, and never later than ten
    // minutes ago, so the company is caught with work under way and no record is dated in the
    // future of this PC.
    let mid_morning = master::local(opts.today, 11, 30);
    let limit = mid_morning.min(Utc::now() - Duration::minutes(10));
    let start_day = opts.today - Duration::days(opts.history_days);
    // The company is set up the evening before the first day of history.
    let setup_at = (start_day - Duration::days(1))
        .and_hms_opt(14, 0, 0)
        .expect("valid time")
        .and_utc();

    let mut ctx = Ctx {
        db,
        clock,
        last: setup_at - Duration::seconds(10),
        limit,
        report: GenerationReport::default(),
    };
    ctx.at(setup_at)?;

    let company = master::build(&mut ctx, opts, start_day).await?;
    history::run(&mut ctx, &company, opts, start_day).await?;

    let end = limit - Duration::minutes(1);
    if end >= ctx.last + Duration::seconds(2) {
        ctx.at(end)?;
    }
    ctx.report.limit = Some(limit);
    ctx.report.kpis = kpis::summarize(db, ctx.last).await?;
    Ok(ctx.report)
}

/// Every check made before anything is written (read only); [`generate`] runs it again.
pub async fn preflight(db: &DatabaseConnection) -> AppResult<()> {
    ensure_no_pending_migrations(db).await?;
    guard::ensure_demo_company_licence(db).await?;
    guard::ensure_no_received_sync_data(db).await?;
    guard::ensure_no_business_data(db).await?;
    Ok(())
}

/// Refuses a database whose schema is behind this build (the app must be opened once first).
pub async fn ensure_no_pending_migrations(db: &DatabaseConnection) -> AppResult<()> {
    let pending = crate::migrations::Migrator::get_pending_migrations(db).await?;
    if !pending.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "La base n'est pas à jour ({} migration(s) en attente). Ouvrez-la une fois avec cette version \
             de Maintafox, puis relancez le générateur.",
            pending.len()
        )]));
    }
    Ok(())
}

/// Root org node written by the licence activation (`db::tenant_bootstrap`), if any.
pub(crate) async fn tenant_bootstrap_root(db: &impl ConnectionTrait) -> AppResult<Option<i64>> {
    let json = scalar_text(
        db,
        "SELECT setting_value_json FROM app_settings WHERE setting_key = 'tenant.initial_bootstrap' LIMIT 1",
        vec![],
    )
    .await?;
    Ok(json
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
        .and_then(|v| v.get("root_org_node_id").and_then(serde_json::Value::as_i64)))
}

// ── Read helpers (raw SQL is only used for reads) ───────────────────────────────────────────

pub(crate) async fn scalar_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    Ok(row.and_then(|r| r.try_get_by_index::<Option<i64>>(0).ok().flatten()))
}

pub(crate) async fn scalar_f64(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    Ok(row.and_then(|r| r.try_get_by_index::<Option<f64>>(0).ok().flatten()))
}

pub(crate) async fn scalar_text(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    Ok(row.and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten()))
}

pub(crate) async fn column_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> AppResult<Vec<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    Ok(rows.iter().filter_map(|r| r.try_get_by_index::<i64>(0).ok()).collect())
}

/// Id of an active value of the published set of a governed reference domain.
pub(crate) async fn ref_value_id(db: &impl ConnectionTrait, domain: &str, code: &str) -> AppResult<i64> {
    scalar_i64(
        db,
        "SELECT rv.id FROM reference_values rv \
         JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE rd.code = ? AND rs.status = 'published' AND rv.code = ? AND rv.is_active = 1 \
         ORDER BY rs.version_no DESC LIMIT 1",
        vec![domain.into(), code.into()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: format!("valeur de référence {domain}"),
        id: code.to_string(),
    })
}

/// Id of a legacy lookup value (`lookup_domains.domain_key` + code).
pub(crate) async fn lookup_value_id(db: &impl ConnectionTrait, domain_key: &str, code: &str) -> AppResult<i64> {
    scalar_i64(
        db,
        "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
         WHERE ld.domain_key = ? AND lv.code = ? LIMIT 1",
        vec![domain_key.into(), code.into()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: format!("lookup {domain_key}"),
        id: code.to_string(),
    })
}

/// Current `row_version` of a row (services want it on every change).
pub(crate) async fn row_version(db: &impl ConnectionTrait, table: &str, id: i64) -> AppResult<i64> {
    scalar_i64(
        db,
        &format!("SELECT row_version FROM {table} WHERE id = ?"),
        vec![id.into()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: table.to_string(),
        id: id.to_string(),
    })
}
