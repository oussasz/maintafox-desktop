//! The personnel lists as governed reference data (migration 228), seeded with the codes the
//! program used to hard-code, so every stored value keeps its meaning.
//!
//! - `PERSONNEL.EMPLOYMENT_STATUSES` — **SYSTEM** (fixed, hidden like `WORK.URGENCY`): `active`
//!   drives availability, capacity, qualification and planning; `terminated` locks the record.
//! - `PERSONNEL.EMPLOYMENT_TYPES` — **COMPANY**: descriptive, each plant adds its own.
//! - `PERSONNEL.ABSENCE_TYPES` — **COMPANY**, each value carries a REQUIRED behaviour tag read by
//!   the program ([`ABSENCE_TAGS`], fixed in code): `leave` (plain absence), `critical` (always a
//!   critical block), `training` (the person shows as in training). A value without a valid tag
//!   is refused (`reference::values::checked_semantic_tag`).
//! - `PERSONNEL.POSITION_CATEGORIES` — **COMPANY**: descriptive.
//!
//! Every write checks the code against the active values of its list (case-insensitive, stored
//! as the list code); the screens read the lists through `list_personnel_reference_options`.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const EMPLOYMENT_STATUSES_DOMAIN: &str = "PERSONNEL.EMPLOYMENT_STATUSES";
pub const EMPLOYMENT_TYPES_DOMAIN: &str = "PERSONNEL.EMPLOYMENT_TYPES";
pub const ABSENCE_TYPES_DOMAIN: &str = "PERSONNEL.ABSENCE_TYPES";
pub const POSITION_CATEGORIES_DOMAIN: &str = "PERSONNEL.POSITION_CATEGORIES";

/// Behaviour tags of an absence type (fixed in code; listed in the editor's help).
pub const ABSENCE_TAG_LEAVE: &str = "leave";
pub const ABSENCE_TAG_CRITICAL: &str = "critical";
pub const ABSENCE_TAG_TRAINING: &str = "training";
pub const ABSENCE_TAGS: &[&str] = &[ABSENCE_TAG_LEAVE, ABSENCE_TAG_CRITICAL, ABSENCE_TAG_TRAINING];

/// Stable codes of the refusals (translated in `training:errors`).
pub const CODE_NOT_IN_LIST: &str = "PERSONNEL_CODE_NOT_IN_LIST";
pub const CODE_ABSENCE_TAG_REQUIRED: &str = "PERSONNEL_ABSENCE_TAG_REQUIRED";

struct Entry {
    code: &'static str,
    label_fr: &'static str,
    label_en: &'static str,
    tag: Option<&'static str>,
}

const fn e(code: &'static str, label_fr: &'static str, label_en: &'static str) -> Entry {
    Entry {
        code,
        label_fr,
        label_en,
        tag: None,
    }
}

const fn tagged(code: &'static str, label_fr: &'static str, label_en: &'static str, tag: &'static str) -> Entry {
    Entry {
        code,
        label_fr,
        label_en,
        tag: Some(tag),
    }
}

const EMPLOYMENT_STATUSES: &[Entry] = &[
    e("active", "Actif", "Active"),
    e("inactive", "Inactif", "Inactive"),
    e("suspended", "Suspendu", "Suspended"),
    e("terminated", "Sorti", "Terminated"),
];

const EMPLOYMENT_TYPES: &[Entry] = &[
    e("employee", "Salarié", "Employee"),
    e("contractor", "Sous-traitant", "Contractor"),
    e("temp", "Intérimaire", "Temporary"),
    e("vendor", "Fournisseur", "Vendor"),
];

const ABSENCE_TYPES: &[Entry] = &[
    tagged("leave", "Congé", "Leave", ABSENCE_TAG_LEAVE),
    tagged("medical", "Arrêt médical", "Medical", ABSENCE_TAG_CRITICAL),
    tagged(
        "restriction",
        "Restriction d'aptitude",
        "Restriction",
        ABSENCE_TAG_CRITICAL,
    ),
    tagged("training", "Formation", "Training", ABSENCE_TAG_TRAINING),
];

const POSITION_CATEGORIES: &[Entry] = &[
    e("technician", "Technicien", "Technician"),
    e("supervisor", "Superviseur", "Supervisor"),
    e("engineer", "Ingénieur", "Engineer"),
    e("operator", "Opérateur", "Operator"),
    e("contractor", "Sous-traitant", "Contractor"),
    e("planner", "Planificateur", "Planner"),
    e("storekeeper", "Magasinier", "Storekeeper"),
    e("hse", "HSE", "HSE"),
];

