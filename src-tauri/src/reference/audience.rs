//! Reference domain audience: who maintains the list of a domain.
//!
//! Every domain carries a stored audience (`reference_domains.audience`, migration 196):
//!
//! - **SYSTEM** — shipped and maintained by Maintafox through releases (migrations, the startup
//!   integrity pass, module catalog seeders). The program's logic depends on its codes, so no plant
//!   edits it. It is hidden from the Données de référence screens and every user-facing write is
//!   refused ([`GATE_REF_SYSTEM_DOMAIN_READ_ONLY`]), whatever its A/B/C category. Its ids, versions,
//!   links and picker reads are unchanged: OT, DI, RAMS… still list its values.
//! - **COMPANY** — differs from one company to the next and is configured by the plant. It is listed
//!   in Données de référence and keeps its A/B/C behaviour (`governance`): category A still means
//!   "visible, read-only", B live edits, C draft → publish.
//!
//! So SYSTEM is a stronger state than category A: A is "the plant sees it but cannot change it",
//! SYSTEM is "this is part of the program, the plant does not manage it at all".
//!
//! The audience is a property of the domain, never chosen by a user: seeders declare it when they
//! create a domain, and domains created through `domains::create_reference_domain` are always
//! COMPANY. The guard lives in the user-facing service functions (the ones the IPC commands call);
//! seeders write their rows with their own SQL and are not affected.
//!
//! The Données de référence tree also shows a few catalogs that are module tables rather than
//! reference domains (OT statuses, the urgency scale's company attributes, article families…). Their audience is declared here in
//! [`MODULE_CATALOGS`] and served to the UI by `list_reference_module_catalogs`, so the frontend has
//! no list of its own.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::domains::ReferenceDomain;

/// Stable gate code of a refused write on a SYSTEM domain (translated under `errors:gateError`).
pub const GATE_REF_SYSTEM_DOMAIN_READ_ONLY: &str = "GATE_REF_SYSTEM_DOMAIN_READ_ONLY";

/// Allowed values of `reference_domains.audience` (CHECK constraint of migration 196).
pub const DOMAIN_AUDIENCES: &[&str] = &["system", "company"];

/// Who maintains a reference domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DomainAudience {
    /// Shipped and maintained by Maintafox; hidden from the management screens, read-only.
    System,
    /// Configured by the company; listed and editable under its A/B/C category.
    Company,
}

impl DomainAudience {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Company => "company",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "company" => Some(Self::Company),
            _ => None,
        }
    }
}

fn refusal(domain_code: &str) -> AppError {
    AppError::ValidationFailed(vec![format!(
        "{GATE_REF_SYSTEM_DOMAIN_READ_ONLY}: the list '{domain_code}' is maintained by Maintafox \
         and cannot be changed."
    )])
}

/// Refuses a user-facing write on a SYSTEM domain.
pub fn assert_domain_editable(domain: &ReferenceDomain) -> AppResult<()> {
    match domain.audience {
        DomainAudience::System => Err(refusal(&domain.code)),
        DomainAudience::Company => Ok(()),
    }
}

/// `(code, audience)` of the domain matched by `sql` (one `?` parameter), if the row exists.
async fn audience_row(db: &impl ConnectionTrait, sql: &str, id: i64) -> AppResult<Option<(String, DomainAudience)>> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?
    else {
        return Ok(None);
    };
    let code: String = row.try_get("", "code")?;
    let raw: String = row.try_get("", "audience")?;
    let audience = DomainAudience::parse(&raw).ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!(
            "reference_domains.audience '{raw}' of domain '{code}' is not system|company"
        ))
    })?;
    Ok(Some((code, audience)))
}

/// Refuses the write when the row resolves to a SYSTEM domain. A missing row is left to the
/// service, which reports its own "not found".
async fn assert_row_editable(db: &impl ConnectionTrait, sql: &str, id: i64) -> AppResult<()> {
    match audience_row(db, sql, id).await? {
        Some((code, DomainAudience::System)) => Err(refusal(&code)),
        _ => Ok(()),
    }
}

/// Write guard for an operation addressed by domain id.
pub async fn assert_domain_id_editable(db: &impl ConnectionTrait, domain_id: i64) -> AppResult<()> {
    assert_row_editable(
        db,
        "SELECT d.code, d.audience FROM reference_domains d WHERE d.id = ?",
        domain_id,
    )
    .await
}

/// Write guard for an operation addressed by value id.
pub async fn assert_value_id_editable(db: &impl ConnectionTrait, value_id: i64) -> AppResult<()> {
    assert_row_editable(
        db,
        "SELECT d.code, d.audience FROM reference_values v \
           JOIN reference_sets s ON s.id = v.set_id \
           JOIN reference_domains d ON d.id = s.domain_id \
          WHERE v.id = ?",
        value_id,
    )
    .await
}

/// Write guard for an operation addressed by alias id.
pub async fn assert_alias_id_editable(db: &impl ConnectionTrait, alias_id: i64) -> AppResult<()> {
    assert_row_editable(
        db,
        "SELECT d.code, d.audience FROM reference_aliases a \
           JOIN reference_values v ON v.id = a.reference_value_id \
           JOIN reference_sets s ON s.id = v.set_id \
           JOIN reference_domains d ON d.id = s.domain_id \
          WHERE a.id = ?",
        alias_id,
    )
    .await
}

