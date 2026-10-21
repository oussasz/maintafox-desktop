//! Currencies under reference-data governance (owner decision: one COMPANY list in Données de
//! référence for every currency of the plant).
//!
//! `INVENTORY.CURRENCY` is a **COMPANY** list (category C, draft → validate → publish) of ISO 4217
//! currencies. A value carries its ISO code (`code`, three letters), its French name (`label`),
//! its English name (`metadata_json.label_en`), its symbol (`metadata_json.symbol`), its number of
//! minor units (`metadata_json.minor_units`, 0 to 4) and its ISO numeric code (`external_code`).
//! The ISO currencies of the plant's region and the main trading currencies ([`ISO_CURRENCIES`])
//! are shipped with `metadata_json.origin = "system"` (protected like standard units); a plant
//! adds any other ISO currency through the reference manager, checked on the server
//! ([`checked_currency_metadata`]).
//!
//! **One list, two readers, no data rewrite.** The list keeps the identity of the existing
//! `inventory.currency` domain, whose operational copy in `lookup_values` (same codes) is what
//! stock costs, supplier prices and valuation policies link to by id. Those links are untouched:
//! every publish of the list mirrors it into that copy ([`mirror_into_lookup`]) — a new currency
//! appears there, a renamed one is relabelled, a retired one is deactivated (never deleted), and
//! the ids already stored keep pointing at the same codes. The tenant currency
//! (`locale.currency_code`, Settings) stores a code and must be an active currency of the
//! published list ([`ensure_active_currency`]); it stays locked once amounts are valued in it
//! (`inventory::fx::guard_currency_write`).
//!
//! Called from migration 214 and from the startup catalog integrity pass.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::governance::derive_category_for_persist;

pub const CURRENCY_DOMAIN: &str = "INVENTORY.CURRENCY";
/// The operational copy of the list (`lookup_domains.domain_key`) that cost links point at.
pub const LOOKUP_DOMAIN_KEY: &str = "inventory.currency";

/// A currency that is not an active value of the published currency list.
pub const GATE_CURRENCY_NOT_ACTIVE: &str = "GATE_CURRENCY_NOT_ACTIVE";

/// A currency still in use cannot be deactivated.
pub const GATE_CURRENCY_IN_USE: &str = "GATE_CURRENCY_IN_USE";

/// One ISO 4217 currency shipped with the application.
#[derive(Debug, Clone, Copy)]
pub struct IsoCurrency {
    pub code: &'static str,
    pub numeric: &'static str,
    pub label_fr: &'static str,
    pub label_en: &'static str,
    pub symbol: &'static str,
    pub minor_units: i64,
}

/// The currencies offered until now in Settings plus the two the stock lookup shipped (EUR, USD).
pub const ISO_CURRENCIES: &[IsoCurrency] = &[
    IsoCurrency {
        code: "DZD",
        numeric: "012",
        label_fr: "Dinar algérien",
        label_en: "Algerian dinar",
        symbol: "DA",
        minor_units: 2,
    },
    IsoCurrency {
        code: "MAD",
        numeric: "504",
        label_fr: "Dirham marocain",
        label_en: "Moroccan dirham",
        symbol: "DH",
        minor_units: 2,
    },
    IsoCurrency {
        code: "TND",
        numeric: "788",
        label_fr: "Dinar tunisien",
        label_en: "Tunisian dinar",
        symbol: "DT",
        minor_units: 3,
    },
    IsoCurrency {
        code: "EUR",
        numeric: "978",
        label_fr: "Euro",
        label_en: "Euro",
        symbol: "€",
        minor_units: 2,
    },
    IsoCurrency {
        code: "USD",
        numeric: "840",
        label_fr: "Dollar des États-Unis",
        label_en: "US dollar",
        symbol: "$",
        minor_units: 2,
    },
    IsoCurrency {
        code: "GBP",
        numeric: "826",
        label_fr: "Livre sterling",
        label_en: "Pound sterling",
        symbol: "£",
        minor_units: 2,
    },
    IsoCurrency {
        code: "CHF",
        numeric: "756",
        label_fr: "Franc suisse",
        label_en: "Swiss franc",
        symbol: "CHF",
        minor_units: 2,
    },
    IsoCurrency {
        code: "CAD",
        numeric: "124",
        label_fr: "Dollar canadien",
        label_en: "Canadian dollar",
        symbol: "$ CA",
        minor_units: 2,
    },
    IsoCurrency {
        code: "SAR",
        numeric: "682",
        label_fr: "Riyal saoudien",
        label_en: "Saudi riyal",
        symbol: "SAR",
        minor_units: 2,
    },
    IsoCurrency {
        code: "AED",
        numeric: "784",
        label_fr: "Dirham des Émirats arabes unis",
        label_en: "UAE dirham",
        symbol: "AED",
        minor_units: 2,
    },
];

