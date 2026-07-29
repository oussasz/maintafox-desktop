//! Reference value tree service.
//!
//! Phase 2 - Sub-phase 03 - File 01 - Sprint S3.
//!
//! Provides governed CRUD for `reference_values`:
//!   - code uniqueness within a set
//!   - hierarchy cycle detection for hierarchical domains
//!   - parent-belongs-to-same-set validation
//!   - deactivation guard for protected analytical domains
//!
//! Mutation permission is decided by `crate::reference::governance`
//! (Compat phase: draft-only manager mutate; Category B operational create).

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use super::{domains, sets};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Complete reference value record for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValue {
    pub id: i64,
    pub set_id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub label: String,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
    pub color_hex: Option<String>,
    pub icon_name: Option<String>,
    pub semantic_tag: Option<String>,
    pub external_code: Option<String>,
    pub is_active: bool,
    pub metadata_json: Option<String>,
}

/// Payload for creating a reference value in a draft set.
#[derive(Debug, Deserialize)]
pub struct CreateReferenceValuePayload {
    pub set_id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub label: String,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
    pub color_hex: Option<String>,
    pub icon_name: Option<String>,
    pub semantic_tag: Option<String>,
    pub external_code: Option<String>,
    pub metadata_json: Option<String>,
}

/// Payload for operational create into the latest published set of a
/// tenant-managed extendable domain (form "create from dropdown" path).
#[derive(Debug, Deserialize)]
pub struct CreateOperationalReferenceValuePayload {
    pub domain_code: String,
    pub label: String,
    pub description: Option<String>,
    pub parent_id: Option<i64>,
    /// Optional explicit code; when omitted, generated from the label.
    pub code: Option<String>,
}

/// Payload for updating a reference value. Only provided fields are changed.
#[derive(Debug, Deserialize)]
pub struct UpdateReferenceValuePayload {
    pub label: Option<String>,
    pub description: Option<Option<String>>,
    pub sort_order: Option<Option<i64>>,
    pub color_hex: Option<Option<String>>,
    pub icon_name: Option<Option<String>>,
    pub semantic_tag: Option<Option<String>>,
    pub external_code: Option<Option<String>>,
    pub metadata_json: Option<Option<String>>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_values row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str =
    "id, set_id, parent_id, code, label, description, sort_order, \
     color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json";

fn map_value(row: &QueryResult) -> AppResult<ReferenceValue> {
    Ok(ReferenceValue {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        set_id: row
            .try_get::<i64>("", "set_id")
            .map_err(|e| decode_err("set_id", e))?,
        parent_id: row
            .try_get::<Option<i64>>("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        label: row
            .try_get::<String>("", "label")
            .map_err(|e| decode_err("label", e))?,
        description: row
            .try_get::<Option<String>>("", "description")
            .map_err(|e| decode_err("description", e))?,
        sort_order: row
            .try_get::<Option<i64>>("", "sort_order")
            .map_err(|e| decode_err("sort_order", e))?,
        color_hex: row
            .try_get::<Option<String>>("", "color_hex")
            .map_err(|e| decode_err("color_hex", e))?,
        icon_name: row
            .try_get::<Option<String>>("", "icon_name")
            .map_err(|e| decode_err("icon_name", e))?,
        semantic_tag: row
            .try_get::<Option<String>>("", "semantic_tag")
            .map_err(|e| decode_err("semantic_tag", e))?,
        external_code: row
            .try_get::<Option<String>>("", "external_code")
            .map_err(|e| decode_err("external_code", e))?,
        is_active: i64_to_bool(
            row.try_get::<i64>("", "is_active")
                .map_err(|e| decode_err("is_active", e))?,
        ),
        metadata_json: row
            .try_get::<Option<String>>("", "metadata_json")
            .map_err(|e| decode_err("metadata_json", e))?,
    })
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Fetches a value by id. Returns `NotFound` if absent.
async fn get_value_by_id(db: &DatabaseConnection, value_id: i64) -> AppResult<ReferenceValue> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {SELECT_COLS} FROM reference_values WHERE id = ?"),
            [value_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceValue".into(),
            id: value_id.to_string(),
        })?;
    map_value(&row)
}

/// Validates value code format: uppercase ASCII + digits + underscores + dots, 1–64 chars.
fn validate_code(code: &str) -> AppResult<()> {
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::ValidationFailed(vec![
            "Le code de valeur doit comporter entre 1 et 64 caractères.".into(),
        ]));
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return Err(AppError::ValidationFailed(vec![
            "Le code de valeur ne peut contenir que des lettres majuscules ASCII, \
             des chiffres, des underscores et des points."
                .into(),
        ]));
    }
    if !code.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err(AppError::ValidationFailed(vec![
            "Le code de valeur doit commencer par une lettre majuscule.".into(),
        ]));
    }
    Ok(())
}

