//! Reference governance policy — single decision engine (A/B/C).
//!
//! Product philosophy: `docs/engineering/REFERENCE_GOVERNANCE.md`
//!
//! Every backend permission for create / edit / deactivate / move / draft-set /
//! publish / operational-create must go through this module. Call sites must not
//! re-implement checks against raw `governance_level`, `is_extendable`, or
//! ad-hoc domain-code branches.
//!
//! # Enforcement phases
//!
//! - [`EnforcementPhase::Compat`]: Category A fully read-only; B/C draft-only
//!   mutate; B draft/publish still allowed (bridge before UI cutover).
//! - [`EnforcementPhase::Target`] (**active**): B live CRUD on published working
//!   catalog; draft/publish restricted to C; A fully read-only for tenants.
//!
//! Flip [`ACTIVE_ENFORCEMENT_PHASE`] only intentionally — Manager and Combobox
//! consume the same capability snapshot and need no structural rewrite.

use crate::errors::{AppError, AppResult};
use crate::reference::domains::ReferenceDomain;
use crate::reference::sets::ReferenceSet;
use serde::{Deserialize, Serialize};

// ─── Categories ───────────────────────────────────────────────────────────────

/// Canonical governance category (A / B / C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceCategory {
    /// A — System Catalog (product-owned, tenant read-only).
    SystemCatalog,
    /// B — Operational Dictionary (tenant live CRUD under Target phase).
    OperationalDictionary,
    /// C — Controlled Business Catalog (draft → publish).
    ControlledCatalog,
}

impl GovernanceCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemCatalog => "system_catalog",
            Self::OperationalDictionary => "operational_dictionary",
            Self::ControlledCatalog => "controlled_catalog",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "system_catalog" | "a" => Some(Self::SystemCatalog),
            "operational_dictionary" | "b" => Some(Self::OperationalDictionary),
            "controlled_catalog" | "c" => Some(Self::ControlledCatalog),
            _ => None,
        }
    }
}

pub const GOVERNANCE_CATEGORIES: &[&str] = &[
    "system_catalog",
    "operational_dictionary",
    "controlled_catalog",
];

// ─── Enforcement phase ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementPhase {
    /// Preserve B/C draft-only mutate; A is read-only (Manager SSOT).
    Compat,
    /// Full A/B/C target matrix (enable in a later PR).
    Target,
}

impl EnforcementPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compat => "compat",
            Self::Target => "target",
        }
    }
}

/// Active enforcement. Keep Compat until Combobox also consumes Target rules.
pub const ACTIVE_ENFORCEMENT_PHASE: EnforcementPhase = EnforcementPhase::Target;

// ─── Canonical domain → category map ──────────────────────────────────────────

/// Known production domain codes → category (philosophy table).
fn category_for_known_code(code: &str) -> Option<GovernanceCategory> {
    match normalize_domain_code(code).as_str() {
        "EQUIPMENT.STATUS"
        | "EQUIPMENT.CRITICALITY"
        | "EQUIPMENT.CLASS"
        | "DI.PRIORITY"
        | "DI.IMPACT_LEVEL"
        | "DI.REQUEST_TYPE" => Some(GovernanceCategory::SystemCatalog),

        "EQUIPMENT.FAMILY"
        | "EQUIPMENT.SUBFAMILY"
        | "DI.SYMPTOM"
        | "DI.ORIGIN"
        | "PERSONNEL.SKILLS"
        | "ORG.SCHEDULE_CLASS"
        | "WORK.DELAY_REASONS"
        | "WORK.PART_UNUSED_REASON" => Some(GovernanceCategory::OperationalDictionary),

        "WORK.FAILURE_MODES"
        | "WORK.SYMPTOMS"
        | "WORK.FAILURE_CAUSES"
        | "WORK.FAILURE_EFFECTS"
        | "PM.MAINTENANCE_TASK_LIST" => {
            Some(GovernanceCategory::ControlledCatalog)
        }

        _ => None,
    }
}

/// Domains that require analytical publish impact / protected delete semantics.
/// Subset of Category A (not all system catalogs).
fn is_analytical_system_code(code: &str) -> bool {
    matches!(
        normalize_domain_code(code).as_str(),
        "DI.PRIORITY" | "DI.IMPACT_LEVEL" | "DI.REQUEST_TYPE"
    )
}

