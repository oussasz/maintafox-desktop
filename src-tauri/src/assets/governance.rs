//! Asset import governance — validation policy engine and conflict classifier.
//!
//! Phase 2 - Sub-phase 02 - File 04 - Sprint S1.
//!
//! This module defines the conflict taxonomy and row-level validation pipeline
//! for staged import rows. It reuses the same lookup-resolution and org-node
//! queries established in `identity.rs` (migration 005/010) rather than
//! reimplementing them. The governance engine runs against the staging table
//! and produces per-row validation outcomes without modifying the asset registry.
//!
//! Conflict categories match the architecture rules in File 04:
//!   - DuplicateAssetCode — code already exists in equipment or within the batch
//!   - UnknownLookupCode — class, family, criticality, or status code not found
//!   - OrgNodeMissing — org_node_id does not reference an active org node
//!   - HierarchyCycleRisk — proposed parent_asset_id would create a cycle
//!   - ForbiddenStatusTransition — status change violates lifecycle rules
//!   - ReclassificationRequiresReview — class/family/criticality changed on
//!     an existing asset; requires explicit policy acknowledgement

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ─── Conflict taxonomy ────────────────────────────────────────────────────────

/// Discrete conflict categories from the File 04 architecture rules.
/// Each category produces a distinct validation message so the import UI
/// can render category-specific guidance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictCategory {
    DuplicateAssetCode,
    DuplicateAssetCodeInBatch,
    UnknownClassCode,
    UnknownFamilyCode,
    FamilyClassMismatch,
    UnknownCriticalityCode,
    UnknownStatusCode,
    OrgNodeMissing,
    OrgNodeInactive,
    HierarchyCycleRisk,
    ForbiddenStatusTransition,
    ReclassificationRequiresReview,
    MissingRequiredField,
    InvalidAssetCodeFormat,
}

/// Single validation issue found for an import row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationMessage {
    pub category: ConflictCategory,
    pub field: String,
    pub message: String,
    /// `error` blocks apply; `warning` flags for review.
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

/// Aggregate validation outcome for a single import row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationOutcome {
    /// Overall row status: `valid`, `warning`, or `error`.
    pub status: String,
    pub messages: Vec<ValidationMessage>,
    /// Proposed action if row is valid: `create`, `update`, `skip`, or `conflict`.
    pub proposed_action: Option<String>,
}

/// Normalized import row extracted from raw CSV/JSON input.
/// Field names align with the `CreateAssetPayload` contract from `identity.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedImportRow {
    pub asset_code: Option<String>,
    pub external_key: Option<String>,
    pub asset_name: Option<String>,
    pub class_code: Option<String>,
    pub family_code: Option<String>,
    pub criticality_code: Option<String>,
    pub status_code: Option<String>,
    pub org_node_id: Option<i64>,
    pub parent_asset_code: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub maintainable_boundary: Option<bool>,
    pub commissioned_at: Option<String>,
}

// ─── Validation pipeline ──────────────────────────────────────────────────────

