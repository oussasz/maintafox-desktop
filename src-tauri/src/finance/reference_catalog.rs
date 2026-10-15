//! Finance catalogs in the governed reference data (Données de référence).
//!
//! The lists Finance uses are reference domains, never lists defined in the finance module:
//! - `FINANCE.COST_CATEGORIES`: the categories budget lines, actuals and commitments are kept in.
//!   `labor`, `parts`, `services` and `other` are the categories OT settlement posts to; with the
//!   planning categories (`contracts`, `shutdown`, `capex`) they are product-seeded and locked
//!   (`reference::governance`), so a tenant cannot deactivate a category settlement relies on.
//!   Plants add their own categories through the reference manager (draft → publish).
//! - `FINANCE.VARIANCE_REASONS`: why a cost centre × category is off budget (variance reviews).
//!   Seeded with the usual maintenance drivers; plants extend, relabel or retire them.
//!
//! Labels are French (primary language of the catalogs, like the other system seeds) and
//! `description` carries the English term, as for the WO close-out catalogs.
//!
//! Called from migration 186 (a freshly migrated database is usable) and from the startup
//! catalog integrity pass (a tenant wipe keeps migrations but deletes catalog rows).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reference::governance::derive_category_for_persist;

pub const DOMAIN_COST_CATEGORIES: &str = "FINANCE.COST_CATEGORIES";
pub const DOMAIN_VARIANCE_REASONS: &str = "FINANCE.VARIANCE_REASONS";

const SYSTEM_META: &str = r#"{"origin":"system"}"#;

/// (code, French label, English term)
type Row = (&'static str, &'static str, &'static str);

const COST_CATEGORIES: &[Row] = &[
    ("labor", "Main-d'œuvre", "Labour"),
    ("parts", "Pièces", "Parts"),
    ("services", "Sous-traitance et services", "Contractors and services"),
    ("other", "Autres coûts", "Other costs"),
    ("contracts", "Contrats", "Contracts"),
    ("shutdown", "Arrêts programmés", "Shutdowns"),
    ("capex", "Investissement", "Capital"),
];

const VARIANCE_REASONS: &[Row] = &[
    ("emergency_break_in", "Dépannages urgents", "Emergency break-ins"),
    ("vendor_delay", "Retard fournisseur", "Vendor delay"),
    ("labor_overrun", "Dépassement d'heures", "Labour overrun"),
    ("estimate_error", "Erreur d'estimation", "Estimate error"),
    ("scope_change", "Changement de périmètre", "Scope change"),
    ("price_increase", "Hausse de prix", "Price increase"),
    ("permit_delay", "Attente de permis", "Permit delay"),
    ("availability_loss", "Perte de disponibilité", "Availability loss"),
    ("repeat_failure", "Panne répétitive", "Repeat failure"),
    ("shutdown_scope_growth", "Extension d'arrêt", "Shutdown scope growth"),
];

async fn ensure_domain(conn: &impl ConnectionTrait, code: &str, name: &str) -> AppResult<()> {
    let category = derive_category_for_persist(code, "tenant_managed");
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         SELECT ?, ?, 'flat', 'tenant_managed', ?, 1, NULL, \
                strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE NOT EXISTS (SELECT 1 FROM reference_domains WHERE UPPER(TRIM(code)) = ?)",
        [
            code.into(),
            name.into(),
            category.as_str().into(),
            code.to_ascii_uppercase().into(),
        ],
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
        [code.to_ascii_uppercase().into()],
    ))
    .await?;
    Ok(())
}