fn seeded_value_sync_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

/// The four lists with their shipped values. Idempotent (integrity pass and migration 228).
pub async fn ensure_personnel_reference_lists(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        EMPLOYMENT_STATUSES_DOMAIN,
        "Statuts d'emploi",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    for (code, name) in [
        (EMPLOYMENT_TYPES_DOMAIN, "Types d'emploi"),
        (ABSENCE_TYPES_DOMAIN, "Types d'absence"),
        (POSITION_CATEGORIES_DOMAIN, "Catégories de poste"),
    ] {
        ensure_domain(db, code, name, "flat", "tenant_managed", 1, DomainAudience::Company).await?;
    }
    for (domain, entries) in [
        (EMPLOYMENT_STATUSES_DOMAIN, EMPLOYMENT_STATUSES),
        (EMPLOYMENT_TYPES_DOMAIN, EMPLOYMENT_TYPES),
        (ABSENCE_TYPES_DOMAIN, ABSENCE_TYPES),
        (POSITION_CATEGORIES_DOMAIN, POSITION_CATEGORIES),
    ] {
        ensure_published_set(db, domain).await?;
        for (i, entry) in entries.iter().enumerate() {
            let meta = serde_json::json!({ "origin": "system", "label_en": entry.label_en });
            let sync_id = seeded_value_sync_id(domain, entry.code);
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, \
                    description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, \
                    metadata_json) \
                 SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, ?, NULL, 1, ? \
                   FROM reference_domains d \
                   JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
                  WHERE UPPER(TRIM(d.code)) = ? \
                    AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
                  ORDER BY rs.version_no DESC LIMIT 1",
                [
                    sync_id.clone().into(),
                    entry.code.into(),
                    entry.label_fr.into(),
                    (i64::try_from(i).unwrap_or(0) + 1).into(),
                    entry.tag.map(str::to_string).into(),
                    meta.to_string().into(),
                    domain.into(),
                    sync_id.into(),
                ],
            ))
            .await?;
        }
    }
    Ok(())
}

/// One choice of a personnel list (active value of the latest published set).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PersonnelOption {
    pub code: String,
    pub label: String,
    pub label_en: Option<String>,
    /// Behaviour tag (absence types only: `leave` / `critical` / `training`).
    pub tag: Option<String>,
    /// A value found in stored data that was not in the list (migration 229, integrity pass):
    /// kept with its exact code and flagged "à revoir" for the plant.
    #[serde(default)]
    pub review_required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersonnelReferenceOptions {
    pub employment_statuses: Vec<PersonnelOption>,
    pub employment_types: Vec<PersonnelOption>,
    pub absence_types: Vec<PersonnelOption>,
    pub position_categories: Vec<PersonnelOption>,
}