/// Validate a single normalized import row against the live registry.
///
/// This function collects all issues (does not short-circuit on first error)
/// and returns an aggregate outcome. The caller (import.rs) persists the
/// outcome into `asset_import_staging`.
///
/// # Arguments
/// - `db` — connection (can be within a transaction)
/// - `row` — normalized row data
/// - `batch_id` — current batch id (for intra-batch duplicate detection)
/// - `row_no` — 1-based row number in the import file
pub async fn validate_import_row(
    db: &impl ConnectionTrait,
    row: &NormalizedImportRow,
    batch_id: i64,
    row_no: i64,
) -> AppResult<ValidationOutcome> {
    let mut messages: Vec<ValidationMessage> = Vec::new();

    // ── 1. Required field checks ──────────────────────────────────────────
    let asset_code = match &row.asset_code {
        Some(c) if !c.trim().is_empty() => {
            let trimmed = c.trim().to_uppercase();
            // Same format rules as identity.rs::validate_asset_code
            if trimmed.len() > 64 {
                messages.push(ValidationMessage {
                    category: ConflictCategory::InvalidAssetCodeFormat,
                    field: "asset_code".into(),
                    message: "Le code equipement ne peut pas depasser 64 caracteres.".into(),
                    severity: ValidationSeverity::Error,
                });
            } else if !trimmed
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_')
            {
                messages.push(ValidationMessage {
                    category: ConflictCategory::InvalidAssetCodeFormat,
                    field: "asset_code".into(),
                    message: "Le code equipement ne peut contenir que des majuscules, chiffres, tirets et tirets bas.".into(),
                    severity: ValidationSeverity::Error,
                });
            }
            Some(trimmed)
        }
        _ => {
            // Asset code is required unless external_key is provided for upsert
            if row.external_key.is_none() || row.external_key.as_deref().unwrap_or("").trim().is_empty() {
                messages.push(ValidationMessage {
                    category: ConflictCategory::MissingRequiredField,
                    field: "asset_code".into(),
                    message: "Le code equipement ou une cle externe est requis.".into(),
                    severity: ValidationSeverity::Error,
                });
            }
            None
        }
    };

    if row.asset_name.as_deref().unwrap_or("").trim().is_empty() {
        messages.push(ValidationMessage {
            category: ConflictCategory::MissingRequiredField,
            field: "asset_name".into(),
            message: "Le nom de l'equipement est requis.".into(),
            severity: ValidationSeverity::Error,
        });
    }

    // ── 2. Lookup domain resolution ───────────────────────────────────────

    // Class code
    let existing_class_parent_id = if let Some(ref class_code) = row.class_code {
        match resolve_class_exists(db, class_code).await? {
            Some((_, parent_id)) => Some(parent_id),
            None => {
                messages.push(ValidationMessage {
                    category: ConflictCategory::UnknownClassCode,
                    field: "class_code".into(),
                    message: format!(
                        "Classe d'equipement '{}' introuvable ou inactive.",
                        class_code
                    ),
                    severity: ValidationSeverity::Error,
                });
                None
            }
        }
    } else {
        messages.push(ValidationMessage {
            category: ConflictCategory::MissingRequiredField,
            field: "class_code".into(),
            message: "Le code classe est requis.".into(),
            severity: ValidationSeverity::Error,
        });
        None
    };

    // Family code — validate parent linkage if both class and family are provided
    if let (Some(ref family_code), Some(ref class_code)) = (&row.family_code, &row.class_code) {
        if existing_class_parent_id.is_some() {
            if let Err(_) = validate_family_exists(db, family_code, class_code).await {
                messages.push(ValidationMessage {
                    category: ConflictCategory::FamilyClassMismatch,
                    field: "family_code".into(),
                    message: format!(
                        "Le code famille '{}' ne correspond pas a la classe '{}'.",
                        family_code, class_code
                    ),
                    severity: ValidationSeverity::Error,
                });
            }
        }
    }

    // Criticality code
    if let Some(ref crit_code) = row.criticality_code {
        if !lookup_code_exists(db, "equipment.criticality", crit_code).await? {
            messages.push(ValidationMessage {
                category: ConflictCategory::UnknownCriticalityCode,
                field: "criticality_code".into(),
                message: format!(
                    "Code criticite '{}' introuvable dans le domaine 'equipment.criticality'.",
                    crit_code
                ),
                severity: ValidationSeverity::Error,
            });
        }
    } else {
        messages.push(ValidationMessage {
            category: ConflictCategory::MissingRequiredField,
            field: "criticality_code".into(),
            message: "Le code criticite est requis.".into(),
            severity: ValidationSeverity::Error,
        });
    }

    // Status code
    if let Some(ref status_code) = row.status_code {
        if !lookup_code_exists(db, "equipment.lifecycle_status", status_code).await? {
            messages.push(ValidationMessage {
                category: ConflictCategory::UnknownStatusCode,
                field: "status_code".into(),
                message: format!(
                    "Code statut '{}' introuvable dans le domaine 'equipment.lifecycle_status'.",
                    status_code
                ),
                severity: ValidationSeverity::Error,
            });
        }
    } else {
        messages.push(ValidationMessage {
            category: ConflictCategory::MissingRequiredField,
            field: "status_code".into(),
            message: "Le code statut est requis.".into(),
            severity: ValidationSeverity::Error,
        });
    }

    // ── 3. Org node validation ────────────────────────────────────────────
    if let Some(org_id) = row.org_node_id {
        match check_org_node_active(db, org_id).await? {
            OrgNodeCheck::Active => {}
            OrgNodeCheck::NotFound => {
                messages.push(ValidationMessage {
                    category: ConflictCategory::OrgNodeMissing,
                    field: "org_node_id".into(),
                    message: format!("Noeud organisationnel {} introuvable.", org_id),
                    severity: ValidationSeverity::Error,
                });
            }
            OrgNodeCheck::Inactive(status) => {
                messages.push(ValidationMessage {
                    category: ConflictCategory::OrgNodeInactive,
                    field: "org_node_id".into(),
                    message: format!(
                        "Le noeud organisationnel {} n'est pas actif (statut: {}).",
                        org_id, status
                    ),
                    severity: ValidationSeverity::Error,
                });
            }
        }
    }

    // ── 4. Duplicate detection — registry + intra-batch ───────────────────
    let mut existing_asset: Option<ExistingAssetInfo> = None;

    if let Some(ref code) = asset_code {
        // Check if asset code already exists in the registry
        if let Some(info) = find_existing_asset_by_code(db, code).await? {
            existing_asset = Some(info);
        }

        // Intra-batch duplicate: same asset_code in another staging row
        if check_intra_batch_duplicate(db, batch_id, code, row_no).await? {
            messages.push(ValidationMessage {
                category: ConflictCategory::DuplicateAssetCodeInBatch,
                field: "asset_code".into(),
                message: format!(
                    "Le code '{}' apparait plusieurs fois dans ce lot d'import.",
                    code
                ),
                severity: ValidationSeverity::Error,
            });
        }
    }

    // External key resolution fallback
    if existing_asset.is_none() {
        if let Some(ref ext_key) = row.external_key {
            let trimmed = ext_key.trim();
            if !trimmed.is_empty() {
                existing_asset = find_existing_asset_by_external_key(db, trimmed).await?;
            }
        }
    }

    // ── 5. Reclassification and status transition checks ──────────────────
    if let Some(ref info) = existing_asset {
        // Reclassification drift detection
        if let Some(ref new_class) = row.class_code {
            if let Some(ref current_class) = info.class_code {
                if new_class != current_class {
                    messages.push(ValidationMessage {
                        category: ConflictCategory::ReclassificationRequiresReview,
                        field: "class_code".into(),
                        message: format!(
                            "Changement de classe de '{}' vers '{}' detecte. Necessite une revision explicite.",
                            current_class, new_class
                        ),
                        severity: ValidationSeverity::Warning,
                    });
                }
            }
        }

        if let Some(ref new_crit) = row.criticality_code {
            if let Some(ref current_crit) = info.criticality_code {
                if new_crit != current_crit {
                    messages.push(ValidationMessage {
                        category: ConflictCategory::ReclassificationRequiresReview,
                        field: "criticality_code".into(),
                        message: format!(
                            "Changement de criticite de '{}' vers '{}' detecte. Necessite une revision explicite.",
                            current_crit, new_crit
                        ),
                        severity: ValidationSeverity::Warning,
                    });
                }
            }
        }

        // Forbidden status transitions (decommission via import is blocked)
        if let Some(ref new_status) = row.status_code {
            if new_status == "DECOMMISSIONED" && info.status_code != "DECOMMISSIONED" {
                messages.push(ValidationMessage {
                    category: ConflictCategory::ForbiddenStatusTransition,
                    field: "status_code".into(),
                    message: "La mise hors service via import est interdite. Utilisez le processus de cycle de vie.".into(),
                    severity: ValidationSeverity::Error,
                });
            }
        }
    }

    // ── 6. Hierarchy cycle risk ───────────────────────────────────────────
    if let (Some(ref parent_code), Some(ref code)) = (&row.parent_asset_code, &asset_code) {
        if let Some(ref info) = existing_asset {
            if let Some(parent_info) = find_existing_asset_by_code(db, parent_code).await? {
                if would_create_cycle(db, parent_info.id, info.id).await? {
                    messages.push(ValidationMessage {
                        category: ConflictCategory::HierarchyCycleRisk,
                        field: "parent_asset_code".into(),
                        message: format!(
                            "Le parent '{}' creerait un cycle dans la hierarchie de '{}'.",
                            parent_code, code
                        ),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
        }
    }

    // ── 7. Compute aggregate outcome ──────────────────────────────────────
    let has_errors = messages
        .iter()
        .any(|m| m.severity == ValidationSeverity::Error);
    let has_warnings = messages
        .iter()
        .any(|m| m.severity == ValidationSeverity::Warning);

    let status = if has_errors {
        "error"
    } else if has_warnings {
        "warning"
    } else {
        "valid"
    };

    let proposed_action = if has_errors {
        Some("conflict".to_string())
    } else if existing_asset.is_some() {
        Some("update".to_string())
    } else {
        Some("create".to_string())
    };

    Ok(ValidationOutcome {
        status: status.to_string(),
        messages,
        proposed_action,
    })
}

// ─── Internal lookup helpers ──────────────────────────────────────────────────
//
// These mirror the same queries in identity.rs but return boolean/option
// results instead of raising AppError, since governance collects all issues.

/// Existing asset info for reclassification and status transition checks.
#[derive(Debug, Clone)]
struct ExistingAssetInfo {
    id: i64,
    class_code: Option<String>,
    criticality_code: Option<String>,
    status_code: String,
}

/// Check if an equipment class code exists and is active.
/// Returns `(class_id, parent_id)` if found.
async fn resolve_class_exists(
    db: &impl ConnectionTrait,
    class_code: &str,
) -> AppResult<Option<(i64, Option<i64>)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_id FROM equipment_classes \
             WHERE code = ? AND is_active = 1 AND deleted_at IS NULL",
            [class_code.into()],
        ))
        .await?;
    match row {
        Some(r) => {
            let id: i64 = r.try_get("", "id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("class.id decode: {e}"))
            })?;
            let parent_id: Option<i64> = r.try_get("", "parent_id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("class.parent_id decode: {e}"))
            })?;
            Ok(Some((id, parent_id)))
        }
        None => Ok(None),
    }
}

