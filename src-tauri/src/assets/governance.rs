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

use crate::assets::identity;
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
    ParentAssetMissing,
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
    /// Legacy numeric organisation node id (column `org_node_id`).
    pub org_node_id: Option<i64>,
    /// Organisation node code as shown in Organisation (column `emplacement`).
    pub org_node_code: Option<String>,
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
                    message: "Le code equipement ne peut contenir que des majuscules, chiffres, tirets et tirets bas."
                        .into(),
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
                    message: format!("Classe d'equipement '{}' introuvable ou inactive.", class_code),
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

    // Family code: a value of the published EQUIPMENT.FAMILY list that belongs to the class
    // (same rule as the equipment form).
    if let Some(ref family_code) = row.family_code {
        match (&row.class_code, existing_class_parent_id.is_some()) {
            (Some(class_code), true) => {
                let outcome = match identity::resolve_class_ref_id(db, class_code).await {
                    Ok(class_ref_id) => identity::validate_family_for_class_ref(db, family_code, class_ref_id)
                        .await
                        .map(|_| ()),
                    Err(e) => Err(e),
                };
                if let Err(e) = outcome {
                    let text = e.to_string();
                    let (category, message) = if text.contains("GATE_ASSET_FAMILY_UNKNOWN") {
                        (
                            ConflictCategory::UnknownFamilyCode,
                            format!("Famille '{family_code}' introuvable dans les Données de référence (Familles d'équipement)."),
                        )
                    } else {
                        (
                            ConflictCategory::FamilyClassMismatch,
                            format!("La famille '{family_code}' n'appartient pas à la classe '{class_code}' dans les Données de référence."),
                        )
                    };
                    messages.push(ValidationMessage {
                        category,
                        field: "family_code".into(),
                        message,
                        severity: ValidationSeverity::Error,
                    });
                }
            }
            _ => {}
        }
    }

    // Criticality code (published EQUIPMENT.CRITICALITY reference set; accepts A–D + legacy labels)
    if let Some(ref crit_code) = row.criticality_code {
        if identity::resolve_criticality_ref_id(db, crit_code).await.is_err() {
            messages.push(ValidationMessage {
                category: ConflictCategory::UnknownCriticalityCode,
                field: "criticality_code".into(),
                message: format!(
                    "Code criticite '{}' introuvable (reference EQUIPMENT.CRITICALITY).",
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

    // Status code (published EQUIPMENT.STATUS reference set)
    if let Some(ref status_code) = row.status_code {
        let ok = match identity::normalize_status_code_for_reference(status_code) {
            Ok(norm) => identity::validate_status_code(db, &norm).await.is_ok(),
            Err(_) => false,
        };
        if !ok {
            messages.push(ValidationMessage {
                category: ConflictCategory::UnknownStatusCode,
                field: "status_code".into(),
                message: format!(
                    "Code statut '{}' introuvable (reference EQUIPMENT.STATUS).",
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
    // The location is given by its code (column `emplacement`); the numeric id
    // column is still read for files exported before.
    let mut org_target = row.org_node_id;
    if let Some(ref org_code) = row.org_node_code {
        match lookup_org_node_by_code(db, org_code).await? {
            OrgCodeLookup::Found(id) => {
                if row.org_node_id.is_some_and(|explicit| explicit != id) {
                    messages.push(ValidationMessage {
                        category: ConflictCategory::OrgNodeMissing,
                        field: "emplacement".into(),
                        message: format!(
                            "L'emplacement '{org_code}' et le numéro de nœud du fichier désignent deux nœuds différents. \
                             Gardez uniquement la colonne « emplacement »."
                        ),
                        severity: ValidationSeverity::Error,
                    });
                    org_target = None;
                } else {
                    org_target = Some(id);
                }
            }
            OrgCodeLookup::NotFound => {
                messages.push(ValidationMessage {
                    category: ConflictCategory::OrgNodeMissing,
                    field: "emplacement".into(),
                    message: format!(
                        "Emplacement '{org_code}' introuvable dans l'organisation active. \
                         Utilisez le code d'un nœud tel qu'il apparaît dans Organisation."
                    ),
                    severity: ValidationSeverity::Error,
                });
                org_target = None;
            }
            OrgCodeLookup::Ambiguous => {
                messages.push(ValidationMessage {
                    category: ConflictCategory::OrgNodeMissing,
                    field: "emplacement".into(),
                    message: format!("Le code d'emplacement '{org_code}' désigne plusieurs nœuds de l'organisation."),
                    severity: ValidationSeverity::Error,
                });
                org_target = None;
            }
        }
    }
    if let Some(org_id) = org_target {
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
                message: format!("Le code '{}' apparait plusieurs fois dans ce lot d'import.", code),
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
                    message: "La mise hors service via import est interdite. Utilisez le processus de cycle de vie."
                        .into(),
                    severity: ValidationSeverity::Error,
                });
            }
        }
    }

    // ── 6. Hierarchy cycle risk ───────────────────────────────────────────
    // The parent is given by its equipment code. It must already exist or be created by
    // another line of the same file; the link itself is made once all lines are applied.
    if let (Some(parent_raw), Some(code)) = (&row.parent_asset_code, &asset_code) {
        let parent_code = parent_raw.trim().to_uppercase();
        if &parent_code == code {
            messages.push(ValidationMessage {
                category: ConflictCategory::HierarchyCycleRisk,
                field: "parent_asset_code".into(),
                message: format!("L'équipement '{code}' ne peut pas être son propre parent."),
                severity: ValidationSeverity::Error,
            });
        } else {
            let parent_in_registry = find_existing_asset_by_code(db, &parent_code).await?;
            let parent_in_file = batch_row_exists(db, batch_id, &parent_code).await?;
            if parent_in_registry.is_none() && !parent_in_file {
                messages.push(ValidationMessage {
                    category: ConflictCategory::ParentAssetMissing,
                    field: "parent_asset_code".into(),
                    message: format!(
                        "Parent '{parent_code}' introuvable : il doit exister dans le registre ou figurer dans ce fichier."
                    ),
                    severity: ValidationSeverity::Error,
                });
            } else if parent_in_registry
                .as_ref()
                .is_some_and(|p| p.status_code == "DECOMMISSIONED" || p.status_code == "SCRAPPED")
            {
                messages.push(ValidationMessage {
                    category: ConflictCategory::ParentAssetMissing,
                    field: "parent_asset_code".into(),
                    message: format!(
                        "Le parent '{parent_code}' est déclassé : il ne peut plus recevoir de sous-ensemble."
                    ),
                    severity: ValidationSeverity::Error,
                });
            } else if parent_chain_reaches(db, batch_id, &parent_code, code).await? {
                messages.push(ValidationMessage {
                    category: ConflictCategory::HierarchyCycleRisk,
                    field: "parent_asset_code".into(),
                    message: format!("Le parent '{parent_code}' créerait un cycle dans la hiérarchie de '{code}'."),
                    severity: ValidationSeverity::Error,
                });
            } else if let Some(current) = registry_parent_code(db, code).await? {
                if current != parent_code {
                    messages.push(ValidationMessage {
                        category: ConflictCategory::ReclassificationRequiresReview,
                        field: "parent_asset_code".into(),
                        message: format!("Le parent actuel '{current}' sera remplacé par '{parent_code}'."),
                        severity: ValidationSeverity::Warning,
                    });
                }
            }
        }
    }

    // ── 7. Compute aggregate outcome ──────────────────────────────────────
    let has_errors = messages.iter().any(|m| m.severity == ValidationSeverity::Error);
    let has_warnings = messages.iter().any(|m| m.severity == ValidationSeverity::Warning);

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
async fn resolve_class_exists(db: &impl ConnectionTrait, class_code: &str) -> AppResult<Option<(i64, Option<i64>)>> {
    let code = class_code.trim();
    if code.is_empty() {
        return Ok(None);
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_id FROM equipment_classes \
             WHERE UPPER(code) = UPPER(?) AND is_active = 1 AND deleted_at IS NULL",
            [code.into()],
        ))
        .await?;
    match row {
        Some(r) => {
            let id: i64 = r
                .try_get("", "id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("class.id decode: {e}")))?;
            let parent_id: Option<i64> = r
                .try_get("", "parent_id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("class.parent_id decode: {e}")))?;
            Ok(Some((id, parent_id)))
        }
        None => Ok(None),
    }
}

/// Outcome of looking an organisation node up by its code.
pub(crate) enum OrgCodeLookup {
    Found(i64),
    NotFound,
    Ambiguous,
}

/// Resolve an organisation node code (as shown in Organisation) in the active structure.
pub(crate) async fn lookup_org_node_by_code(db: &impl ConnectionTrait, code: &str) -> AppResult<OrgCodeLookup> {
    let Some(model_id) = crate::org::model_scope::try_get_active_model_id(db).await? else {
        return Ok(OrgCodeLookup::NotFound);
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes \
             WHERE structure_model_id = ? AND UPPER(TRIM(code)) = UPPER(TRIM(?)) AND deleted_at IS NULL",
            [model_id.into(), code.into()],
        ))
        .await?;
    match rows.as_slice() {
        [] => Ok(OrgCodeLookup::NotFound),
        [only] => {
            Ok(OrgCodeLookup::Found(only.try_get("", "id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("org_node.id decode: {e}"))
            })?))
        }
        _ => Ok(OrgCodeLookup::Ambiguous),
    }
}

/// True when another line of the batch (or this one) carries the given equipment code.
async fn batch_row_exists(db: &impl ConnectionTrait, batch_id: i64, asset_code: &str) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM asset_import_staging \
             WHERE batch_id = ? AND UPPER(TRIM(normalized_asset_code)) = ?",
            [batch_id.into(), asset_code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    Ok(row.try_get::<i64>("", "cnt").unwrap_or(0) > 0)
}

/// Active PARENT_CHILD parent of an equipment already in the registry.
async fn registry_parent_code(db: &impl ConnectionTrait, asset_code: &str) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.asset_id_code AS code FROM equipment_hierarchy h \
             INNER JOIN equipment c ON c.id = h.child_equipment_id \
             INNER JOIN equipment p ON p.id = h.parent_equipment_id \
             WHERE c.asset_id_code = ? AND c.deleted_at IS NULL \
               AND h.relationship_type = 'PARENT_CHILD' AND h.effective_to IS NULL \
             LIMIT 1",
            [asset_code.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => Some(
            r.try_get::<String>("", "code")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("parent code decode: {e}")))?,
        ),
        None => None,
    })
}

/// Parent a code will have once the batch is applied: the one written in the file when
/// the line has one, else the registry parent.
async fn parent_after_import(db: &impl ConnectionTrait, batch_id: i64, code: &str) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT json_extract(raw_json, '$.parent_asset_code') AS parent FROM asset_import_staging \
             WHERE batch_id = ? AND UPPER(TRIM(normalized_asset_code)) = ? LIMIT 1",
            [batch_id.into(), code.into()],
        ))
        .await?;
    if let Some(r) = row {
        let parent: Option<String> = r.try_get("", "parent").unwrap_or(None);
        if let Some(p) = parent.map(|p| p.trim().to_uppercase()).filter(|p| !p.is_empty()) {
            return Ok(Some(p));
        }
    }
    registry_parent_code(db, code).await
}