async fn active_options(db: &impl ConnectionTrait, domain: &str) -> AppResult<Vec<PersonnelOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label, rv.semantic_tag, rv.metadata_json FROM reference_values rv \
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
            let review_required = meta
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("review_required").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
            Ok(PersonnelOption {
                code: r.try_get("", "code")?,
                label: r.try_get("", "label")?,
                label_en,
                tag: r.try_get("", "semantic_tag")?,
                review_required,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

pub async fn list_personnel_reference_options(db: &impl ConnectionTrait) -> AppResult<PersonnelReferenceOptions> {
    Ok(PersonnelReferenceOptions {
        employment_statuses: active_options(db, EMPLOYMENT_STATUSES_DOMAIN).await?,
        employment_types: active_options(db, EMPLOYMENT_TYPES_DOMAIN).await?,
        absence_types: active_options(db, ABSENCE_TYPES_DOMAIN).await?,
        position_categories: active_options(db, POSITION_CATEGORIES_DOMAIN).await?,
    })
}

async fn checked_option(
    db: &impl ConnectionTrait,
    domain: &str,
    field: &str,
    code: &str,
) -> AppResult<PersonnelOption> {
    let wanted = code.trim();
    active_options(db, domain)
        .await?
        .into_iter()
        .find(|o| o.code.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            crate::errors::org_validation_failed(
                CODE_NOT_IN_LIST,
                format!("{field} '{wanted}' is not an active value of {domain}."),
                std::collections::HashMap::from([
                    ("field".to_string(), field.to_string()),
                    ("code".to_string(), wanted.to_string()),
                ]),
            )
        })
}

/// An employment status: an active value of the SYSTEM list (its list code).
pub async fn checked_employment_status(db: &impl ConnectionTrait, code: &str) -> AppResult<String> {
    Ok(
        checked_option(db, EMPLOYMENT_STATUSES_DOMAIN, "employment_status", code)
            .await?
            .code,
    )
}

/// An employment type: an active value of the plant's list (its list code).
pub async fn checked_employment_type(db: &impl ConnectionTrait, code: &str) -> AppResult<String> {
    Ok(checked_option(db, EMPLOYMENT_TYPES_DOMAIN, "employment_type", code)
        .await?
        .code)
}

/// A position category: an active value of the plant's list (its list code).
pub async fn checked_position_category(db: &impl ConnectionTrait, code: &str) -> AppResult<String> {
    Ok(checked_option(db, POSITION_CATEGORIES_DOMAIN, "category", code)
        .await?
        .code)
}

/// An absence type: an active value of the plant's list, with its behaviour tag.
pub async fn checked_absence_type(db: &impl ConnectionTrait, code: &str) -> AppResult<(String, String)> {
    let option = checked_option(db, ABSENCE_TYPES_DOMAIN, "block_type", code).await?;
    let tag = option
        .tag
        .filter(|t| ABSENCE_TAGS.contains(&t.as_str()))
        .ok_or_else(|| absence_tag_required(&option.code))?;
    Ok((option.code, tag))
}

fn absence_tag_required(code: &str) -> AppError {
    crate::errors::org_validation_failed(
        CODE_ABSENCE_TAG_REQUIRED,
        if code.is_empty() {
            format!("An absence type needs a behaviour tag: {}.", ABSENCE_TAGS.join(" / "))
        } else {
            format!(
                "Absence type '{code}' needs a behaviour tag: {}.",
                ABSENCE_TAGS.join(" / ")
            )
        },
        std::collections::HashMap::from([
            ("code".to_string(), code.to_string()),
            ("tags".to_string(), ABSENCE_TAGS.join(", ")),
        ]),
    )
}

/// Whether a reference domain is the absence-type list (its values need a behaviour tag).
pub fn is_absence_types_domain(domain_code: &str) -> bool {
    domain_code.trim().eq_ignore_ascii_case(ABSENCE_TYPES_DOMAIN)
}

/// The tag of a value written to `PERSONNEL.ABSENCE_TYPES` through the reference editor: required,
/// one of [`ABSENCE_TAGS`]; no default.
pub fn checked_absence_tag(code: &str, tag: Option<&str>) -> AppResult<String> {
    tag.map(str::trim)
        .filter(|t| ABSENCE_TAGS.contains(t))
        .map(str::to_string)
        .ok_or_else(|| absence_tag_required(code))
}

/// SQL fragment (alias `b` for `personnel_availability_blocks`): the behaviour tag of the block's
/// absence type, matched case-insensitively like the checkers (an exact-case value first); NULL
/// only for a type that is in no list at all (migration 229 and the integrity pass adopt every
/// stored type, so this does not happen for data written before or received by sync).
pub const BLOCK_TAG_SQL: &str = "COALESCE(\
    (SELECT rv.semantic_tag FROM reference_values rv \
      JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
      JOIN reference_domains d ON d.id = rs.domain_id \
     WHERE UPPER(TRIM(d.code)) = 'PERSONNEL.ABSENCE_TYPES' AND rv.code = b.block_type \
     ORDER BY rs.version_no DESC LIMIT 1), \
    (SELECT rv.semantic_tag FROM reference_values rv \
      JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
      JOIN reference_domains d ON d.id = rs.domain_id \
     WHERE UPPER(TRIM(d.code)) = 'PERSONNEL.ABSENCE_TYPES' AND UPPER(rv.code) = UPPER(b.block_type) \
     ORDER BY rs.version_no DESC LIMIT 1))";

/// The behaviour the program gave a stored absence type before the list existed (migration 228):
/// `leave` / `vacation` / `sick` showed the person on leave, `training` in training, anything
/// else blocked (= `critical` here, the only other non-leave behaviour).
pub fn legacy_absence_tag(code: &str) -> &'static str {
    match code {
        "leave" | "vacation" | "sick" => ABSENCE_TAG_LEAVE,
        "training" => ABSENCE_TAG_TRAINING,
        _ => ABSENCE_TAG_CRITICAL,
    }
}