fn validate_label(label: &str) -> AppResult<()> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err(AppError::ValidationFailed(vec![
            "Le libellé doit comporter entre 1 et 255 caractères.".into(),
        ]));
    }
    Ok(())
}

fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn strip_diacritics(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
            'À' | 'Á' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' => 'A',
            'ç' | 'ć' | 'č' => 'c',
            'Ç' | 'Ć' | 'Č' => 'C',
            'è' | 'é' | 'ê' | 'ë' | 'ē' => 'e',
            'È' | 'É' | 'Ê' | 'Ë' | 'Ē' => 'E',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' => 'I',
            'ñ' | 'ń' => 'n',
            'Ñ' | 'Ń' => 'N',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
            'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' | 'Ō' => 'O',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' => 'U',
            'ý' | 'ÿ' => 'y',
            'Ý' => 'Y',
            other => other,
        })
        .collect()
}

/// Deterministic uppercase snake code from a human label.
pub fn code_from_label(label: &str) -> AppResult<String> {
    let ascii = strip_diacritics(label.trim());
    let mut out = String::new();
    let mut prev_underscore = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
            prev_underscore = false;
        } else if !prev_underscore && !out.is_empty() {
            out.push('_');
            prev_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Impossible de générer un code à partir du libellé. Utilisez des lettres ou chiffres."
                .into(),
        ]));
    }
    if !out.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        out = format!("V_{out}");
    }
    if out.len() > 64 {
        out.truncate(64);
        while out.ends_with('_') {
            out.pop();
        }
    }
    validate_code(&out)?;
    Ok(out)
}

fn allows_cross_domain_parent(child_domain_code: &str, parent_domain_code: &str) -> bool {
    matches!(
        (child_domain_code, parent_domain_code),
        ("EQUIPMENT.FAMILY", "EQUIPMENT.CLASS")
            | ("EQUIPMENT.SUBFAMILY", "EQUIPMENT.FAMILY")
    )
}

/// Validates that a parent_id exists and is valid for the target set/domain.
async fn validate_parent(
    db: &DatabaseConnection,
    set_id: i64,
    parent_id: i64,
) -> AppResult<()> {
    let child_set = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rs.id AS set_id, d.code AS domain_code \
             FROM reference_sets rs \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE rs.id = ?",
            [set_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceSet".into(),
            id: set_id.to_string(),
        })?;
    let child_domain_code: String = child_set
        .try_get("", "domain_code")
        .map_err(|e| decode_err("child.domain_code", e))?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.set_id, d.code AS domain_code \
             FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE rv.id = ?",
            [parent_id.into()],
        ))
        .await?;

    match row {
        None => Err(AppError::ValidationFailed(vec![format!(
            "La valeur parente (id={parent_id}) n'existe pas."
        )])),
        Some(r) => {
            let parent_set_id: i64 = r
                .try_get("", "set_id")
                .map_err(|e| decode_err("set_id", e))?;
            let parent_domain_code: String = r
                .try_get("", "domain_code")
                .map_err(|e| decode_err("parent.domain_code", e))?;
            if parent_set_id != set_id
                && !allows_cross_domain_parent(&child_domain_code, &parent_domain_code)
            {
                return Err(AppError::ValidationFailed(vec![
                    "La valeur parente n'est pas autorisée pour ce domaine.".into(),
                ]));
            }
            Ok(())
        }
    }
}

/// Detects hierarchy cycles: walks up the parent chain from `start_parent_id`
/// and returns an error if `target_value_id` is encountered (would create a cycle).
async fn detect_cycle(
    db: &DatabaseConnection,
    target_value_id: i64,
    start_parent_id: i64,
) -> AppResult<()> {
    let mut current_id = Some(start_parent_id);
    let mut visited = std::collections::HashSet::new();

    while let Some(cid) = current_id {
        if cid == target_value_id {
            return Err(AppError::ValidationFailed(vec![
                "Cycle détecté dans la hiérarchie. \
                 Impossible de déplacer une valeur sous l'un de ses descendants."
                    .into(),
            ]));
        }
        if !visited.insert(cid) {
            // Already visited — corrupt hierarchy, but not our target cycle
            break;
        }
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT parent_id FROM reference_values WHERE id = ?",
                [cid.into()],
            ))
            .await?;
        current_id = match row {
            Some(r) => r
                .try_get::<Option<i64>>("", "parent_id")
                .map_err(|e| decode_err("parent_id", e))?,
            None => None,
        };
    }

    Ok(())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns all values for a set, ordered by sort_order then code.