/// Inserts one product value into the latest published set unless the code already exists in
/// the domain (tenant changes to existing rows are never touched).
async fn ensure_value(conn: &impl ConnectionTrait, domain_code: &str, row: Row, sort_order: i64) -> AppResult<()> {
    let (code, label, english) = row;
    let sync_id = crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain_code}:{code}"));
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
          semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, ?, ?, NULL, NULL, NULL, NULL, 1, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS ( \
                SELECT 1 FROM reference_values rv \
                  JOIN reference_sets s2 ON s2.id = rv.set_id \
                 WHERE s2.domain_id = d.id AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
            ) \
          ORDER BY rs.version_no DESC \
          LIMIT 1",
        [
            sync_id.into(),
            code.into(),
            label.into(),
            english.into(),
            sort_order.into(),
            SYSTEM_META.into(),
            domain_code.to_ascii_uppercase().into(),
            code.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn ensure_catalog(conn: &impl ConnectionTrait, domain_code: &str, name: &str, rows: &[Row]) -> AppResult<()> {
    ensure_domain(conn, domain_code, name).await?;
    for (i, row) in rows.iter().enumerate() {
        ensure_value(conn, domain_code, *row, (i as i64) + 1).await?;
    }
    Ok(())
}

/// Ensure the finance domains exist with their starter values.
pub async fn ensure_finance_reference_catalogs(conn: &impl ConnectionTrait) -> AppResult<()> {
    ensure_catalog(conn, DOMAIN_COST_CATEGORIES, "Catégories de coût", COST_CATEGORIES).await?;
    ensure_catalog(
        conn,
        DOMAIN_VARIANCE_REASONS,
        "Causes d'écart budgétaire",
        VARIANCE_REASONS,
    )
    .await?;
    Ok(())
}

/// Whether `code` is an active value of the latest published set of `domain_code`.
pub async fn is_active_code(conn: &impl ConnectionTrait, domain_code: &str, code: &str) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.is_active FROM reference_values rv
               JOIN reference_sets rs ON rs.id = rv.set_id
               JOIN reference_domains d ON d.id = rs.domain_id
              WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?))
              ORDER BY rs.version_no DESC LIMIT 1",
            [domain_code.to_ascii_uppercase().into(), code.into()],
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<i64>("", "is_active").ok())
        .is_some_and(|v| v != 0))
}

/// Refuses a code that is not an active value of the domain.
pub async fn ensure_active_code(
    conn: &impl ConnectionTrait,
    domain_code: &str,
    code: &str,
    what_en: &str,
) -> AppResult<()> {
    if is_active_code(conn, domain_code, code).await? {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "GATE_FIN_REFERENCE_VALUE_INACTIVE: {what_en} '{code}' is not an active value of reference domain {domain_code}"
        )]))
    }
}

/// One value of a finance catalog, as the finance screens show it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FinanceCatalogValue {
    pub code: String,
    /// French label (catalog language).
    pub label: String,
    /// English term for product values; tenant values may leave it empty.
    pub description: Option<String>,
    pub is_active: bool,
    /// Product-seeded value (`metadata_json.origin = "system"`).
    pub is_system: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FinanceCatalogs {
    pub cost_categories: Vec<FinanceCatalogValue>,
    pub variance_reasons: Vec<FinanceCatalogValue>,
    /// Fiscal-year start month (Settings › Localization), so screens name periods by month.
    pub fiscal_year_start_month: i64,
}

async fn catalog_values(conn: &impl ConnectionTrait, domain_code: &str) -> AppResult<Vec<FinanceCatalogValue>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.description, rv.is_active, rv.metadata_json, COALESCE(rv.sort_order, 0) AS sort_order
               FROM reference_values rv
              WHERE rv.set_id = (
                    SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id
                     WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published'
                     ORDER BY rs.version_no DESC LIMIT 1)
              ORDER BY sort_order, rv.code",
            [domain_code.to_ascii_uppercase().into()],
        ))
        .await?;
    rows.into_iter()
        .map(|r| {
            let metadata: Option<String> = r.try_get("", "metadata_json").ok().flatten();
            Ok(FinanceCatalogValue {
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                description: r
                    .try_get::<Option<String>>("", "description")?
                    .filter(|d| !d.trim().is_empty()),
                is_active: r.try_get::<i64>("", "is_active")? != 0,
                is_system: crate::reference::governance::metadata_marks_system_origin(metadata.as_deref()),
                sort_order: r.try_get("", "sort_order")?,
            })
        })
        .collect()
}

/// Both finance catalogs (latest published set, active and retired values).
pub async fn list_finance_catalogs(conn: &impl ConnectionTrait) -> AppResult<FinanceCatalogs> {
    Ok(FinanceCatalogs {
        cost_categories: catalog_values(conn, DOMAIN_COST_CATEGORIES).await?,
        variance_reasons: catalog_values(conn, DOMAIN_VARIANCE_REASONS).await?,
        fiscal_year_start_month: crate::finance::fiscal::fiscal_start_month(conn).await?,
    })
}
