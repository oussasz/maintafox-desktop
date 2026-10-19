//! Who hands a closed permit back to whom: the roles of a permit hand-over (`PERMIT.HANDOVER_ROLES`,
//! migration 235).
//!
//! A COMPANY list of Données de référence: each plant names its own roles. It starts with three
//! roles flagged "à revoir" ([`STARTER_ROLES`], migration 239) that the administrator reviews
//! (owner decision); flagged roles stay usable, shown "(à revoir)" by the hand-over form
//! ([`HandoverRoleOption::review_required`]). A hand-over's "from" and "to" roles must be active
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

/// The starter roles (owner decision, migration 239): code (also the French label) and English label.
/// Seeded flagged "à revoir": the administrator reviews, renames, deactivates or completes them.
pub const STARTER_ROLES: &[(&str, &str)] = &[
    ("Maintenance", "Maintenance"),
    ("Exploitation", "Operations"),
    ("HSE", "HSE"),
];

/// Seeds [`STARTER_ROLES`] (migration 239 only — never the integrity pass, so a role removed later
/// is not brought back). Only into a list that holds no value an administrator added (values
/// adopted from old free text, `origin = "migrated"`, do not count); a starter whose code is already
/// in the list (ignoring case and spaces, e.g. adopted from free text) is skipped; each has a seeded
/// sync identity, the same on every PC, and is never written over an existing value.
pub async fn seed_starter_roles(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_handover_roles(db).await?;
    let added_by_plant = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND COALESCE(json_extract(CASE WHEN json_valid(rv.metadata_json) \
                                               THEN rv.metadata_json END, '$.origin'), '') <> 'migrated'",
            [DOMAIN.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0);
    if added_by_plant > 0 {
        return Ok(());
    }
    for (i, (code, label_en)) in STARTER_ROLES.iter().enumerate() {
        let sync_id = crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{DOMAIN}:{code}"));
        let meta = serde_json::json!({ "origin": "starter", "review_required": true, "label_en": label_en });
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
                AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.set_id = rs.id \
                                  AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?))) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                sync_id.clone().into(),
                (*code).into(),
                (*code).into(),
                i64::try_from(i + 1).unwrap_or(1).into(),
                meta.to_string().into(),
                DOMAIN.into(),
                sync_id.into(),
                (*code).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// Adopts the stored "from" / "to" texts that are not in the list (exact text kept, flagged
/// "à revoir"). Migration 235 and the integrity pass at start-up (the catch-all); a hand-over log
/// received by sync adopts only its own two roles ([`adopt_received_roles`]).
pub async fn adopt_stored_roles(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_handover_roles(db).await?;
    for column in ["handed_from_role", "handed_to_role"] {
        crate::personnel::reference_lists::adopt_stored_values(db, DOMAIN, "permit_handover_logs", column, None)
            .await?;
    }
    Ok(())
}

/// Adopts the two roles of one hand-over log received by sync (`sync::apply_hooks::after_upsert`),
/// as [`adopt_stored_roles`] would: only that log's "from" and "to" texts, and nothing at all when
/// both are already codes of a published set of the list. The list and its published set are not
/// created or touched here (they exist since migration 235, and the integrity pass ensures them):
/// a list without a published set adopts nothing on arrival, so a list problem never refuses the
/// received hand-overs; the next integrity pass adopts what was left.
pub async fn adopt_received_roles(db: &impl ConnectionTrait, log_id: i64) -> AppResult<()> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT handed_from_role, handed_to_role FROM permit_handover_logs WHERE id = ?",
            [log_id.into()],
        ))
        .await?
    else {
        return Ok(());
    };
    let mut roles: Vec<String> = Vec::new();
    for column in ["handed_from_role", "handed_to_role"] {
        if let Some(role) = row
            .try_get::<Option<String>>("", column)?
            .filter(|r| !r.trim().is_empty())
        {
            if !roles.contains(&role) {
                roles.push(role);
            }
        }
    }
    for role in roles {
        let listed = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM reference_values rv \
                   JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                   JOIN reference_domains d ON d.id = rs.domain_id \
                  WHERE UPPER(TRIM(d.code)) = ? AND rv.code = ? LIMIT 1",
                [DOMAIN.into(), role.clone().into()],
            ))
            .await?
            .is_some();
        if !listed {
            crate::personnel::reference_lists::adopt_value(db, DOMAIN, &role, None).await?;
        }
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
