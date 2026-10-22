//! Currencies for procurement: the tenant currency (Settings `locale.currency_code`), supplier /
//! PO currencies and dated exchange rates entered by users (`currency_exchange_rates`).
//!
//! Stock is always valued in the tenant currency. A PO may be priced in the supplier currency;
//! the rate is captured on each goods receipt (entered, or taken from the rate table valid on the
//! posting date) and the received quantity is valued at `PO price × rate`. No rate is built in.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{CurrencyExchangeRate, CurrencyExchangeRateInput, ResolvedExchangeRate};

/// Where the rate of a receipt came from (`goods_receipts.exchange_rate_source`).
pub const RATE_SOURCE_TENANT: &str = "TENANT_CURRENCY";
pub const RATE_SOURCE_ENTERED: &str = "ENTERED";
pub const RATE_SOURCE_TABLE: &str = "RATE_TABLE";

/// ISO 4217 code: three letters, upper-cased.
pub fn normalize_currency_code(value: &str) -> AppResult<String> {
    let code = value.trim().to_uppercase();
    if code.len() != 3 || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_FIN_CURRENCY_CODE_INVALID: Currency code '{}' is not an ISO 4217 code (three letters, e.g. EUR).",
            value.trim()
        )]));
    }
    Ok(code)
}

pub fn normalize_optional_currency(value: Option<&str>) -> AppResult<Option<String>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => Ok(Some(normalize_currency_code(v)?)),
        None => Ok(None),
    }
}

/// Settings key of the tenant currency.
pub const CURRENCY_SETTING: &str = "locale.currency_code";

/// Amounts are already stored in the tenant currency: valued stock movements, goods receipts,
/// closed valuation periods, posted settlements or budget actuals. Changing the currency then
/// would relabel every one of them, so the setting is locked (same rule as the fiscal start).
pub async fn tenant_currency_in_use<C: ConnectionTrait>(db: &C) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT EXISTS (SELECT 1 FROM inventory_transactions
                            WHERE unit_cost IS NOT NULL OR extended_cost IS NOT NULL)
                 OR EXISTS (SELECT 1 FROM goods_receipts)
                 OR EXISTS (SELECT 1 FROM inventory_valuation_periods)
                 OR EXISTS (SELECT 1 FROM wo_cost_settlements WHERE status = 'posted')
                 OR EXISTS (SELECT 1 FROM budget_actuals) AS in_use"
                .to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "in_use").ok())
        .unwrap_or(0)
        != 0)
}

/// Settings guard for `locale.currency_code` (called by `settings::set_setting`): an ISO 4217
/// code, and no change once amounts are stored in the current one.
pub async fn guard_currency_write<C: ConnectionTrait>(db: &C, key: &str, value_json: &str) -> AppResult<()> {
    if key != CURRENCY_SETTING {
        return Ok(());
    }
    let parsed: String = serde_json::from_str(value_json.trim()).unwrap_or_else(|_| value_json.trim().to_string());
    let code = normalize_currency_code(&parsed)?;
    if tenant_currency(db).await.ok().as_deref() == Some(code.as_str()) {
        return Ok(());
    }
    if tenant_currency_in_use(db).await? {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_CURRENCY_LOCKED: stock, receipts or postings are already valued in the tenant currency; \
             changing it would relabel every stored amount"
                .to_string(),
        ]));
    }
    Ok(())
}

/// Tenant currency from Settings (`locale.currency_code`, stored as a JSON string).
pub async fn tenant_currency<C: ConnectionTrait>(db: &C) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = 'locale.currency_code' LIMIT 1"
                .to_string(),
        ))
        .await?;
    let raw: Option<String> = row.and_then(|r| r.try_get("", "setting_value_json").ok());
    let Some(raw) = raw else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_TENANT_CURRENCY_MISSING: The tenant currency is not configured (Settings › Localization)."
                .to_string(),
        ]));
    };
    let parsed: String = serde_json::from_str(&raw).unwrap_or(raw);
    normalize_currency_code(&parsed)
}

/// ISO date (YYYY-MM-DD) or an error naming the field.
pub fn parse_iso_date(value: &str, field: &str) -> AppResult<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a date (YYYY-MM-DD), got '{value}'.")]))
}

pub fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

/// Latest active rate `from → to` valid on `on_date` (YYYY-MM-DD).
pub async fn rate_on<C: ConnectionTrait>(
    db: &C,
    from_currency: &str,
    to_currency: &str,
    on_date: &str,
) -> AppResult<Option<(f64, String)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rate, valid_from FROM currency_exchange_rates
             WHERE from_currency = ? AND to_currency = ? AND is_active = 1 AND valid_from <= ?
             ORDER BY valid_from DESC, id DESC LIMIT 1",
            [from_currency.into(), to_currency.into(), on_date.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => Some((r.try_get("", "rate")?, r.try_get("", "valid_from")?)),
        None => None,
    })
}