pub fn is_currency_domain(domain_code: &str) -> bool {
    domain_code.eq_ignore_ascii_case(CURRENCY_DOMAIN)
}

fn metadata_for(c: &IsoCurrency) -> String {
    serde_json::json!({
        "origin": "system",
        "label_en": c.label_en,
        "symbol": c.symbol,
        "minor_units": c.minor_units,
    })
    .to_string()
}

/// Ensure the currency list exists (domain, published set, shipped currencies) and mirror it
/// into the operational copy. Existing values, including a plant's changes, are never touched.
pub async fn ensure_currency_catalog(conn: &impl ConnectionTrait) -> AppResult<()> {
    let category = derive_category_for_persist(CURRENCY_DOMAIN, "tenant_managed");
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         SELECT ?, 'Devises', 'flat', 'tenant_managed', ?, 1, NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE NOT EXISTS (SELECT 1 FROM reference_domains WHERE UPPER(TRIM(code)) = ?)",
        [CURRENCY_DOMAIN.into(), category.as_str().into(), CURRENCY_DOMAIN.into()],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_sets \
         (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at) \
         SELECT d.id, COALESCE((SELECT MAX(rs.version_no) FROM reference_sets rs WHERE rs.domain_id = d.id), 0) + 1, \
                'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
           FROM reference_domains d \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS (SELECT 1 FROM reference_sets rs WHERE rs.domain_id = d.id AND rs.status = 'published')",
        [CURRENCY_DOMAIN.into()],
    ))
    .await?;
    crate::reference::audience::declare_domain_audience(
        conn,
        CURRENCY_DOMAIN,
        crate::reference::audience::DomainAudience::Company,
    )
    .await?;

    for (i, c) in ISO_CURRENCIES.iter().enumerate() {
        let sync_id =
            crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{CURRENCY_DOMAIN}:{}", c.code));
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values \
             (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, NULL, ?, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS ( \
                    SELECT 1 FROM reference_values rv \
                      JOIN reference_sets s2 ON s2.id = rv.set_id \
                     WHERE s2.domain_id = d.id AND UPPER(TRIM(rv.code)) = ? \
                ) \
              ORDER BY rs.version_no DESC \
              LIMIT 1",
            [
                sync_id.into(),
                c.code.into(),
                c.label_fr.into(),
                ((i as i64) + 1).into(),
                c.numeric.into(),
                metadata_for(c).into(),
                CURRENCY_DOMAIN.into(),
                c.code.into(),
            ],
        ))
        .await?;
    }

    mirror_into_lookup(conn).await?;
    Ok(())
}

/// One currency of the published list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CurrencyOption {
    pub code: String,
    pub label: String,
    pub label_en: Option<String>,
    pub symbol: Option<String>,
    pub minor_units: Option<i64>,
    pub is_active: bool,
}

/// The values of the published currency list, in list order.
pub async fn published_currencies(conn: &impl ConnectionTrait) -> AppResult<Vec<CurrencyOption>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.is_active, rv.metadata_json \
               FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? \
              ORDER BY rv.sort_order, rv.code",
            [CURRENCY_DOMAIN.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let meta: serde_json::Value = r
            .try_get::<Option<String>>("", "metadata_json")?
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or(serde_json::Value::Null);
        out.push(CurrencyOption {
            code: r.try_get("", "code")?,
            label: r.try_get("", "label")?,
            label_en: meta
                .get("label_en")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            symbol: meta
                .get("symbol")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            minor_units: meta.get("minor_units").and_then(serde_json::Value::as_i64),
            is_active: r.try_get::<i32>("", "is_active")? == 1,
        });
    }
    Ok(out)
}