/// Validate that a family code is the parent of the given class code.
async fn validate_family_exists(
    db: &impl ConnectionTrait,
    family_code: &str,
    class_code: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ec.id FROM equipment_classes ec \
             INNER JOIN equipment_classes ef ON ef.id = ec.parent_id \
             WHERE ec.code = ? AND ef.code = ? \
               AND ec.is_active = 1 AND ec.deleted_at IS NULL \
               AND ef.is_active = 1 AND ef.deleted_at IS NULL",
            [class_code.into(), family_code.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Family '{}' is not the parent of class '{}'.",
            family_code, class_code
        )]));
    }
    Ok(())
}

/// Check if a lookup code exists in the given domain.
async fn lookup_code_exists(
    db: &impl ConnectionTrait,
    domain_key: &str,
    code: &str,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [domain_key.into(), code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    Ok(cnt > 0)
}

enum OrgNodeCheck {
    Active,
    NotFound,
    Inactive(String),
}

/// Check org node existence and active status (mirrors identity.rs::assert_org_node_active).
async fn check_org_node_active(
    db: &impl ConnectionTrait,
    org_node_id: i64,
) -> AppResult<OrgNodeCheck> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [org_node_id.into()],
        ))
        .await?;
    match row {
        None => Ok(OrgNodeCheck::NotFound),
        Some(r) => {
            let status: String = r.try_get("", "status").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("org_node.status decode: {e}"))
            })?;
            if status == "active" {
                Ok(OrgNodeCheck::Active)
            } else {
                Ok(OrgNodeCheck::Inactive(status))
            }
        }
    }
}