/// Rate to value `currency` amounts in the tenant currency on `on_date`. 1.0 for the tenant
/// currency itself; None when no rate is stored.
pub async fn rate_to_tenant<C: ConnectionTrait>(
    db: &C,
    currency: Option<&str>,
    on_date: &str,
) -> AppResult<Option<f64>> {
    let tenant = tenant_currency(db).await?;
    match currency {
        None => Ok(Some(1.0)),
        Some(c) if c == tenant => Ok(Some(1.0)),
        Some(c) => Ok(rate_on(db, c, &tenant, on_date).await?.map(|(rate, _)| rate)),
    }
}

pub async fn resolve_rate(
    db: &DatabaseConnection,
    from_currency: String,
    on_date: Option<String>,
) -> AppResult<ResolvedExchangeRate> {
    let from = normalize_currency_code(&from_currency)?;
    let tenant = tenant_currency(db).await?;
    let date = on_date.unwrap_or_else(today);
    parse_iso_date(&date, "on_date")?;
    if from == tenant {
        return Ok(ResolvedExchangeRate {
            from_currency: from,
            to_currency: tenant,
            rate: Some(1.0),
            valid_from: None,
        });
    }
    let found = rate_on(db, &from, &tenant, &date).await?;
    Ok(ResolvedExchangeRate {
        from_currency: from,
        to_currency: tenant,
        rate: found.as_ref().map(|(r, _)| *r),
        valid_from: found.map(|(_, d)| d),
    })
}

fn map_rate(row: sea_orm::QueryResult) -> AppResult<CurrencyExchangeRate> {
    Ok(CurrencyExchangeRate {
        id: row.try_get("", "id")?,
        from_currency: row.try_get("", "from_currency")?,
        to_currency: row.try_get("", "to_currency")?,
        rate: row.try_get("", "rate")?,
        valid_from: row.try_get("", "valid_from")?,
        source: row.try_get("", "source")?,
        is_active: row.try_get("", "is_active")?,
        created_by_name: row.try_get("", "created_by_name").ok().flatten(),
        created_at: row.try_get("", "created_at")?,
    })
}

const RATE_SELECT: &str = "SELECT r.id, r.from_currency, r.to_currency, r.rate, r.valid_from, r.source, r.is_active,
            COALESCE(ua.display_name, ua.username) AS created_by_name, r.created_at
     FROM currency_exchange_rates r
     LEFT JOIN user_accounts ua ON ua.id = r.created_by_id";

pub async fn list_rates(db: &DatabaseConnection) -> AppResult<Vec<CurrencyExchangeRate>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{RATE_SELECT} ORDER BY r.from_currency, r.to_currency, r.valid_from DESC, r.id DESC"),
        ))
        .await?;
    rows.into_iter().map(map_rate).collect()
}

pub async fn create_rate(
    db: &DatabaseConnection,
    input: CurrencyExchangeRateInput,
    actor_id: Option<i64>,
) -> AppResult<CurrencyExchangeRate> {
    let from = normalize_currency_code(&input.from_currency)?;
    let to = match input.to_currency.as_deref() {
        Some(t) if !t.trim().is_empty() => normalize_currency_code(t)?,
        _ => tenant_currency(db).await?,
    };
    if from == to {
        return Err(AppError::ValidationFailed(vec![
            "A rate converts between two different currencies.".to_string(),
        ]));
    }
    if !input.rate.is_finite() || input.rate <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_FIN_RATE_POSITIVE: The rate must be greater than 0.".to_string(),
        ]));
    }
    parse_iso_date(&input.valid_from, "valid_from")?;
    let duplicate = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM currency_exchange_rates
             WHERE from_currency = ? AND to_currency = ? AND valid_from = ? AND is_active = 1",
            [from.clone().into(), to.clone().into(), input.valid_from.trim().into()],
        ))
        .await?;
    if duplicate.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "A {from} → {to} rate already exists for {}; deactivate it first.",
            input.valid_from.trim()
        )]));
    }
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO currency_exchange_rates
            (entity_sync_id, from_currency, to_currency, rate, valid_from, source, is_active, row_version,
             created_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, 1, ?, ?, ?)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            from.into(),
            to.into(),
            input.rate.into(),
            input.valid_from.trim().to_string().into(),
            input
                .source
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map_or(Value::String(None), Value::from),
            actor_id.map_or(Value::BigInt(None), Value::from),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("{RATE_SELECT} WHERE r.id = last_insert_rowid()"),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created rate not found")))?;
    map_rate(row)
}

pub async fn deactivate_rate(db: &DatabaseConnection, rate_id: i64) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE currency_exchange_rates
             SET is_active = 0, row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [rate_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "currency_exchange_rates".to_string(),
            id: rate_id.to_string(),
        });
    }
    Ok(())
}
