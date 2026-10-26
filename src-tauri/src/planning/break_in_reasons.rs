//! Why work breaks into a frozen schedule: the COMPANY list `PLANNING.BREAK_IN_REASONS` (each
//! plant words and extends its own reasons; Maximo "schedule break-in" reasons, SAP PM emergency
//! orders). A reason tagged [`TAG_URGENT`] (the starter `emergency` and `safety`) needs the
//! signature of a second planner (`planning::break_in_approval`); the others do not.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};
use crate::reference::audience::DomainAudience;

pub const BREAK_IN_REASONS_DOMAIN: &str = "PLANNING.BREAK_IN_REASONS";
/// Semantic tag of the reasons that need approver evidence (emergency, safety).
pub const TAG_URGENT: &str = "planning_break_in_urgent";
/// Semantic tag of the other reasons.
pub const TAG_BREAK_IN: &str = "planning_break_in";

/// Starter values: (code, French label, English label, semantic tag).
const STARTER_REASONS: &[(&str, &str, &str, &str)] = &[
    ("emergency", "Urgence", "Emergency", TAG_URGENT),
    ("safety", "Sécurité", "Safety", TAG_URGENT),
    (
        "production_loss",
        "Perte de production",
        "Production loss",
        TAG_BREAK_IN,
    ),
    ("regulatory", "Obligation réglementaire", "Regulatory", TAG_BREAK_IN),
    ("other", "Autre", "Other", TAG_BREAK_IN),
];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("break-in reason decode '{field}': {err}"))
}

/// Ensures the COMPANY list and its starter values (migration and the startup integrity pass).
/// Values the plant changed or added are never rewritten.
pub async fn ensure_break_in_reason_catalog(db: &impl ConnectionTrait) -> AppResult<()> {
    crate::reference::system_catalog_integrity::ensure_domain(
        db,
        BREAK_IN_REASONS_DOMAIN,
        "Motifs d'insertion dans le planning figé",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    crate::reference::system_catalog_integrity::ensure_published_set(db, BREAK_IN_REASONS_DOMAIN).await?;
    for (i, (code, label, label_en, tag)) in STARTER_REASONS.iter().enumerate() {
        let sync_id = crate::sync::identity::seeded_entity_sync_id(
            "reference_values",
            &format!("{BREAK_IN_REASONS_DOMAIN}:{code}"),
        );
        let meta = serde_json::json!({ "origin": "system", "label_en": label_en }).to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values \
             (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, ?, NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                                 WHERE s2.domain_id = d.id AND LOWER(TRIM(rv.code)) = LOWER(TRIM(?))) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                sync_id.into(),
                (*code).into(),
                (*label).into(),
                ((i as i64) + 1).into(),
                (*tag).into(),
                meta.into(),
                BREAK_IN_REASONS_DOMAIN.into(),
                (*code).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// A break-in reason of the published list: its canonical code and whether it is urgent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakInReason {
    pub code: String,
    pub urgent: bool,
}

/// The active reason with this code (case and spaces ignored), or a coded refusal.
pub async fn resolve_break_in_reason(db: &impl ConnectionTrait, code: &str) -> AppResult<BreakInReason> {
    match find_break_in_reason(db, code).await? {
        Some(reason) => Ok(reason),
        None => Err(org_validation_failed(
            "PLANNING_BREAK_IN_REASON_UNKNOWN",
            format!("'{code}' is not an active break-in reason (Données de référence, {BREAK_IN_REASONS_DOMAIN})."),
            HashMap::from([("code".to_string(), code.to_string())]),
        )),
    }
}

/// The active reason with this code (case and spaces ignored); `None` when the list here has none.
pub async fn find_break_in_reason(db: &impl ConnectionTrait, code: &str) -> AppResult<Option<BreakInReason>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, COALESCE(rv.semantic_tag, '') AS tag FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1 \
                AND LOWER(TRIM(rv.code)) = LOWER(TRIM(?)) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [BREAK_IN_REASONS_DOMAIN.into(), code.into()],
        ))
        .await?;
    row.map(|r| {
        Ok(BreakInReason {
            code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
            urgent: r.try_get::<String>("", "tag").map_err(|e| decode_err("tag", e))? == TAG_URGENT,
        })
    })
    .transpose()
}
