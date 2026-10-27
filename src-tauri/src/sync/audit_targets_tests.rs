//! Every audit target type the application writes is either sent by the sync codec (mapped in the
//! `audit_events` target mapping of `sync::row_spec`) or kept on its device on purpose (listed in
//! that entity's row filter). Otherwise its events are sealed in their local form and never leave
//! the PC, without anyone deciding so (`audit::chain::seal` logs it as an error): this test makes a
//! new unmapped type fail CI instead.
//!
//! The written types are read from the source: the `target_type` / `entity_type` literal of every
//! `AuditEventInput { … }` / `AuditEvent { … }` built outside tests, plus the types set at run time,
//! listed below with where they come from. Every file that sets a type at run time must be listed
//! too (`DYNAMIC_SITES`), so a new such site fails the test until its types are added.

use std::collections::BTreeSet;
use std::path::Path;

use crate::sync::row_spec::{row_spec, RefTarget};

/// Types set at run time (not a literal at the construction site).
const RUNTIME_TYPES: &[(&str, &str)] = &[
    (
        "inventory_transactions",
        "inventory::count_sync::write_posted_twice_audit",
    ),
    (
        "inventory_mutation_audit_links",
        "inventory::count_sync::write_posted_twice_audit",
    ),
    (
        "inventory_count_sessions",
        "inventory::count_sync::write_posted_twice_audit",
    ),
    ("work_order", "org::site_assignment (NoSiteKind::audit_target)"),
    (
        "intervention_request",
        "org::site_assignment (NoSiteKind::audit_target)",
    ),
    ("planning_window", "org::site_assignment (NoSiteKind::audit_target)"),
    ("work_order", "qualification::gate (override context)"),
    ("permit_role", "qualification::gate (override context)"),
    (
        "schedule_commitment",
        "planning::sync_check::audit_closed (frozen-week conflict)",
    ),
    (
        "weekly_schedule",
        "planning::sync_check::audit_closed (reopened-week conflict)",
    ),
];

/// Files that build an audit event with a target type set at run time, and where the types they
/// can write are listed (`RUNTIME_TYPES`, or the literal events that feed them). A new such file
/// fails the test until its types are added to `RUNTIME_TYPES` and the file is listed here.
const DYNAMIC_SITES: &[(&str, &str)] = &[
    (
        "audit/mod.rs",
        "audit::emit: fed by the AuditEvent literals, which are scanned",
    ),
    (
        "commands/admin_governance.rs",
        "user_account (also written as a literal)",
    ),
    ("inventory/count_sync.rs", "write_posted_twice_audit: RUNTIME_TYPES"),
    ("org/site_assignment.rs", "NoSiteKind::audit_target: RUNTIME_TYPES"),
    ("qualification/gate.rs", "override context: RUNTIME_TYPES"),
    (
        "planning/sync_check.rs",
        "audit_closed (planning conflict closed by itself): RUNTIME_TYPES",
    ),
];

/// Types kept on their device by an action-code term of the row filter (not by their type). Only
/// this action is covered: a notification audit with another action code must be handled on its own.
const KEPT_BY_ACTION: &[(&str, &str)] = &[("notification", "notification.safety_notice_acknowledged")];

/// Known gaps, reported to their domain leads (not decided here): written with a target the codec
/// cannot encode, so their events stay on the PC that wrote them (stub only) and `seal` logs an
/// error. Each must be mapped (if the target travels) or added to the row filter (if it is kept on
/// purpose), then removed from this list. Routed by the Coordinator to the lead that owns each one.
const KNOWN_GAPS: &[(&str, &str)] = &[];

fn source_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "migrations") {
                continue;
            }
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !(name.ends_with("_tests.rs") || name == "tests.rs" || name == "test_support.rs") {
                out.push(path);
            }
        }
    }
}

/// What the source scan finds in the audit events built outside tests.
struct Scan {
    /// `target_type` / `entity_type` literals.
    literal: BTreeSet<String>,
    /// Source files (relative to `src`) that build an audit event with a non-literal target type.
    dynamic_files: BTreeSet<String>,
}