/// Mirror the published list into the operational copy (`lookup_values` of
/// `inventory.currency`): add the currencies it lacks, relabel and (de)activate the others to
/// match the list. Rows are never deleted and their ids never change, so every cost already
/// linked to a currency keeps its link. Returns the number of rows written.
pub async fn mirror_into_lookup(conn: &impl ConnectionTrait) -> AppResult<u64> {
    let domain_id: Option<i64> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
            [LOOKUP_DOMAIN_KEY.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "id").ok());
    let Some(domain_id) = domain_id else {
        // No operational copy on this database (it is created by the start-up seeder): nothing
        // links to a lookup currency yet.
        return Ok(0);
    };
    let now = chrono::Utc::now().to_rfc3339();
    let mut written = 0;
    for (i, c) in published_currencies(conn).await?.iter().enumerate() {
        if !c.is_active {
            // Retiring a currency (active in the operational copy, inactive in the list being
            // published): refused while it is still in use.
            let active_now = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS x FROM lookup_values \
                      WHERE domain_id = ? AND code = ? AND deleted_at IS NULL AND is_active = 1",
                    [domain_id.into(), c.code.clone().into()],
                ))
                .await?
                .is_some();
            if active_now {
                ensure_currency_not_in_use(conn, &c.code).await?;
            }
        }
        // Only the shipped ISO currencies are system rows; a currency the plant added is not.
        let is_system = ISO_CURRENCIES.iter().any(|iso| iso.code.eq_ignore_ascii_case(&c.code));
        let en = c.label_en.clone().unwrap_or_else(|| c.label.clone());
        let sync_id =
            crate::sync::identity::seeded_entity_sync_id("lookup_values", &format!("{LOOKUP_DOMAIN_KEY}:{}", c.code));
        written += conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO lookup_values \
                 (entity_sync_id, domain_id, code, label, fr_label, en_label, color, sort_order, is_active, is_system, \
                  created_at, updated_at, row_version) \
                 SELECT ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, 1 \
                  WHERE NOT EXISTS (SELECT 1 FROM lookup_values WHERE domain_id = ? AND code = ? AND deleted_at IS NULL)",
                [
                    sync_id.into(),
                    domain_id.into(),
                    c.code.clone().into(),
                    c.label.clone().into(),
                    c.label.clone().into(),
                    en.clone().into(),
                    ((i as i64) + 1).into(),
                    i32::from(c.is_active).into(),
                    i32::from(is_system).into(),
                    now.clone().into(),
                    now.clone().into(),
                    domain_id.into(),
                    c.code.clone().into(),
                ],
            ))
            .await?
            .rows_affected();
        written += conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE lookup_values \
                    SET label = ?, fr_label = ?, en_label = ?, is_active = ?, updated_at = ?, row_version = row_version + 1 \
                  WHERE domain_id = ? AND code = ? AND deleted_at IS NULL \
                    AND (label IS NOT ? OR fr_label IS NOT ? OR en_label IS NOT ? OR is_active IS NOT ?)",
                [
                    c.label.clone().into(),
                    c.label.clone().into(),
                    en.clone().into(),
                    i32::from(c.is_active).into(),
                    now.clone().into(),
                    domain_id.into(),
                    c.code.clone().into(),
                    c.label.clone().into(),
                    c.label.clone().into(),
                    en.into(),
                    i32::from(c.is_active).into(),
                ],
            ))
            .await?
            .rows_affected();
    }
    Ok(written)
}