/// Walk up from `start`; true when `target` is met (the proposed link would close a loop).
async fn parent_chain_reaches(db: &impl ConnectionTrait, batch_id: i64, start: &str, target: &str) -> AppResult<bool> {
    let mut current = start.to_string();
    for _ in 0..200 {
        if current == target {
            return Ok(true);
        }
        match parent_after_import(db, batch_id, &current).await? {
            Some(next) => current = next,
            None => return Ok(false),
        }
    }
    // A chain this deep is a loop that does not include the target: refuse it too.
    Ok(true)
}

enum OrgNodeCheck {
    Active,
    NotFound,
    Inactive(String),
}

/// Check org node existence and active status (mirrors identity.rs::assert_org_node_active).
async fn check_org_node_active(db: &impl ConnectionTrait, org_node_id: i64) -> AppResult<OrgNodeCheck> {
    if let Err(e) = crate::org::model_scope::assert_node_is_active_tree(db, org_node_id).await {
        return Ok(match e {
            AppError::NotFound { .. } | AppError::ValidationFailed(_) => OrgNodeCheck::NotFound,
            other => return Err(other),
        });
    }
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
            let status: String = r
                .try_get("", "status")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("org_node.status decode: {e}")))?;
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
            id: r
                .try_get("", "id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment.id decode: {e}")))?,
            class_code: r.try_get("", "class_code").unwrap_or(None),
            criticality_code: r.try_get("", "criticality_code").unwrap_or(None),
            status_code: r
                .try_get("", "status_code")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment.status_code decode: {e}")))?,
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
            id: r
                .try_get("", "id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment.id decode: {e}")))?,
            class_code: r.try_get("", "class_code").unwrap_or(None),
            criticality_code: r.try_get("", "criticality_code").unwrap_or(None),
            status_code: r
                .try_get("", "status_code")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment.status_code decode: {e}")))?,
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
