//! CI guards for the per-user signing rules (`sync::signing_rules`).

use std::collections::{BTreeSet, HashSet};

use crate::rbac::permissions;

use super::registry::{self, SYNC_ENTITIES};
use super::row_layout::{self, Role, Target};
use super::row_spec::ROW_SPECS;
use super::signing_rules::{author_payload_key, signing_rule, SERVER_ONLY_TYPES, SIGNING_RULES};

const USER: &str = "user_accounts";

/// Rights the server can judge as an `authority` (plant-wide holder in its own state).
const KNOWN_AUTHORITIES: &[&str] = &[
    permissions::ADM_USERS,
    permissions::ADM_ROLES,
    permissions::ADM_SETTINGS,
    permissions::ORG_ADMIN,
];

fn is_user_reference(role: &Role) -> bool {
    match role {
        Role::Ref {
            target: Target::Entity { entity_type, .. },
            ..
        } => *entity_type == USER,
        Role::Ref {
            target: Target::Poly { targets, .. },
            ..
        } => targets.iter().any(|(_, entity_type, _)| *entity_type == USER),
        _ => false,
    }
}

fn is_plain_user_reference(role: &Role) -> bool {
    matches!(
        role,
        Role::Ref {
            target: Target::Entity { entity_type: USER, .. },
            ..
        }
    )
}

/// Every column of a synchronised table that refers to a user account is classified exactly once
/// (author field or not attribution), every classified column exists and refers to a user, and
/// every guard is a payload key of its row. A migration adding a user column fails here until the
/// column is classified in `signing_rules.rs`.
#[tokio::test]
async fn every_user_reference_is_classified() {
    let db = crate::db::test_db::migrated().await;
    let mut problems: Vec<String> = Vec::new();
    for spec in ROW_SPECS {
        let entity_type = spec.entity_type;
        let layout = row_layout::layout(&db, entity_type)
            .await
            .unwrap_or_else(|e| panic!("layout of {entity_type}: {e}"));
        let rule = signing_rule(entity_type);
        let authors: HashSet<&str> = rule
            .map(|r| r.author_fields.iter().map(|f| f.column).collect())
            .unwrap_or_default();
        let not_attribution: HashSet<&str> = rule
            .map(|r| r.not_attribution.iter().copied().collect())
            .unwrap_or_default();

        let mut payload_keys: HashSet<&str> = HashSet::new();
        for c in &layout.columns {
            match &c.role {
                Role::Value { key } | Role::Ref { key, .. } => {
                    payload_keys.insert(key.as_str());
                }
                _ => {}
            }
            let names_user = is_user_reference(&c.role) || c.json_refs.iter().any(|j| j.entity_type == USER);
            if !names_user {
                continue;
            }
            let name = c.name.as_str();
            match (authors.contains(name), not_attribution.contains(name)) {
                (false, false) => problems.push(format!(
                    "{entity_type}.{name} refers to a user account and is not classified in signing_rules.rs"
                )),
                (true, true) => problems.push(format!(
                    "{entity_type}.{name} is both an author field and not attribution"
                )),
                _ => {}
            }
        }

        let Some(rule) = rule else { continue };
        for field in rule.author_fields {
            match layout.column(field.column) {
                Some(c) if is_plain_user_reference(&c.role) => {}
                Some(_) => problems.push(format!(
                    "{entity_type}.{}: an author field must be a plain reference to user_accounts",
                    field.column
                )),
                None => problems.push(format!(
                    "{entity_type}.{}: author field is not a synchronised column",
                    field.column
                )),
            }
            for guard in field.guards {
                if !payload_keys.contains(guard) {
                    problems.push(format!(
                        "{entity_type}.{}: guard {guard} is not a payload key of the row",
                        field.column
                    ));
                }
            }
        }
        for column in rule.not_attribution {
            let ok = layout
                .column(column)
                .is_some_and(|c| is_user_reference(&c.role) || c.json_refs.iter().any(|j| j.entity_type == USER));
            if !ok {
                problems.push(format!(
                    "{entity_type}.{column}: listed as not attribution but holds no user reference"
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "signing rules out of date:\n{}",
        problems.join("\n")
    );
}

/// Rules are sorted, unique, name staged types, and use only rights the server can judge.
#[test]
fn signing_rules_are_well_formed() {
    let staged: HashSet<&str> = SYNC_ENTITIES
        .iter()
        .filter(|e| matches!(e.sync_status(), "synced" | "staged_only"))
        .map(|e| e.entity_type)
        .collect();
    let mut previous = "";
    for rule in SIGNING_RULES {
        assert!(
            rule.entity_type > previous,
            "SIGNING_RULES not sorted/unique at {}",
            rule.entity_type
        );
        previous = rule.entity_type;
        assert!(
            staged.contains(rule.entity_type),
            "{} is not a staged sync type",
            rule.entity_type
        );
        if let Some(a) = rule.authority {
            assert!(
                KNOWN_AUTHORITIES.contains(&a),
                "{}: unknown authority {a}",
                rule.entity_type
            );
        }
        let mut keys = BTreeSet::new();
        for f in rule.author_fields {
            assert!(
                keys.insert(author_payload_key(f)),
                "{}: author field listed twice",
                rule.entity_type
            );
        }
    }
    for (entity_type, scope) in SERVER_ONLY_TYPES {
        assert!(
            registry::entity(entity_type).is_none(),
            "{entity_type} is server-only but staged here"
        );
        assert!(matches!(*scope, "tenant" | "user"));
    }
    // The authority types the server judges (design §5) keep their right.
    for (entity_type, right) in [
        ("user_accounts", permissions::ADM_USERS),
        ("user_scope_assignments", permissions::ADM_USERS),
        ("delegated_admin_policies", permissions::ADM_USERS),
        ("roles", permissions::ADM_ROLES),
        ("role_permissions", permissions::ADM_ROLES),
    ] {
        assert_eq!(
            signing_rule(entity_type).and_then(|r| r.authority),
            Some(right),
            "{entity_type}"
        );
    }
}

/// The exported registry carries the signing contract the allowlist script copies to the server.
#[test]
fn registry_export_carries_signing_contract() {
    let json: serde_json::Value =
        serde_json::from_str(&registry::registry_export_json().expect("export")).expect("json");
    let entity = |t: &str| {
        json["entities"]
            .as_array()
            .expect("entities")
            .iter()
            .find(|e| e["entity_type"] == t)
            .cloned()
            .unwrap_or_else(|| panic!("{t} exported"))
    };
    let accounts = entity("user_accounts");
    assert_eq!(accounts["authority"], permissions::ADM_USERS);
    let fields: Vec<&str> = accounts["author_fields"]
        .as_array()
        .expect("author_fields")
        .iter()
        .map(|f| f["field"].as_str().expect("field"))
        .collect();
    assert!(fields.contains(&"password_changed_by_sync_id"));
    assert!(fields.contains(&"status_changed_by_sync_id"));
    assert!(entity("articles").get("author_fields").is_none());
    assert_eq!(json["server_only_types"][0]["entity_type"], "user_signing_keys");
}
