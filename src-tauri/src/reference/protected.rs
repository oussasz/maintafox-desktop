//! Protected-domain policy layer.
//!
//! Phase 2 - Sub-phase 03 - File 02 - Sprint S1.
//!
//! Enforces PRD 6.13 protected analytical domain semantics:
//!   - Protected domains (`governance_level = 'protected_analytical'`) block
//!     hard deletion of in-use values; deactivation or migration is required.
//!   - Non-protected domains allow deletion when the value is not in use.
//!   - Usage probes inspect downstream tables for active references.
//!
//! The probe set is designed to be extended incrementally as downstream modules
//! (work orders, failure coding, etc.) are delivered in later sub-phases.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::domains;
use super::sets;
use super::values;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns `true` if the domain's governance level is `protected_analytical`.
pub async fn is_protected_domain(
    db: &DatabaseConnection,
    domain_id: i64,
) -> AppResult<bool> {
    let domain = domains::get_reference_domain(db, domain_id).await?;
    Ok(domain.governance_level == "protected_analytical")
}

/// Asserts that a reference value can be deactivated.
///
/// For protected analytical domains, deactivation of in-use values is allowed
/// (this is the governed alternative to deletion). For non-protected domains,
/// deactivation is always allowed (handled in `values::deactivate_value`).
///
/// This guard exists so that future policy extensions (e.g. requiring a
/// migration map before deactivation) can be enforced centrally.
pub async fn assert_can_deactivate_value(
    db: &DatabaseConnection,
    value_id: i64,
) -> AppResult<()> {
    let value = values::get_value(db, value_id).await?;
    let set = sets::get_reference_set(db, value.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;

    if domain.governance_level != "protected_analytical" {
        // Non-protected domains: deactivation always permitted at service layer.
        return Ok(());
    }

    // Protected analytical: deactivation is allowed (it is the governed path).
    // Future policy: check if a migration map exists before deactivation.
    // For now, log the intent and allow.
    Ok(())
}

/// Asserts that a reference value can be hard-deleted.
///
/// Policy rules:
///   - **Protected analytical domain + value in use** → blocked. Must deactivate
///     or create a migration map instead.
///   - **Protected analytical domain + value not in use** → allowed.
///   - **Non-protected domain + value in use** → blocked.
///   - **Non-protected domain + value not in use** → allowed.
pub async fn assert_can_delete_value(
    db: &DatabaseConnection,
    value_id: i64,
) -> AppResult<()> {
    let value = values::get_value(db, value_id).await?;
    let set = sets::get_reference_set(db, value.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;

    let is_protected = domain.governance_level == "protected_analytical";
    let usages = collect_usage_references(db, &domain, &value).await?;

    if is_protected && !usages.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de supprimer la valeur '{}' (code '{}') du domaine protege '{}'. \
             Elle est utilisee par : {}. \
             Utilisez la desactivation ou la migration a la place.",
            value.label,
            value.code,
            domain.code,
            format_usage_summary(&usages),
        )]));
    }

    if !usages.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de supprimer la valeur '{}' (code '{}') car elle est utilisee par : {}.",
            value.label,
            value.code,
            format_usage_summary(&usages),
        )]));
    }

    Ok(())
}

// ─── Usage probe types ────────────────────────────────────────────────────────

/// A single usage reference found by a probe.
#[derive(Debug, Clone)]
pub struct UsageReference {
    /// Human-readable label for the consuming table/entity (e.g. "equipment_classes").
    pub source_table: String,
    /// Number of rows referencing this value.
    pub ref_count: i64,
}

// ─── Usage collection ─────────────────────────────────────────────────────────

/// Runs all registered usage probes and returns a combined list of references.
///
/// Probes are intentionally individual functions so that new downstream modules
/// can add probes without modifying existing ones.
async fn collect_usage_references(
    db: &DatabaseConnection,
    domain: &domains::ReferenceDomain,
    value: &values::ReferenceValue,
) -> AppResult<Vec<UsageReference>> {
    let mut usages: Vec<UsageReference> = Vec::new();

    // ── Probe 1: equipment_classes (class / family / subfamily) ───────────
    // The equipment_classes table stores its own classification hierarchy.
    // If the governed reference domain maps to equipment classification,
    // check if the value code matches any active equipment class code.
    if let Some(u) = probe_equipment_classes(db, domain, value).await? {
        usages.push(u);
    }

    // ── Probe 2: equipment criticality via lookup_values bridge ───────────
    // The equipment table references lookup_values.id for criticality.
    // A governed reference_value is considered "in use" if a lookup_value
    // with the same code in the corresponding lookup_domain is actively
    // referenced by at least one equipment row.
    if let Some(u) = probe_equipment_criticality(db, domain, value).await? {
        usages.push(u);
    }

    // ── Probe 3: lookup_values direct code match ─────────────────────────
    // If a lookup_value in the consumer layer shares the same code under a
    // domain with matching domain_key, that constitutes downstream usage
    // regardless of which entity consumes it.
    if let Some(u) = probe_lookup_values(db, domain, value).await? {
        usages.push(u);
    }

    // ── Future probes (added incrementally as modules are delivered) ──────
    // - work order type/urgency references
    // - failure code domain references
    // - PM strategy type references
    // - inspection round classification references

    Ok(usages)
}