/// The text between the `{` that opens a construction (the text starts right after it) and its
/// matching `}`; string and character literals are skipped so their braces do not count.
fn construction_body(text: &str) -> &str {
    let bytes = text.as_bytes();
    let (mut depth, mut i) = (1usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' if bytes.get(i + 2) == Some(&b'\'') => i += 2,
            b'\'' if bytes.get(i + 1) == Some(&b'\\') && bytes.get(i + 3) == Some(&b'\'') => i += 3,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[..i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    text
}

/// Scans the application's (non-test) source for the audit events it builds.
fn scan_audit_constructions() -> Scan {
    let construct = regex::Regex::new(r"\bAuditEvent(?:Input)?\s*\{").expect("regex");
    // `target_type: Some("literal"`, `target_type: None`, anything else (`Some(kind.to_string())`,
    // `target_type: value`, the shorthand `target_type,`) is a type set at run time. A field of
    // the literal starts after whitespace, `{` or `,`, never after `.` (`row.entity_type,` is a
    // value read inside the literal, not a field of it).
    let field = regex::Regex::new(
        r#"(?:^|[\s{,])(?:target_type|entity_type)\s*(?::\s*(?:Some\(\s*"(?P<ty>[A-Za-z0-9_]+)"|(?P<none>None))?|(?P<short>[,}]))"#,
    )
    .expect("regex");
    // Inline test modules sit at the end of their file (`mod x;` declarations do not count).
    let test_module = regex::Regex::new(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{").expect("regex");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    let mut scan = Scan {
        literal: BTreeSet::new(),
        dynamic_files: BTreeSet::new(),
    };
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read source");
        let text = test_module.split(&text).next().unwrap_or_default();
        let relative = file
            .strip_prefix(&src)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        for m in construct.find_iter(text) {
            if text[..m.start()].ends_with("struct ") {
                continue;
            }
            for c in field.captures_iter(construction_body(&text[m.end()..])) {
                if let Some(ty) = c.name("ty") {
                    scan.literal.insert(ty.as_str().to_string());
                } else if c.name("none").is_none() {
                    scan.dynamic_files.insert(relative.clone());
                }
            }
        }
    }
    scan
}

fn mapped_types() -> BTreeSet<String> {
    let spec = row_spec("audit_events").expect("audit_events is synchronised");
    spec.refs
        .iter()
        .filter(|r| r.column == "target_id")
        .flat_map(|r| match r.target {
            RefTarget::Poly { targets, .. } => targets.iter().map(|(t, _)| (*t).to_string()).collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

fn kept_by_filter(target_type: &str) -> bool {
    let filter = row_spec("audit_events").and_then(|s| s.row_filter).unwrap_or_default();
    let (_, types) = filter
        .split_once("target_type, '') NOT IN (")
        .expect("the audit_events filter lists the target types it keeps");
    let list = types.split(')').next().unwrap_or_default();
    list.split(',').any(|t| t.trim().trim_matches('\'') == target_type)
}

#[test]
fn every_written_audit_target_type_is_sent_or_kept_on_purpose() {
    let scan = scan_audit_constructions();
    let literal = scan.literal;
    assert!(
        literal.contains("user_account") && literal.contains("stock_location_article"),
        "the source scan finds the audit constructions: {literal:?}"
    );
    let unknown_sites: Vec<&String> = scan
        .dynamic_files
        .iter()
        .filter(|f| !DYNAMIC_SITES.iter().any(|(k, _)| k == &f.as_str()))
        .collect();
    assert!(
        unknown_sites.is_empty(),
        "audit events built with a target type set at run time in files not listed in DYNAMIC_SITES:          {unknown_sites:?}. Add every type they can write to RUNTIME_TYPES (it must be mapped for sync or          kept on purpose), then list the file in DYNAMIC_SITES"
    );
    for (file, what) in DYNAMIC_SITES {
        assert!(
            scan.dynamic_files.contains(*file),
            "{file} ({what}) no longer builds an audit event with a run-time target type: remove it from DYNAMIC_SITES"
        );
    }
    let written: BTreeSet<String> = literal
        .into_iter()
        .chain(RUNTIME_TYPES.iter().map(|(t, _)| (*t).to_string()))
        .collect();
    let mapped = mapped_types();
    let gaps: BTreeSet<&str> = KNOWN_GAPS.iter().map(|(t, _)| *t).collect();
    let unhandled: Vec<&String> = written
        .iter()
        .filter(|t| {
            !mapped.contains(*t)
                && !kept_by_filter(t)
                && !KEPT_BY_ACTION.iter().any(|(k, _)| k == t)
                && !gaps.contains(t.as_str())
        })
        .collect();
    assert!(
        unhandled.is_empty(),
        "audit target types written but neither mapped for sync nor kept on purpose: {unhandled:?}"
    );
    // A gap that was fixed (or is no longer written) leaves the list.
    for (gap, owner) in KNOWN_GAPS {
        assert!(
            !mapped.contains(*gap) && !kept_by_filter(gap),
            "{gap} ({owner}) is handled now: remove it from KNOWN_GAPS"
        );
        assert!(
            written.contains(*gap),
            "{gap} ({owner}) is no longer written: remove it from KNOWN_GAPS"
        );
    }
    for (kept, action) in KEPT_BY_ACTION {
        let filter = row_spec("audit_events").and_then(|s| s.row_filter).unwrap_or_default();
        assert!(
            filter.contains(action),
            "{kept}: its action {action} is in the row filter"
        );
    }
}

/// A qualification override in a permit role (`qualification::gate::write_override`: target type
/// `permit_role`, target id the permit's local id, the role in the details) travels: it is sealed
/// in its sync form on the PC that wrote it, the other PC reads the same permit under its own local
/// id with the same role, and both PCs verify the chain.
#[tokio::test]
async fn a_permit_role_override_audit_travels_and_names_the_same_permit_on_the_other_pc() {
    use crate::audit::chain::verify_audit_chain;
    use crate::audit::writer::{write_audit_event, AuditEventInput};
    use crate::sync::test_support::{scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut factory_a = RowFactory::for_device(1);
    let mut factory_b = RowFactory::for_device(2);
    // B already has permits of its own, so A's permit gets another local id there.
    for _ in 0..3 {
        factory_b.create(&b, "work_permits").await.expect("permit on B");
    }
    let permit = factory_a.create(&a, "work_permits").await.expect("permit on A");
    let id = write_audit_event(
        &a,
        AuditEventInput {
            action_code: "qualification.override".to_string(),
            target_type: Some("permit_role".to_string()),
            target_id: Some(permit.rowid.to_string()),
            actor_id: None,
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "role": "isolation_apply",
                "person": "Karim B.",
                "gaps": [{ "code": "QUALIFICATION_MISSING", "qualification": "Habilitation B2V" }],
                "reason": "Chargé de consignation absent",
            })),
        },
    )
    .await
    .expect("write on A");
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE id = ? AND chain_hash NOT LIKE 'L%'",
            vec![id.into()],
        )
        .await,
        1,
        "sealed in its sync form on A (not kept on A as a local-form event)"
    );
    assert!(verify_audit_chain(&a).await.expect("verify on A").intact);

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");

    let permit_on_b = scalar_i64(
        &b,
        "SELECT id FROM work_permits WHERE entity_sync_id = ?",
        vec![permit.entity_sync_id.clone().into()],
    )
    .await;
    assert_ne!(
        permit_on_b, permit.rowid,
        "the test needs different local ids on the two PCs"
    );
    let on_b = |column: &str| {
        format!(
            "SELECT {column} FROM audit_events WHERE action_code = 'qualification.override' \
             AND target_type = 'permit_role'"
        )
    };
    assert_eq!(
        scalar_opt_text(&b, &on_b("target_id"), vec![]).await,
        Some(permit_on_b.to_string()),
        "the audit target is B's id of the same permit"
    );
    assert_eq!(
        scalar_opt_text(&b, &on_b("json_extract(details_json, '$.role')"), vec![]).await,
        Some("isolation_apply".to_string()),
        "the role travels in the details"
    );
    let verified = verify_audit_chain(&b).await.expect("verify on B");
    assert!(verified.intact, "{verified:?}");
    let from_a = verified.chains.iter().find(|c| !c.is_this_device).expect("A's chain");
    assert_eq!(
        (from_a.events, from_a.kept_positions),
        (1, 0),
        "B holds the event itself, not a stub"
    );
}