/// Find existing asset by asset code (non-deleted).
async fn find_existing_asset_by_code(
    db: &impl ConnectionTrait,
    asset_code: &str,
) -> AppResult<Option<ExistingAssetInfo>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, ec.code AS class_code, lv.code AS criticality_code, \
                    e.lifecycle_status AS status_code \
             FROM equipment e \
             LEFT JOIN equipment_classes ec ON ec.id = e.class_id \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             WHERE e.asset_id_code = ? AND e.deleted_at IS NULL",
            [asset_code.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(ExistingAssetInfo {
            id: r.try_get("", "id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("equipment.id decode: {e}"))
            })?,
            class_code: r.try_get("", "class_code").unwrap_or(None),
            criticality_code: r.try_get("", "criticality_code").unwrap_or(None),
            status_code: r.try_get("", "status_code").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("equipment.status_code decode: {e}"))
            })?,
        })),
        None => Ok(None),
    }
}

/// Find existing asset by external key in `asset_external_ids`.
async fn find_existing_asset_by_external_key(
    db: &impl ConnectionTrait,
    external_key: &str,
) -> AppResult<Option<ExistingAssetInfo>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, ec.code AS class_code, lv.code AS criticality_code, \
                    e.lifecycle_status AS status_code \
             FROM asset_external_ids aei \
             INNER JOIN equipment e ON e.id = aei.asset_id \
             LEFT JOIN equipment_classes ec ON ec.id = e.class_id \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             WHERE aei.external_id = ? AND e.deleted_at IS NULL \
               AND (aei.valid_to IS NULL OR aei.valid_to > datetime('now'))",
            [external_key.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(ExistingAssetInfo {
            id: r.try_get("", "id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("equipment.id decode: {e}"))
            })?,
            class_code: r.try_get("", "class_code").unwrap_or(None),
            criticality_code: r.try_get("", "criticality_code").unwrap_or(None),
            status_code: r.try_get("", "status_code").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("equipment.status_code decode: {e}"))
            })?,
        })),
        None => Ok(None),
    }
}

