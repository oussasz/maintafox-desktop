//! Governed lists of the PM execution dialogs (migration 226).
//!
//! - `PM.DEFERRAL_REASONS` — why an occurrence is deferred, missed or cancelled. A **COMPANY**
//!   list (category B): the starter reasons below are the codes the dialogs used to hard-code, so
//!   reasons already stored keep their meaning; each plant adds its own.
//! - `PM.FINDING_SEVERITY` — severity of a PM finding. A **SYSTEM** list: the program maps it to
//!   the urgency of the follow-up work (`urgency_scale`), so its codes are fixed.
//!
//! The server checks every reason and severity against these lists (case-insensitive, stored as
//! the list code); the screens read them through `list_pm_execution_options` (PM view right).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const REASONS_DOMAIN: &str = "PM.DEFERRAL_REASONS";
pub const SEVERITY_DOMAIN: &str = "PM.FINDING_SEVERITY";

struct Entry {
    code: &'static str,
    label_fr: &'static str,
    label_en: &'static str,
}

const REASONS: &[Entry] = &[
    Entry {
        code: "capacity_constraint",
        label_fr: "Manque de capacité (main-d'œuvre)",
        label_en: "Capacity constraint",
    },
    Entry {
        code: "asset_unavailable",
        label_fr: "Équipement non disponible",
        label_en: "Asset unavailable",
    },
    Entry {
        code: "material_missing",
        label_fr: "Matériel / pièces manquants",
        label_en: "Material missing",
    },
    Entry {
        code: "weather_constraint",
        label_fr: "Conditions météo",
        label_en: "Weather constraint",
    },
    Entry {
        code: "safety_constraint",
        label_fr: "Contrainte de sécurité",
        label_en: "Safety constraint",
    },
    Entry {
        code: "other",
        label_fr: "Autre (préciser dans la note)",
        label_en: "Other (see note)",
    },
];

const SEVERITIES: &[Entry] = &[
    Entry {
        code: "low",
        label_fr: "Faible",
        label_en: "Low",
    },
    Entry {
        code: "medium",
        label_fr: "Moyenne",
        label_en: "Medium",
    },
    Entry {
        code: "high",
        label_fr: "Élevée",
        label_en: "High",
    },
    Entry {
        code: "critical",
        label_fr: "Critique",
        label_en: "Critical",
    },
];

fn seeded_value_sync_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

/// Both lists with their shipped values. Idempotent (integrity pass and migration 226).
pub async fn ensure_pm_reason_catalogs(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        REASONS_DOMAIN,
        "Motifs de report / non-réalisation PM",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_domain(
        db,
        SEVERITY_DOMAIN,
        "Gravité des constats PM",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    for (domain, entries) in [(REASONS_DOMAIN, REASONS), (SEVERITY_DOMAIN, SEVERITIES)] {
        ensure_published_set(db, domain).await?;
        for (i, e) in entries.iter().enumerate() {
            let meta = serde_json::json!({ "origin": "system", "label_en": e.label_en });
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, \
                    description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, \
                    metadata_json) \
                 SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, NULL, NULL, 1, ? \
                   FROM reference_domains d \
                   JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
                  WHERE UPPER(TRIM(d.code)) = ? \
                    AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
                  ORDER BY rs.version_no DESC LIMIT 1",
                [
                    seeded_value_sync_id(domain, e.code).into(),
                    e.code.into(),
                    e.label_fr.into(),
                    (i64::try_from(i).unwrap_or(0) + 1).into(),
                    meta.to_string().into(),
                    domain.into(),
                    seeded_value_sync_id(domain, e.code).into(),
                ],
            ))
            .await?;
        }
    }
    Ok(())
}

/// One choice of a PM dialog (active value of the latest published set).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PmOption {
    pub code: String,
    pub label: String,
    pub label_en: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PmExecutionOptions {
    pub reasons: Vec<PmOption>,
    pub severities: Vec<PmOption>,
}

async fn active_options(db: &impl ConnectionTrait, domain: &str) -> AppResult<Vec<PmOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.metadata_json FROM reference_values rv \
              WHERE rv.is_active = 1 AND rv.set_id = ( \
                    SELECT rs.id FROM reference_sets rs \
                      JOIN reference_domains d ON d.id = rs.domain_id \
                     WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' \
                     ORDER BY rs.version_no DESC LIMIT 1) \
              ORDER BY rv.sort_order, rv.label",
            [domain.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let meta: Option<String> = r.try_get("", "metadata_json")?;
            let label_en = meta
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("label_en").and_then(|l| l.as_str()).map(str::to_string));
            Ok(PmOption {
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                label_en,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

pub async fn list_pm_execution_options(db: &impl ConnectionTrait) -> AppResult<PmExecutionOptions> {
    Ok(PmExecutionOptions {
        reasons: active_options(db, REASONS_DOMAIN).await?,
        severities: active_options(db, SEVERITY_DOMAIN).await?,
    })
}

async fn checked_code(db: &impl ConnectionTrait, domain: &str, field: &str, code: &str) -> AppResult<String> {
    let wanted = code.trim();
    active_options(db, domain)
        .await?
        .into_iter()
        .find(|o| o.code.eq_ignore_ascii_case(wanted))
        .map(|o| o.code)
        .ok_or_else(|| {
            crate::errors::org_validation_failed(
                "PM_CODE_NOT_IN_LIST",
                format!("{field} '{wanted}' is not an active value of {domain}."),
                std::collections::HashMap::from([
                    ("field".to_string(), field.to_string()),
                    ("code".to_string(), wanted.to_string()),
                ]),
            )
        })
}

/// A deferral / miss / cancel reason: an active value of `PM.DEFERRAL_REASONS` (its list code).
pub async fn checked_reason(db: &impl ConnectionTrait, field: &str, code: &str) -> AppResult<String> {
    checked_code(db, REASONS_DOMAIN, field, code).await
}

/// A finding severity: an active value of `PM.FINDING_SEVERITY` (its list code).
pub async fn checked_severity(db: &impl ConnectionTrait, code: &str) -> AppResult<String> {
    checked_code(db, SEVERITY_DOMAIN, "severity", code).await
}