/// The ids in the details of a qualification override audit (`qualification::gate::write_override`,
/// under `$.qualification_override`) name the same rows on every PC: the override by its sync id
/// (sent as it is), the personnel record (permit-role override of a person) and the login
/// (work-order override of an account without a personnel record) by B's own local ids.
#[tokio::test]
async fn qualification_override_audit_details_name_the_same_override_and_person_on_the_other_pc() {
    use crate::audit::chain::verify_audit_chain;
    use crate::audit::writer::{write_audit_event, AuditEventInput};
    use crate::sync::test_support::{scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut factory_a = RowFactory::for_device(1);
    let mut factory_b = RowFactory::for_device(2);
    // B already has rows of its own, so A's rows get other local ids there.
    for entity in ["personnel", "user_accounts", "work_permits", "work_orders"] {
        for _ in 0..3 {
            factory_b.create(&b, entity).await.expect("row on B");
        }
    }
    let person = factory_a.create(&a, "personnel").await.expect("person on A");
    let account = factory_a.create(&a, "user_accounts").await.expect("account on A");
    let permit = factory_a.create(&a, "work_permits").await.expect("permit on A");
    let work_order = factory_a.create(&a, "work_orders").await.expect("work order on A");
    let overridden = factory_a
        .create(&a, "qualification_overrides")
        .await
        .expect("override on A");
    let event = |target_type: &str, target_id: i64, personnel_id: Option<i64>, user_id: Option<i64>| AuditEventInput {
        action_code: "qualification.override".to_string(),
        target_type: Some(target_type.to_string()),
        target_id: Some(target_id.to_string()),
        actor_id: None,
        auth_context: "step_up".to_string(),
        result: "success".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(serde_json::json!({
            "qualification_override": {
                "sync_id": overridden.entity_sync_id,
                "personnel_id": personnel_id,
                "user_id": user_id,
            },
            "person": "Karim B.",
            "reason": "Chargé de consignation absent",
        })),
    };
    for input in [
        event("permit_role", permit.rowid, Some(person.rowid), None),
        event("work_order", work_order.rowid, None, Some(account.rowid)),
    ] {
        let id = write_audit_event(&a, input).await.expect("write on A");
        assert_eq!(
            scalar_i64(
                &a,
                "SELECT COUNT(*) FROM audit_events WHERE id = ? AND chain_hash NOT LIKE 'L%'",
                vec![id.into()],
            )
            .await,
            1,
            "sealed in its sync form on A"
        );
    }
    assert!(verify_audit_chain(&a).await.expect("verify on A").intact);

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");

    let id_on_b = |table: &str, sync_id: String| {
        let b = &b;
        let sql = format!("SELECT id FROM {table} WHERE entity_sync_id = ?");
        async move { scalar_i64(b, &sql, vec![sync_id.into()]).await }
    };
    let person_on_b = id_on_b("personnel", person.entity_sync_id.clone()).await;
    let account_on_b = id_on_b("user_accounts", account.entity_sync_id.clone()).await;
    assert_ne!(
        person_on_b, person.rowid,
        "the test needs different local ids on the two PCs"
    );
    assert_ne!(
        account_on_b, account.rowid,
        "the test needs different local ids on the two PCs"
    );
    let override_on_b = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM qualification_overrides WHERE entity_sync_id = ?",
        vec![overridden.entity_sync_id.clone().into()],
    )
    .await;
    assert_eq!(override_on_b, 1, "the override row is on B under the same sync id");

    let detail = |target_type: &'static str, path: &'static str| {
        let b = &b;
        async move {
            scalar_opt_text(
                b,
                &format!(
                    "SELECT CAST(json_extract(details_json, '{path}') AS TEXT) FROM audit_events \
                     WHERE action_code = 'qualification.override' AND target_type = '{target_type}'"
                ),
                vec![],
            )
            .await
        }
    };
    for target_type in ["permit_role", "work_order"] {
        assert_eq!(
            detail(target_type, "$.qualification_override.sync_id").await,
            Some(overridden.entity_sync_id.clone()),
            "{target_type}: the override is named by its sync id on B"
        );
    }
    assert_eq!(
        detail("permit_role", "$.qualification_override.personnel_id").await,
        Some(person_on_b.to_string()),
        "the person is B's id of the same personnel record"
    );
    assert_eq!(detail("permit_role", "$.qualification_override.user_id").await, None);
    assert_eq!(
        detail("work_order", "$.qualification_override.user_id").await,
        Some(account_on_b.to_string()),
        "the account is B's id of the same login"
    );
    assert_eq!(
        detail("work_order", "$.qualification_override.personnel_id").await,
        None
    );
    let verified = verify_audit_chain(&b).await.expect("verify on B");
    assert!(verified.intact, "{verified:?}");
}