/// Where a currency is still used: the company currency (Settings), stock costs (balances,
/// movements, work-order parts, requisition lines), valuation periods, cost estimates and
/// settlements. Returns the uses as "French / English" labels, empty when unused.
pub async fn currency_uses(conn: &impl ConnectionTrait, code: &str) -> AppResult<Vec<&'static str>> {
    let code = code.trim().to_ascii_uppercase();
    let mut uses = Vec::new();
    if crate::inventory::fx::tenant_currency(conn).await.ok().as_deref() == Some(code.as_str()) {
        uses.push("devise de l'entreprise / company currency");
    }
    let lookup_id = "(SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
                      WHERE ld.domain_key = ? AND UPPER(lv.code) = ?)";
    let checks: [(&str, String, bool); 4] = [
        (
            "coûts de stock / stock costs",
            format!(
                "SELECT EXISTS (SELECT 1 FROM stock_balances WHERE valuation_currency_value_id IN {lookup_id}) \
                     OR EXISTS (SELECT 1 FROM inventory_transactions WHERE cost_currency_value_id IN {lookup_id}) \
                     OR EXISTS (SELECT 1 FROM work_order_parts WHERE cost_currency_value_id IN {lookup_id}) \
                     OR EXISTS (SELECT 1 FROM procurement_requisition_lines \
                                 WHERE projected_cost_currency_value_id IN {lookup_id}) AS used"
            ),
            true,
        ),
        (
            "périodes de valorisation / valuation periods",
            "SELECT EXISTS (SELECT 1 FROM inventory_valuation_periods WHERE UPPER(currency_code) = ?) AS used".into(),
            false,
        ),
        (
            "estimations de coût d'OT / work-order cost estimates",
            "SELECT EXISTS (SELECT 1 FROM wo_cost_estimates WHERE UPPER(currency_code) = ?) AS used".into(),
            false,
        ),
        (
            "règlements de coût d'OT / work-order cost settlements",
            "SELECT EXISTS (SELECT 1 FROM wo_cost_settlements WHERE UPPER(currency_code) = ?) AS used".into(),
            false,
        ),
    ];
    for (label, sql, by_lookup) in checks {
        let values: Vec<sea_orm::Value> = if by_lookup {
            (0..4)
                .flat_map(|_| [LOOKUP_DOMAIN_KEY.into(), code.clone().into()])
                .collect::<Vec<sea_orm::Value>>()
        } else {
            vec![code.clone().into()]
        };
        let used = conn
            .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
            .await?
            .and_then(|r| r.try_get::<i64>("", "used").ok())
            .unwrap_or(0)
            != 0;
        if used {
            uses.push(label);
        }
    }
    Ok(uses)
}

/// Refuse to deactivate a currency that is still in use ([`currency_uses`]), in French and
/// English.
pub async fn ensure_currency_not_in_use(conn: &impl ConnectionTrait, code: &str) -> AppResult<()> {
    let uses = currency_uses(conn, code).await?;
    if uses.is_empty() {
        return Ok(());
    }
    Err(AppError::ValidationFailed(vec![format!(
        "{GATE_CURRENCY_IN_USE}: la devise {code} ne peut pas être désactivée, elle est utilisée / \
         the currency {code} cannot be deactivated, it is in use: {}.",
        uses.join(", ")
    )]))
}

/// The tenant currency must be an active currency of the published list.
pub async fn ensure_active_currency(conn: &impl ConnectionTrait, code: &str) -> AppResult<()> {
    let active = published_currencies(conn)
        .await?
        .iter()
        .any(|c| c.is_active && c.code.eq_ignore_ascii_case(code));
    if active {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "{GATE_CURRENCY_NOT_ACTIVE}: '{code}' is not an active currency of the list (Données de référence › Devises)."
        )]))
    }
}

/// Check a currency value a plant adds or edits: an ISO 4217 code (three letters) and, in the
/// metadata, the number of minor units (0 to 4) and optionally a symbol (at most 8 characters)
/// and the English name. Returns the normalised metadata JSON.
pub fn checked_currency_metadata(code: &str, metadata_json: Option<&str>) -> AppResult<String> {
    crate::inventory::fx::normalize_currency_code(code)?;
    let meta: serde_json::Value = match metadata_json.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => serde_json::from_str(m)
            .map_err(|_| AppError::ValidationFailed(vec!["The currency details are not valid JSON.".into()]))?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    let Some(obj) = meta.as_object() else {
        return Err(AppError::ValidationFailed(vec![
            "The currency details must be an object.".into(),
        ]));
    };
    let minor = obj.get("minor_units").and_then(serde_json::Value::as_i64);
    if !matches!(minor, Some(0..=4)) {
        return Err(AppError::ValidationFailed(vec![
            "A currency needs its number of minor units (0 to 4, e.g. 2 for cents).".into(),
        ]));
    }
    if let Some(symbol) = obj.get("symbol") {
        match symbol.as_str() {
            Some(s) if !s.trim().is_empty() && s.chars().count() <= 8 => {}
            _ => {
                return Err(AppError::ValidationFailed(vec![
                    "The currency symbol must be 1 to 8 characters.".into(),
                ]))
            }
        }
    }
    Ok(meta.to_string())
}

#[cfg(test)]
#[path = "currencies_tests.rs"]
mod currencies_tests;