pub async fn list_values(
    db: &DatabaseConnection,
    set_id: i64,
) -> AppResult<Vec<ReferenceValue>> {
    // Verify set exists
    sets::get_reference_set(db, set_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_values \
                 WHERE set_id = ? ORDER BY sort_order ASC, code ASC"
            ),
            [set_id.into()],
        ))
        .await?;

    rows.iter().map(map_value).collect()
}

/// Returns a single value by id.
pub async fn get_value(
    db: &DatabaseConnection,
    value_id: i64,
) -> AppResult<ReferenceValue> {
    get_value_by_id(db, value_id).await
}

/// Creates a reference value in a draft set. Returns the created value.
///
/// Validates: code format, code uniqueness in set, label, parent in same set,
/// and that the set is in draft status.
pub async fn create_value(
    db: &DatabaseConnection,
    payload: CreateReferenceValuePayload,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    // Verify set exists and mutation is allowed by governance policy.
    let set = sets::get_reference_set(db, payload.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    let code = normalize_code(&payload.code);
    validate_code(&code)?;
    validate_label(&payload.label)?;

    // Validate parent if provided
    if let Some(pid) = payload.parent_id {
        validate_parent(db, payload.set_id, pid).await?;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, \
              color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
        [
            payload.set_id.into(),
            payload.parent_id.into(),
            code.clone().into(),
            payload.label.trim().to_string().into(),
            payload.description.into(),
            payload.sort_order.into(),
            payload.color_hex.into(),
            payload.icon_name.into(),
            payload.semantic_tag.into(),
            payload.external_code.into(),
            payload.metadata_json.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![format!(
                "Le code '{code}' existe déjà dans ce jeu de référence."
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    // Fetch by set_id + code (unique index)
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_values WHERE set_id = ? AND code = ?"
            ),
            [payload.set_id.into(), code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_values row missing after insert"
            ))
        })?;

    let created = map_value(&row)?;
    crate::reference::schedule_patterns::seed_default_details_if_schedule_class(db, &created)
        .await?;
    Ok(created)
}

/// Creates a reference value in the latest **published** set of an
/// Operational Dictionary (Category B) domain. Used by form
/// "create from dropdown" for immediate usability.
///
/// Permission is decided solely by `governance` policy (not raw
/// `governance_level` / `is_extendable` checks).
///
/// Does not invent domains or sets. Fails loudly when governance does not allow
/// operational extension or when no published set exists.
pub async fn create_operational_value(
    db: &DatabaseConnection,
    payload: CreateOperationalReferenceValuePayload,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    let domain_code = normalize_code(&payload.domain_code);
    if domain_code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code de domaine est obligatoire.".into(),
        ]));
    }

    let domain_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = ?",
            [domain_code.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceDomain".into(),
            id: domain_code.clone(),
        })?;

    let domain_id: i64 = domain_row
        .try_get("", "id")
        .map_err(|e| decode_err("domain.id", e))?;
    let domain = domains::get_reference_domain(db, domain_id).await?;
    crate::reference::governance::assert_allows_operational_create(&domain)?;

    let published = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'published' \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "Aucun jeu publié pour le domaine '{domain_code}'. \
                 Publiez un jeu dans Données de référence avant d'ajouter des valeurs."
            )])
        })?;
    let set_id: i64 = published
        .try_get("", "id")
        .map_err(|e| decode_err("set.id", e))?;

    validate_label(&payload.label)?;
    let code = if let Some(ref raw) = payload.code {
        let c = normalize_code(raw);
        validate_code(&c)?;
        c
    } else {
        code_from_label(&payload.label)?
    };

    if let Some(pid) = payload.parent_id {
        validate_parent(db, set_id, pid).await?;
    }

    let sort_order_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(sort_order), 0) + 1 AS next_sort \
             FROM reference_values WHERE set_id = ?",
            [set_id.into()],
        ))
        .await?;
    let next_sort: i64 = sort_order_row
        .map(|r| r.try_get::<i64>("", "next_sort").unwrap_or(1))
        .unwrap_or(1);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, \
              color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, 1, NULL)",
        [
            set_id.into(),
            payload.parent_id.into(),
            code.clone().into(),
            payload.label.trim().to_string().into(),
            payload.description.into(),
            next_sort.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![format!(
                "Le code '{code}' existe déjà dans le jeu publié de '{domain_code}'."
            )])
        } else {
            AppError::Database(e)
        }
    })?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {SELECT_COLS} FROM reference_values WHERE set_id = ? AND code = ?"
            ),
            [set_id.into(), code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_values row missing after operational insert"
            ))
        })?;

    let created = map_value(&row)?;
    crate::reference::schedule_patterns::seed_default_details_if_schedule_class(db, &created)
        .await?;
    Ok(created)
}