/// Adds every distinct value stored in `table.column` that is not in `domain` (case-insensitive)
/// to the domain's published set, with its exact code, flagged `review_required` and
/// `origin = "migrated"` — no stored value is rewritten. `tag_for` gives the behaviour tag (absence
/// types). Idempotent: seeded sync identity per (domain, code), the same on every device.
pub(crate) async fn adopt_stored_values(
    db: &impl ConnectionTrait,
    domain: &str,
    table: &str,
    column: &str,
    tag_for: Option<fn(&str) -> &'static str>,
) -> AppResult<Vec<String>> {
    let present = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some();
    if !present {
        return Ok(Vec::new());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT t.\"{column}\" AS code FROM \"{table}\" t \
                  WHERE t.\"{column}\" IS NOT NULL AND TRIM(t.\"{column}\") <> '' \
                    AND NOT EXISTS (SELECT 1 FROM reference_values rv \
                          JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                          JOIN reference_domains d ON d.id = rs.domain_id \
                         WHERE UPPER(TRIM(d.code)) = ? AND UPPER(rv.code) = UPPER(t.\"{column}\")) \
                  ORDER BY 1"
            ),
            [domain.into()],
        ))
        .await?;
    let mut adopted = Vec::new();
    for r in &rows {
        let code: String = r.try_get("", "code")?;
        let sync_id = seeded_value_sync_id(domain, &code);
        let meta = serde_json::json!({ "origin": "migrated", "review_required": true });
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, \
                description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, \
                metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, NULL, 900, NULL, NULL, ?, NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                sync_id.clone().into(),
                code.clone().into(),
                code.clone().into(),
                tag_for.map(|f| f(&code).to_string()).into(),
                meta.to_string().into(),
                domain.into(),
                sync_id.into(),
            ],
        ))
        .await?;
        adopted.push(code);
    }
    Ok(adopted)
}

/// Adopts the stored employment types, position categories and absence types that are not in
/// their lists (migration 229 and the integrity pass, so values received by sync are caught).
pub async fn adopt_stored_personnel_values(db: &impl ConnectionTrait) -> AppResult<()> {
    adopt_stored_values(db, EMPLOYMENT_TYPES_DOMAIN, "personnel", "employment_type", None).await?;
    adopt_stored_values(db, POSITION_CATEGORIES_DOMAIN, "positions", "category", None).await?;
    adopt_stored_values(
        db,
        ABSENCE_TYPES_DOMAIN,
        "personnel_availability_blocks",
        "block_type",
        Some(legacy_absence_tag),
    )
    .await?;
    Ok(())
}

/// A person whose stored employment status is not in the SYSTEM list (older data or sync). The
/// readers treat it as before: not active (left out of availability, capacity, qualification and
/// planning), neither terminated nor inactive. Listed for review; never rewritten.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PersonnelStatusReview {
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub stored_status: String,
}

/// People whose stored employment status is not a value of `PERSONNEL.EMPLOYMENT_STATUSES`.
pub async fn list_personnel_status_review(db: &impl ConnectionTrait) -> AppResult<Vec<PersonnelStatusReview>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.id, p.employee_code, p.full_name, p.employment_status FROM personnel p \
              WHERE p.employment_status IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv \
                      JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                      JOIN reference_domains d ON d.id = rs.domain_id \
                     WHERE UPPER(TRIM(d.code)) = ? AND rv.code = p.employment_status) \
              ORDER BY p.employee_code",
            [EMPLOYMENT_STATUSES_DOMAIN.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PersonnelStatusReview {
                personnel_id: r.try_get("", "id")?,
                employee_code: r.try_get("", "employee_code")?,
                full_name: r.try_get("", "full_name")?,
                stored_status: r.try_get("", "employment_status")?,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

/// The behaviour tag of an absence type that is already stored on a block (the edit keeps it):
/// the list value whatever its state (a retired type keeps its behaviour), `None` when absent.
pub async fn stored_absence_tag(db: &impl ConnectionTrait, code: &str) -> AppResult<Option<String>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.semantic_tag AS tag FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND UPPER(rv.code) = UPPER(?) \
              ORDER BY rs.version_no DESC, (rv.code = ?) DESC LIMIT 1",
            [ABSENCE_TYPES_DOMAIN.into(), code.into(), code.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "tag"))
        .transpose()?
        .flatten())
}
