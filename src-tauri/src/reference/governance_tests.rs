//! Unit tests for reference governance policy (A/B/C decision engine).

use crate::reference::domains::ReferenceDomain;
use crate::reference::governance::{
    allows_create_draft_set, allows_operational_create, allows_publish, allows_value_mutation,
    assert_allows_operational_create, assert_allows_value_mutation, assert_system_value_mutable, capabilities_for,
    category_of, default_is_extendable, derive_category_for_persist, metadata_marks_system_origin,
    requires_analytical_protection, requires_publish_impact_preview, resolve_category, system_values_locked,
    EnforcementPhase, GovernanceCategory, ACTIVE_ENFORCEMENT_PHASE,
};
use crate::reference::sets::ReferenceSet;

fn domain(code: &str, governance_level: &str, governance_category: &str, is_extendable: bool) -> ReferenceDomain {
    ReferenceDomain {
        id: 1,
        code: code.into(),
        name: code.into(),
        structure_type: "flat".into(),
        governance_level: governance_level.into(),
        governance_category: governance_category.into(),
        is_extendable,
        validation_rules_json: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}

fn set_with_status(status: &str) -> ReferenceSet {
    ReferenceSet {
        id: 10,
        domain_id: 1,
        version_no: 1,
        status: status.into(),
        effective_from: None,
        created_by_id: Some(1),
        created_at: "2026-01-01T00:00:00Z".into(),
        published_at: None,
    }
}

#[test]
fn known_codes_map_to_philosophy_categories() {
    assert_eq!(
        resolve_category("EQUIPMENT.CLASS", "system_seeded", None),
        GovernanceCategory::ControlledCatalog
    );
    assert_eq!(
        resolve_category("EQUIPMENT.STATUS", "system_seeded", None),
        GovernanceCategory::SystemCatalog
    );
    assert_eq!(
        resolve_category("EQUIPMENT.FAMILY", "tenant_managed", None),
        GovernanceCategory::OperationalDictionary
    );
    assert_eq!(
        resolve_category("WORK.FAILURE_MODES", "system_seeded", None),
        GovernanceCategory::ControlledCatalog
    );
}

#[test]
fn target_operational_create_only_for_b() {
    assert_eq!(ACTIVE_ENFORCEMENT_PHASE, EnforcementPhase::Target);
    assert!(!allows_operational_create(GovernanceCategory::SystemCatalog));
    assert!(allows_operational_create(GovernanceCategory::OperationalDictionary));
    assert!(!allows_operational_create(GovernanceCategory::ControlledCatalog));
}

#[test]
fn target_a_is_fully_read_only() {
    let draft = set_with_status("draft");
    let published = set_with_status("published");
    let a = GovernanceCategory::SystemCatalog;
    assert!(!allows_value_mutation(a, &draft));
    assert!(!allows_value_mutation(a, &published));
    assert!(!allows_create_draft_set(a));
    assert!(!allows_publish(a));
}

#[test]
fn target_b_live_on_published_no_draft_publish() {
    let draft = set_with_status("draft");
    let published = set_with_status("published");
    let b = GovernanceCategory::OperationalDictionary;
    assert!(!allows_value_mutation(b, &draft));
    assert!(allows_value_mutation(b, &published));
    assert!(!allows_create_draft_set(b));
    assert!(!allows_publish(b));
}

#[test]
fn target_c_draft_mutate_published_blocked() {
    let draft = set_with_status("draft");
    let published = set_with_status("published");
    let c = GovernanceCategory::ControlledCatalog;
    assert!(allows_value_mutation(c, &draft));
    assert!(!allows_value_mutation(c, &published));
    assert!(allows_create_draft_set(c));
    assert!(allows_publish(c));
}

#[test]
fn capabilities_snapshot_for_family_published() {
    let family = domain("EQUIPMENT.FAMILY", "tenant_managed", "operational_dictionary", true);
    let published = set_with_status("published");
    let caps = capabilities_for(&family, Some(&published));
    assert_eq!(caps.category, "operational_dictionary");
    assert_eq!(caps.enforcement_phase, "target");
    assert!(caps.can_create_value);
    assert!(caps.can_update_value);
    assert!(caps.can_deactivate_value);
    assert!(!caps.can_create_draft_set);
    assert!(!caps.can_discard_draft_set);
    assert!(!caps.can_publish);
    assert!(caps.can_operational_create);
    assert!(!caps.is_read_only);
    assert_eq!(caps.set_status.as_deref(), Some("published"));
}

#[test]
fn capabilities_snapshot_for_status_is_read_only() {
    let status = domain("EQUIPMENT.STATUS", "system_seeded", "system_catalog", false);
    let draft = set_with_status("draft");
    let caps = capabilities_for(&status, Some(&draft));
    assert!(caps.is_read_only);
    assert!(!caps.can_create_value);
    assert!(!caps.can_create_draft_set);
    assert!(!caps.can_discard_draft_set);
    assert!(!caps.can_publish);
    assert!(!caps.can_operational_create);
    assert!(caps.system_values_locked);
}

/// The class catalogue is extended by each plant through draft → publish. A seeder that still
/// stores `system_catalog` for it must not make it read-only again.
#[test]
fn capabilities_snapshot_for_class_is_controlled_and_locks_seeded_values() {
    let class = domain("EQUIPMENT.CLASS", "system_seeded", "system_catalog", false);
    assert_eq!(category_of(&class), GovernanceCategory::ControlledCatalog);
    let draft = set_with_status("draft");
    let caps = capabilities_for(&class, Some(&draft));
    assert_eq!(caps.category, "controlled_catalog");
    assert!(!caps.is_read_only);
    assert!(caps.can_create_value);
    assert!(caps.can_update_value);
    assert!(caps.can_deactivate_value);
    assert!(caps.can_create_draft_set);
    assert!(caps.can_discard_draft_set);
    assert!(caps.can_publish);
    assert!(!caps.can_operational_create);
    assert!(caps.system_values_locked);
    let published = set_with_status("published");
    let caps_pub = capabilities_for(&class, Some(&published));
    assert!(!caps_pub.can_create_value);
    assert!(caps_pub.can_create_draft_set);
}

#[test]
fn seeded_values_of_locked_domains_are_protected() {
    let class = domain("EQUIPMENT.CLASS", "system_seeded", "controlled_catalog", false);
    assert!(assert_system_value_mutable(&class, Some(r#"{"origin":"system"}"#)).is_err());
    assert!(assert_system_value_mutable(&class, None).is_ok());
    assert!(assert_system_value_mutable(&class, Some(r#"{"origin":"tenant"}"#)).is_ok());
    // Domains without integrity-owned values keep their seeded rows editable.
    let symptom = domain("DI.SYMPTOM", "tenant_managed", "operational_dictionary", true);
    assert!(!system_values_locked(&symptom));
    assert!(assert_system_value_mutable(&symptom, Some(r#"{"origin":"system"}"#)).is_ok());
    assert!(metadata_marks_system_origin(Some(r#"{"origin":"system","x":1}"#)));
    assert!(!metadata_marks_system_origin(Some("not json")));
}

#[test]
fn stored_category_still_applies_to_tenant_domains() {
    assert_eq!(
        resolve_category("PLANT.LOCAL", "tenant_managed", Some("controlled_catalog")),
        GovernanceCategory::ControlledCatalog
    );
}

#[test]
fn capabilities_without_set_blocks_value_mutation() {
    let family = domain("EQUIPMENT.FAMILY", "tenant_managed", "operational_dictionary", true);
    let caps = capabilities_for(&family, None);
    assert!(!caps.can_create_value);
    assert!(!caps.can_create_draft_set);
    assert!(!caps.can_discard_draft_set);
    assert!(caps.can_operational_create);
}

#[test]
fn capabilities_c_draft_allows_discard() {
    let failure = domain("WORK.FAILURE_MODES", "system_seeded", "controlled_catalog", false);
    let draft = set_with_status("draft");
    let published = set_with_status("published");
    let caps_draft = capabilities_for(&failure, Some(&draft));
    assert!(caps_draft.can_create_draft_set);
    assert!(caps_draft.can_discard_draft_set);
    assert!(caps_draft.can_create_value);
    let caps_pub = capabilities_for(&failure, Some(&published));
    assert!(caps_pub.can_create_draft_set);
    assert!(!caps_pub.can_discard_draft_set);
    assert!(!caps_pub.can_create_value);
}

#[test]
fn analytical_protection_matches_legacy_protected_analytical() {
    let di_priority = domain("DI.PRIORITY", "protected_analytical", "system_catalog", false);
    assert!(requires_analytical_protection(&di_priority));
    assert!(requires_publish_impact_preview(&di_priority));

    let class = domain("EQUIPMENT.CLASS", "system_seeded", "controlled_catalog", false);
    assert!(!requires_analytical_protection(&class));
}

#[test]
fn assert_operational_create_family_ok_class_rejected() {
    let family = domain("EQUIPMENT.FAMILY", "tenant_managed", "operational_dictionary", true);
    assert!(assert_allows_operational_create(&family).is_ok());

    let class = domain("EQUIPMENT.CLASS", "system_seeded", "system_catalog", false);
    assert!(assert_allows_operational_create(&class).is_err());
}

#[test]
fn assert_value_mutation_system_catalog_rejected_even_on_draft() {
    let criticality = domain("EQUIPMENT.CRITICALITY", "system_seeded", "system_catalog", false);
    let draft = set_with_status("draft");
    let err = assert_allows_value_mutation(&criticality, &draft).expect_err("A read-only");
    match err {
        crate::errors::AppError::ValidationFailed(msgs) => {
            let joined = msgs.join(" ");
            assert!(joined.contains("système") || joined.contains("lecture"));
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }
}

#[test]
fn default_extendable_mirrors_category() {
    assert!(!default_is_extendable(GovernanceCategory::SystemCatalog));
    assert!(default_is_extendable(GovernanceCategory::OperationalDictionary));
}

#[test]
fn derive_persist_uses_known_map() {
    assert_eq!(
        derive_category_for_persist("WORK.FAILURE_MODES", "system_seeded"),
        GovernanceCategory::ControlledCatalog
    );
    assert_eq!(
        category_of(&domain(
            "WORK.FAILURE_MODES",
            "system_seeded",
            "controlled_catalog",
            false
        )),
        GovernanceCategory::ControlledCatalog
    );
}