/// Updates a reference value. Only provided (Some) fields are changed.
/// Code is immutable after creation. The set must be in draft status.
pub async fn update_value(
    db: &DatabaseConnection,
    value_id: i64,
    payload: UpdateReferenceValuePayload,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    let existing = get_value_by_id(db, value_id).await?;
    let set = sets::get_reference_set(db, existing.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    let mut sets_clause: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref label) = payload.label {
        validate_label(label)?;
        sets_clause.push("label = ?".into());
        values.push(label.trim().to_string().into());
    }
    if let Some(ref desc) = payload.description {
        sets_clause.push("description = ?".into());
        values.push(desc.clone().into());
    }
    if let Some(ref so) = payload.sort_order {
        sets_clause.push("sort_order = ?".into());
        values.push((*so).into());
    }
    if let Some(ref ch) = payload.color_hex {
        sets_clause.push("color_hex = ?".into());
        values.push(ch.clone().into());
    }
    if let Some(ref icon) = payload.icon_name {
        sets_clause.push("icon_name = ?".into());
        values.push(icon.clone().into());
    }
    if let Some(ref st) = payload.semantic_tag {
        sets_clause.push("semantic_tag = ?".into());
        values.push(st.clone().into());
    }
    if let Some(ref ec) = payload.external_code {
        sets_clause.push("external_code = ?".into());
        values.push(ec.clone().into());
    }
    if let Some(ref mj) = payload.metadata_json {
        sets_clause.push("metadata_json = ?".into());
        values.push(mj.clone().into());
    }

    if sets_clause.is_empty() {
        return Ok(existing);
    }

    values.push(value_id.into());

    let sql = format!(
        "UPDATE reference_values SET {} WHERE id = ?",
        sets_clause.join(", ")
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        values,
    ))
    .await?;

    get_value_by_id(db, value_id).await
}

/// Deactivates a reference value (soft-disable).
///
/// Mutation permission is decided by `governance` policy.
/// Protected analytical domains block hard deletion; deactivation is the
/// governed alternative. Full usage-check enforcement is in File 04.
pub async fn deactivate_value(
    db: &DatabaseConnection,
    value_id: i64,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    let existing = get_value_by_id(db, value_id).await?;
    let set = sets::get_reference_set(db, existing.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    if !existing.is_active {
        return Err(AppError::ValidationFailed(vec![
            "Cette valeur est déjà désactivée.".into(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET is_active = 0 WHERE id = ?",
        [value_id.into()],
    ))
    .await?;

    get_value_by_id(db, value_id).await
}

/// Reactivates a previously deactivated reference value.
///
/// Same mutation gate as `deactivate_value`.
pub async fn reactivate_value(
    db: &DatabaseConnection,
    value_id: i64,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    let existing = get_value_by_id(db, value_id).await?;
    let set = sets::get_reference_set(db, existing.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    if existing.is_active {
        return Err(AppError::ValidationFailed(vec![
            "Cette valeur est déjà active.".into(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET is_active = 1 WHERE id = ?",
        [value_id.into()],
    ))
    .await?;

    get_value_by_id(db, value_id).await
}

/// Moves a value to a new parent within the same set.
/// Validates: parent in same set, no hierarchy cycle.
/// Mutation permission is decided by `governance` policy.
pub async fn move_value_parent(
    db: &DatabaseConnection,
    value_id: i64,
    new_parent_id: Option<i64>,
    _actor_id: i64,
) -> AppResult<ReferenceValue> {
    let existing = get_value_by_id(db, value_id).await?;
    let set = sets::get_reference_set(db, existing.set_id).await?;
    let domain = domains::get_reference_domain(db, set.domain_id).await?;
    crate::reference::governance::assert_allows_value_mutation(&domain, &set)?;

    if let Some(pid) = new_parent_id {
        // Cannot parent to self
        if pid == value_id {
            return Err(AppError::ValidationFailed(vec![
                "Une valeur ne peut pas être son propre parent.".into(),
            ]));
        }
        validate_parent(db, existing.set_id, pid).await?;
        detect_cycle(db, value_id, pid).await?;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET parent_id = ? WHERE id = ?",
        [new_parent_id.into(), value_id.into()],
    ))
    .await?;

    get_value_by_id(db, value_id).await
}
