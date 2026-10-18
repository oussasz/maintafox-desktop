//! Who hands a closed permit back to whom: the roles of a permit hand-over (`PERMIT.HANDOVER_ROLES`,
//! migration 235).
//!
//! A COMPANY list of Données de référence, shipped **empty**: each plant names its own roles
//! (maintenance executant, operations, HSE…). A hand-over's "from" and "to" roles must be active
//! values of the list. Free texts stored before the list existed (or received from a PC that has
//! not run 235) are adopted with their exact text, flagged "à revoir" (`review_required`), and no
//! stored hand-over is rewritten — the same pattern as migration 229.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const DOMAIN: &str = "PERMIT.HANDOVER_ROLES";

/// Stable code: a hand-over role that is not an active value of the list.
pub const CODE_ROLE_NOT_IN_LIST: &str = "PERMIT_HANDOVER_ROLE_NOT_IN_LIST";

/// The list with its published set (empty). Idempotent (migration 235, integrity pass).
pub async fn ensure_handover_roles(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        DOMAIN,
        "Rôles de passation de permis",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, DOMAIN).await
}

/// Adopts the stored "from" / "to" texts that are not in the list (exact text kept, flagged
/// "à revoir"). Migration 235 and the integrity pass (texts received by sync).
pub async fn adopt_stored_roles(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_handover_roles(db).await?;
    for column in ["handed_from_role", "handed_to_role"] {
        crate::personnel::reference_lists::adopt_stored_values(db, DOMAIN, "permit_handover_logs", column, None)
            .await?;
    }
    Ok(())
}

/// One hand-over role, as the hand-over form lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoverRoleOption {
    pub code: String,
    pub label: String,
    /// Adopted from a stored free text: to review in Données de référence.
    pub review_required: bool,
}

/// Active roles of the latest published set, in the list order.
pub async fn list_handover_roles(db: &impl ConnectionTrait) -> AppResult<Vec<HandoverRoleOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.metadata_json FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1 \
                AND rs.id = (SELECT rs2.id FROM reference_sets rs2 \
                              WHERE rs2.domain_id = d.id AND rs2.status = 'published' \
                              ORDER BY rs2.version_no DESC LIMIT 1) \
              ORDER BY rv.sort_order ASC, rv.code ASC",
            [DOMAIN.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let meta: Option<String> = r.try_get("", "metadata_json")?;
            let review_required = meta
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("review_required").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
            Ok(HandoverRoleOption {
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                review_required,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

/// A hand-over role: an active value of the list (exact code first, then ignoring case). Returns
/// the list code to store.
pub async fn checked_role(db: &impl ConnectionTrait, field: &str, code: &str) -> AppResult<String> {
    let wanted = code.trim();
    let options = list_handover_roles(db).await?;
    let exact = options.iter().position(|o| o.code == wanted);
    let index = exact.or_else(|| options.iter().position(|o| o.code.eq_ignore_ascii_case(wanted)));
    index.map(|i| options[i].code.clone()).ok_or_else(|| {
        crate::errors::org_validation_failed(
            CODE_ROLE_NOT_IN_LIST,
            format!("{field} '{wanted}' is not an active hand-over role of Reference data."),
            std::collections::HashMap::from([
                ("field".to_string(), field.to_string()),
                ("code".to_string(), wanted.to_string()),
            ]),
        )
    })
}