/// Check if the same normalized_asset_code already exists in staging rows
/// for this batch (excluding the current row_no).
async fn check_intra_batch_duplicate(
    db: &impl ConnectionTrait,
    batch_id: i64,
    asset_code: &str,
    current_row_no: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM asset_import_staging \
             WHERE batch_id = ? AND normalized_asset_code = ? AND row_no != ?",
            [batch_id.into(), asset_code.into(), current_row_no.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    Ok(cnt > 0)
}

/// Hierarchy cycle detection — mirrors hierarchy.rs::detect_cycle logic
/// via BFS walk. Returns true if creating a parent→child link would form a cycle.
async fn would_create_cycle(
    db: &impl ConnectionTrait,
    proposed_parent_id: i64,
    proposed_child_id: i64,
) -> AppResult<bool> {
    if proposed_parent_id == proposed_child_id {
        return Ok(true);
    }

    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![proposed_parent_id];

    while let Some(current) = queue.pop() {
        if current == proposed_child_id {
            return Ok(true);
        }
        if !visited.insert(current) {
            continue;
        }
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT parent_equipment_id FROM equipment_hierarchy \
                 WHERE child_equipment_id = ? AND effective_to IS NULL",
                [current.into()],
            ))
            .await?;
        for r in &rows {
            let pid: i64 = r.try_get("", "parent_equipment_id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("parent_equipment_id decode: {e}"))
            })?;
            queue.push(pid);
        }
    }

    Ok(false)
}