/// Seeder helper: records the audience of a product domain the seeder has just ensured.
///
/// For seeders that a migration older than 196 also runs (the WO close-out coding catalog is seeded
/// by migration 178): before migration 196 the column does not exist yet, so nothing is written and
/// migration 196 backfills the domain. Afterwards the declared audience is written on every run.
pub async fn declare_domain_audience(
    db: &impl ConnectionTrait,
    domain_code: &str,
    audience: DomainAudience,
) -> AppResult<()> {
    let has_column = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info('reference_domains') WHERE name = 'audience'",
        ))
        .await?
        .is_some();
    if !has_column {
        return Ok(());
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET audience = ? WHERE UPPER(TRIM(code)) = ?",
        [audience.as_str().into(), domain_code.trim().to_ascii_uppercase().into()],
    ))
    .await?;
    Ok(())
}

// ─── Module catalogs shown in the Données de référence tree ─────────────────────

/// A catalog of another module (its own table) listed in the Données de référence tree.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModuleCatalog {
    /// Stable code the UI keys the catalog by.
    pub code: &'static str,
    pub audience: DomainAudience,
}

/// Audience of the module catalogs. The OT statuses drive the work-order state machine, so they
/// belong to the program. Article families and tax categories are the company's own. The
/// notification categories are the program's (each has a generator); who receives them, the
/// escalation delays and the retention are the company's (`NOTIFICATION.RULES`).
///
/// `WORK.URGENCY_SCALE` is the plant's view of the shared DI / OT urgency scale: the five levels
/// are the SYSTEM domain `WORK.URGENCY` (fixed codes, hidden from the generic editor), and this
/// catalog edits only their company attributes, labels and target times
/// (`reference::urgency_scale`). The OT types and their EN 13306 natures are SYSTEM reference
/// domains since migration 198 (`WORK.ORDER_TYPES`, `WORK.MAINTENANCE_NATURES`), hidden like any
/// SYSTEM domain.
pub const MODULE_CATALOGS: &[ModuleCatalog] = &[
    ModuleCatalog {
        code: "INVENTORY.ARTICLE_FAMILY",
        audience: DomainAudience::Company,
    },
    ModuleCatalog {
        code: "INVENTORY.TAX_CATEGORY",
        audience: DomainAudience::Company,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_URGENCY_SCALE,
        audience: DomainAudience::Company,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_WORK_ORDER_STATUSES,
        audience: DomainAudience::System,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_NOTIFICATION_RULES,
        audience: DomainAudience::Company,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_NOTIFICATION_CATEGORIES,
        audience: DomainAudience::System,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_PERMIT_TYPES,
        audience: DomainAudience::Company,
    },
    ModuleCatalog {
        code: MODULE_CATALOG_FMECA_RATING_SCALES,
        audience: DomainAudience::Company,
    },
];

pub const MODULE_CATALOG_URGENCY_SCALE: &str = "WORK.URGENCY_SCALE";
pub const MODULE_CATALOG_WORK_ORDER_STATUSES: &str = "WO.WORK_ORDER_STATUSES";
/// The plant's notification settings: who receives each category, acknowledgement, escalation
/// delays and targets, retention (`notifications::config`).
pub const MODULE_CATALOG_NOTIFICATION_RULES: &str = "NOTIFICATION.RULES";
/// The notification categories (event types), their module and severity: fixed by Maintafox.
pub const MODULE_CATALOG_NOTIFICATION_CATEGORIES: &str = "NOTIFICATION.CATEGORIES";
/// The plant's work-permit types (`permit_types`, edited through `upsert_permit_type`: permit
/// issuer, and administrator + step-up to switch the energy-isolation rule off; the LOTO type
/// keeps it whatever the editor).
pub const MODULE_CATALOG_PERMIT_TYPES: &str = "PERMIT.TYPES";
/// The plant's FMECA rating grids: the SYSTEM lists `RELIABILITY.FMECA_*` (fixed scores and zone
/// codes), of which this catalog edits the definitions and attributes (`reliability::study::scales`,
/// right `ram.manage`).
pub const MODULE_CATALOG_FMECA_RATING_SCALES: &str = "RELIABILITY.FMECA_SCALES";

/// Audience of a module catalog, `None` for an unknown code.
pub fn module_catalog_audience(code: &str) -> Option<DomainAudience> {
    let code = code.trim();
    MODULE_CATALOGS
        .iter()
        .find(|c| c.code.eq_ignore_ascii_case(code))
        .map(|c| c.audience)
}

/// Refuses a user-facing write on a SYSTEM module catalog.
pub fn assert_module_catalog_editable(code: &str) -> AppResult<()> {
    match module_catalog_audience(code) {
        Some(DomainAudience::System) => Err(refusal(code)),
        Some(DomainAudience::Company) => Ok(()),
        None => Err(AppError::Internal(anyhow::anyhow!("unknown module catalog '{code}'"))),
    }
}