fn normalize_domain_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

/// Map legacy `governance_level` when no known code / stored category exists.
fn category_from_legacy_level(governance_level: &str) -> GovernanceCategory {
    match governance_level.trim() {
        "tenant_managed" => GovernanceCategory::OperationalDictionary,
        "protected_analytical" | "system_seeded" | "erp_synced" => {
            GovernanceCategory::SystemCatalog
        }
        _ => GovernanceCategory::OperationalDictionary,
    }
}

/// Resolve the effective category for a domain.
///
/// Precedence: stored `governance_category` → known code map → legacy level.
pub fn resolve_category(
    domain_code: &str,
    legacy_governance_level: &str,
    stored_category: Option<&str>,
) -> GovernanceCategory {
    if let Some(raw) = stored_category {
        if let Some(cat) = GovernanceCategory::parse(raw) {
            return cat;
        }
    }
    if let Some(cat) = category_for_known_code(domain_code) {
        return cat;
    }
    category_from_legacy_level(legacy_governance_level)
}

/// Resolve category from a loaded domain row.
pub fn category_of(domain: &ReferenceDomain) -> GovernanceCategory {
    resolve_category(
        &domain.code,
        &domain.governance_level,
        Some(domain.governance_category.as_str()),
    )
}

/// Derive category to persist when creating/updating a domain.
pub fn derive_category_for_persist(
    domain_code: &str,
    legacy_governance_level: &str,
) -> GovernanceCategory {
    resolve_category(domain_code, legacy_governance_level, None)
}

/// Compat `is_extendable` mirror derived from category (A=false; B/C=true).
pub fn default_is_extendable(category: GovernanceCategory) -> bool {
    !matches!(category, GovernanceCategory::SystemCatalog)
}

// ─── Capability queries ───────────────────────────────────────────────────────

fn set_is_draft(set: &ReferenceSet) -> bool {
    set.status == "draft"
}

/// Operational create-from-dropdown (published working catalog).
pub fn allows_operational_create(category: GovernanceCategory) -> bool {
    match ACTIVE_ENFORCEMENT_PHASE {
        EnforcementPhase::Compat | EnforcementPhase::Target => {
            matches!(category, GovernanceCategory::OperationalDictionary)
        }
    }
}

/// Manager / draft CRUD create-update-deactivate-move on a given set.
pub fn allows_value_mutation(category: GovernanceCategory, set: &ReferenceSet) -> bool {
    match ACTIVE_ENFORCEMENT_PHASE {
        EnforcementPhase::Compat => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary | GovernanceCategory::ControlledCatalog => {
                set_is_draft(set)
            }
        },
        EnforcementPhase::Target => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary => set.status == "published",
            GovernanceCategory::ControlledCatalog => set_is_draft(set),
        },
    }
}

/// Creating a new draft set for the domain.
pub fn allows_create_draft_set(category: GovernanceCategory) -> bool {
    match ACTIVE_ENFORCEMENT_PHASE {
        EnforcementPhase::Compat => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary | GovernanceCategory::ControlledCatalog => {
                true
            }
        },
        EnforcementPhase::Target => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary => false,
            GovernanceCategory::ControlledCatalog => true,
        },
    }
}

/// Publishing a validated set (and related validate→publish path).
pub fn allows_publish(category: GovernanceCategory) -> bool {
    match ACTIVE_ENFORCEMENT_PHASE {
        EnforcementPhase::Compat => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary | GovernanceCategory::ControlledCatalog => {
                true
            }
        },
        EnforcementPhase::Target => match category {
            GovernanceCategory::SystemCatalog => false,
            GovernanceCategory::OperationalDictionary => false,
            GovernanceCategory::ControlledCatalog => true,
        },
    }
}

/// Whether governed publish requires an impact preview (analytical catalogs).
pub fn requires_publish_impact_preview(domain: &ReferenceDomain) -> bool {
    requires_analytical_protection(domain)
}

/// Protected-analytical delete / deactivate / import semantics.
pub fn requires_analytical_protection(domain: &ReferenceDomain) -> bool {
    // Compat-preserving: legacy level OR known analytical system codes.
    if domain.governance_level == "protected_analytical" {
        return true;
    }
    let cat = category_of(domain);
    matches!(cat, GovernanceCategory::SystemCatalog) && is_analytical_system_code(&domain.code)
}