// ─── Individual probes ────────────────────────────────────────────────────────

/// Checks if a reference value code is used as an equipment class/family code.
///
/// Applies to domains whose code starts with "EQUIPMENT" and relates to
/// classification semantics (class, family, subfamily).
async fn probe_equipment_classes(
    db: &DatabaseConnection,
    domain: &domains::ReferenceDomain,
    value: &values::ReferenceValue,
) -> AppResult<Option<UsageReference>> {
    // Only probe if the domain is related to equipment classification.
    let domain_upper = domain.code.to_ascii_uppercase();
    if !domain_upper.starts_with("EQUIPMENT")
        || !(domain_upper.contains("CLASS")
            || domain_upper.contains("FAMILY")
            || domain_upper.contains("CLASSIFICATION"))
    {
        return Ok(None);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM equipment_classes \
             WHERE code = ? AND (deleted_at IS NULL) AND is_active = 1",
            [value.code.clone().into()],
        ))
        .await?;

    let count = match row {
        Some(r) => r.try_get::<i64>("", "cnt").unwrap_or(0),
        None => 0,
    };

    if count > 0 {
        Ok(Some(UsageReference {
            source_table: "equipment_classes".into(),
            ref_count: count,
        }))
    } else {
        Ok(None)
    }
}

/// Checks if a reference value code corresponds to a lookup_value used by
/// equipment rows via `criticality_value_id`.
///
/// Applies to domains whose code relates to criticality semantics.
async fn probe_equipment_criticality(
    db: &DatabaseConnection,
    domain: &domains::ReferenceDomain,
    value: &values::ReferenceValue,
) -> AppResult<Option<UsageReference>> {
    let domain_upper = domain.code.to_ascii_uppercase();
    if !domain_upper.contains("CRITICALITY") {
        return Ok(None);
    }

    // Find the corresponding lookup_value by matching code in the consumer layer.
    // Then count equipment rows that reference it.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM equipment e \
             INNER JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             WHERE lv.code = ? AND lv.is_active = 1 AND e.deleted_at IS NULL",
            [value.code.clone().into()],
        ))
        .await?;

    let count = match row {
        Some(r) => r.try_get::<i64>("", "cnt").unwrap_or(0),
        None => 0,
    };

    if count > 0 {
        Ok(Some(UsageReference {
            source_table: "equipment (criticality)".into(),
            ref_count: count,
        }))
    } else {
        Ok(None)
    }
}

/// Checks if a reference value code exists in the lookup_values consumer layer
/// under a matching domain_key.
///
/// This catch-all probe detects any downstream consumption through the flat
/// lookup path, regardless of which specific entity uses the lookup_value.
async fn probe_lookup_values(
    db: &DatabaseConnection,
    domain: &domains::ReferenceDomain,
    value: &values::ReferenceValue,
) -> AppResult<Option<UsageReference>> {
    // Map the governed domain code to a lookup_domains.domain_key pattern.
    // Convention: governed domain "EQUIPMENT.CLASS" maps to lookup domain_key
    // containing "equipment.class" (case-insensitive match via LIKE).
    let domain_key_pattern = domain.code.to_ascii_lowercase().replace('_', ".");

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE LOWER(ld.domain_key) = ? \
               AND lv.code = ? \
               AND lv.is_active = 1 \
               AND lv.deleted_at IS NULL \
               AND ld.deleted_at IS NULL",
            [domain_key_pattern.into(), value.code.clone().into()],
        ))
        .await?;

    let count = match row {
        Some(r) => r.try_get::<i64>("", "cnt").unwrap_or(0),
        None => 0,
    };

    if count > 0 {
        Ok(Some(UsageReference {
            source_table: "lookup_values".into(),
            ref_count: count,
        }))
    } else {
        Ok(None)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Formats a human-readable summary of usage references for error messages.
fn format_usage_summary(usages: &[UsageReference]) -> String {
    usages
        .iter()
        .map(|u| format!("{} ({} ref.)", u.source_table, u.ref_count))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Checks whether a migration map already exists for a value (from → any target).
///
/// This is used by future policy rules that require a migration map before
/// deactivation is allowed in protected domains.
pub async fn has_migration_map(
    db: &DatabaseConnection,
    from_value_id: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM reference_value_migrations \
             WHERE from_value_id = ?",
            [from_value_id.into()],
        ))
        .await?;

    let count = match row {
        Some(r) => r.try_get::<i64>("", "cnt").unwrap_or(0),
        None => 0,
    };

    Ok(count > 0)
}