// ─── Capabilities DTO (Manager / Combobox SSOT over the wire) ─────────────────

/// Snapshot of what the current actor's UI may offer for a domain (+ optional set).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceGovernanceCapabilities {
    pub category: String,
    pub enforcement_phase: String,
    pub can_create_value: bool,
    pub can_update_value: bool,
    pub can_deactivate_value: bool,
    pub can_create_draft_set: bool,
    pub can_discard_draft_set: bool,
    pub can_publish: bool,
    pub can_operational_create: bool,
    pub is_read_only: bool,
    pub requires_analytical_protection: bool,
    pub set_status: Option<String>,
}

/// Build capability snapshot for a domain and optional selected set.
///
/// When `set` is `None`, value-mutation flags are `false` (mutation is set-scoped).
pub fn capabilities_for(
    domain: &ReferenceDomain,
    set: Option<&ReferenceSet>,
) -> ReferenceGovernanceCapabilities {
    let cat = category_of(domain);
    let can_mutate = set
        .map(|s| allows_value_mutation(cat, s))
        .unwrap_or(false);
    let can_draft = allows_create_draft_set(cat);
    let can_discard = can_draft && set.map(|s| s.status == "draft").unwrap_or(false);
    let can_pub = allows_publish(cat);
    let can_op = allows_operational_create(cat);
    let read_only = matches!(cat, GovernanceCategory::SystemCatalog)
        || (!can_mutate && !can_draft && !can_pub && !can_op);

    ReferenceGovernanceCapabilities {
        category: cat.as_str().to_string(),
        enforcement_phase: ACTIVE_ENFORCEMENT_PHASE.as_str().to_string(),
        can_create_value: can_mutate,
        can_update_value: can_mutate,
        can_deactivate_value: can_mutate,
        can_create_draft_set: can_draft,
        can_discard_draft_set: can_discard,
        can_publish: can_pub,
        can_operational_create: can_op,
        is_read_only: read_only,
        requires_analytical_protection: requires_analytical_protection(domain),
        set_status: set.map(|s| s.status.clone()),
    }
}

// ─── Assert helpers (AppResult) ───────────────────────────────────────────────

pub fn assert_allows_operational_create(domain: &ReferenceDomain) -> AppResult<()> {
    let cat = category_of(domain);
    if allows_operational_create(cat) {
        return Ok(());
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Le domaine '{}' n'autorise pas la création opérationnelle \
         (catégorie={}).",
        domain.code,
        cat.as_str()
    )]))
}

pub fn assert_allows_value_mutation(
    domain: &ReferenceDomain,
    set: &ReferenceSet,
) -> AppResult<()> {
    let cat = category_of(domain);
    if allows_value_mutation(cat, set) {
        return Ok(());
    }
    if matches!(cat, GovernanceCategory::SystemCatalog) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le domaine '{}' est un catalogue système (lecture seule).",
            domain.code
        )]));
    }
    if !set_is_draft(set) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier un jeu en statut '{}'. \
             Seuls les brouillons ('draft') peuvent être modifiés.",
            set.status
        )]));
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Le domaine '{}' (catégorie={}) n'autorise pas cette mutation \
         sur un jeu en statut '{}'.",
        domain.code,
        cat.as_str(),
        set.status
    )]))
}

pub fn assert_allows_create_draft_set(domain: &ReferenceDomain) -> AppResult<()> {
    let cat = category_of(domain);
    if allows_create_draft_set(cat) {
        return Ok(());
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Le domaine '{}' (catégorie={}) n'autorise pas la création de brouillons.",
        domain.code,
        cat.as_str()
    )]))
}

pub fn assert_allows_publish(domain: &ReferenceDomain) -> AppResult<()> {
    let cat = category_of(domain);
    if allows_publish(cat) {
        return Ok(());
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Le domaine '{}' (catégorie={}) n'autorise pas la publication de jeux.",
        domain.code,
        cat.as_str()
    )]))
}

#[cfg(test)]
mod phase_tests {
    use super::*;

    #[test]
    fn active_phase_is_target() {
        assert_eq!(ACTIVE_ENFORCEMENT_PHASE, EnforcementPhase::Target);
    }
}
