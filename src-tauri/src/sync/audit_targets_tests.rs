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
    (
        "schedule_break_in",
        "planning::sync_check::audit_closed (break-in conflict)",
    ),
    ("team", "planning::sync_check::audit_closed (crew site change conflict)"),
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

/// A qualification override audit in the shape `qualification::gate::write_override` writes it.
fn override_event(
    target_type: &str,
    target_id: i64,
    details: serde_json::Value,
) -> crate::audit::writer::AuditEventInput {
    crate::audit::writer::AuditEventInput {
        action_code: "qualification.override".to_string(),
        target_type: Some(target_type.to_string()),
        target_id: Some(target_id.to_string()),
        actor_id: None,
        auth_context: "step_up".to_string(),
        result: "success".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(details),
    }
}

/// Events sealed by earlier versions, and rows that arrive late:
/// - an override audit in the old shape (local ids at the top level of the details, target
///   `work_order`) still travels and verifies on both PCs: the new detail mappings do not touch it;
/// - a permit_role audit an earlier version kept on its PC (sealed `L2:`) stays there: only its
///   stub travels, and both PCs verify the chain;
/// - a permit_role audit whose permit has not reached B yet waits, then is applied with B's id of
///   the permit once it arrives;
/// - a PC that received the new-shape audit before its own upgrade stored the sender's sync ids
///   unchanged in the details: after the upgrade it still verifies the sender's chain.
#[tokio::test]
async fn override_audits_sealed_earlier_or_received_late_keep_a_verifiable_chain() {
    use crate::audit::chain::{reseal_kept_on_device, verify_audit_chain};
    use crate::audit::writer::write_audit_event;
    use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut factory_a = RowFactory::for_device(1);
    let mut factory_b = RowFactory::for_device(2);
    for entity in ["personnel", "work_permits", "work_orders"] {
        for _ in 0..3 {
            factory_b.create(&b, entity).await.expect("row on B");
        }
    }
    let person = factory_a.create(&a, "personnel").await.expect("person on A");
    let work_order = factory_a.create(&a, "work_orders").await.expect("work order on A");
    let permit = factory_a.create(&a, "work_permits").await.expect("permit on A");
    let text_on = |db: &'static str, sql: &'static str, sync_id: String| {
        let conn = if db == "A" { &a } else { &b };
        async move { scalar_opt_text(conn, sql, vec![sync_id.into()]).await }
    };
    let event_sync_id = |id: i64| {
        let a = &a;
        async move {
            scalar_opt_text(
                a,
                "SELECT entity_sync_id FROM audit_events WHERE id = ?",
                vec![id.into()],
            )
            .await
            .expect("sync id")
        }
    };
    const HASH: &str = "SELECT chain_hash FROM audit_events WHERE entity_sync_id = ?";
    let count_on_b = |sync_id: String| {
        let b = &b;
        async move {
            scalar_i64(
                b,
                "SELECT COUNT(*) FROM audit_events WHERE entity_sync_id = ?",
                vec![sync_id.into()],
            )
            .await
        }
    };

    // Old shape, as the previous version wrote it (sealed in sync form: work_order was mapped).
    let old_shape = write_audit_event(
        &a,
        override_event(
            "work_order",
            work_order.rowid,
            serde_json::json!({
                "override_id": 7,
                "personnel_id": person.rowid,
                "user_id": null,
                "person": "Karim B.",
                "reason": "Ancien format",
            }),
        ),
    )
    .await
    .expect("old shape");
    let old_shape = event_sync_id(old_shape).await;
    let old_hash = text_on("A", HASH, old_shape.clone()).await.expect("hash");
    assert!(!old_hash.starts_with('L'), "old shape: sync form: {old_hash}");

    // A permit_role audit an earlier version kept on A (it could not encode the target).
    let kept = write_audit_event(
        &a,
        override_event(
            "permit_role",
            permit.rowid,
            serde_json::json!({ "role": "isolation_apply", "reason": "Gardé sur ce PC" }),
        ),
    )
    .await
    .expect("kept");
    let kept = event_sync_id(kept).await;
    reseal_kept_on_device(&a, &kept).await.expect("reseal as kept");

    // A new permit_role audit, in the new shape.
    let new_shape = write_audit_event(
        &a,
        override_event(
            "permit_role",
            permit.rowid,
            serde_json::json!({
                "qualification_override": {
                    "sync_id": crate::sync::identity::new_entity_sync_id(),
                    "personnel_id": person.rowid,
                    "user_id": null,
                },
                "role": "isolation_apply",
                "reason": "Nouveau format",
            }),
        ),
    )
    .await
    .expect("new shape");
    let new_shape = event_sync_id(new_shape).await;
    let new_hash = text_on("A", HASH, new_shape.clone()).await.expect("hash");
    assert!(!new_hash.starts_with('L'), "new shape: sync form: {new_hash}");
    assert!(verify_audit_chain(&a).await.expect("verify on A").intact);

    // The permit reaches B late: its permit type (and so the permit) is held back.
    let push_a = server.exchange("A", &a).await;
    assert_eq!(push_a.conflicts, 0, "{push_a:?}");
    let permit_type = scalar_opt_text(
        &a,
        "SELECT pt.entity_sync_id FROM permit_types pt JOIN work_permits wp ON wp.permit_type_id = pt.id \
         WHERE wp.id = ?",
        vec![permit.rowid.into()],
    )
    .await
    .expect("permit type");
    server.hold_back("B", &permit_type);
    let first = server.exchange("B", &b).await;
    assert_eq!(first.materialized.conflicts, 0, "{first:?}");
    assert_eq!(count_on_b(old_shape.clone()).await, 1, "the old shape travels");
    assert_eq!(count_on_b(kept.clone()).await, 0, "an event kept before stays kept");
    assert_eq!(count_on_b(new_shape.clone()).await, 0, "the audit waits for its permit");
    let kept_hash = text_on("A", HASH, kept.clone()).await.expect("hash");
    assert!(kept_hash.starts_with("L2:"), "kept, in its local form: {kept_hash}");

    server.release("B");
    let second = server.exchange("B", &b).await;
    assert_eq!(second.materialized.conflicts, 0, "{second:?}");
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
    assert_eq!(
        text_on(
            "B",
            "SELECT target_id FROM audit_events WHERE entity_sync_id = ?",
            new_shape.clone()
        )
        .await,
        Some(permit_on_b.to_string()),
        "applied once the permit arrived, naming B's id of it"
    );
    let on_a = verify_audit_chain(&a).await.expect("verify on A");
    assert!(on_a.intact, "{on_a:?}");
    let on_b = verify_audit_chain(&b).await.expect("verify on B");
    assert!(on_b.intact, "{on_b:?}");
    let from_a = on_b.chains.iter().find(|c| !c.is_this_device).expect("A's chain");
    assert_eq!(
        (from_a.events, from_a.kept_positions),
        (2, 1),
        "B holds the two travelling events and the stub of the kept one"
    );

    // As B stored the event before its upgrade: the sender's sync id in the details, unchanged.
    exec(
        &b,
        "UPDATE audit_events SET details_json = \
         json_set(details_json, '$.qualification_override.personnel_id', ?) WHERE entity_sync_id = ?",
        vec![person.entity_sync_id.clone().into(), new_shape.clone().into()],
    )
    .await;
    let upgraded = verify_audit_chain(&b).await.expect("verify on B after its upgrade");
    assert!(
        upgraded.intact,
        "a sync id stored by an older B is re-encoded as it is: {upgraded:?}"
    );
}

/// Through the real writer (`qualification::gate`): a person let into a permit role and an account
/// let onto a work order, each by an override. The sealed audit names the override by the sync id
/// of the row just written and the subject by its local id under `$.qualification_override` (no id
/// left at the top level), and is sealed in its sync form, so it travels.
#[tokio::test]
async fn the_gate_seals_override_audits_with_details_that_travel() {
    use crate::audit::chain::verify_audit_chain;
    use crate::qualification::gate::{
        enforce_permit_role, record_account_work_order_override, QualificationOverrideInput,
    };
    use crate::qualification::requirements::{upsert_requirement, QualificationRequirementUpsertInput};
    use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory};

    let db = started_device_db().await;
    let mut factory = RowFactory::for_device(1);
    let approver = factory.create(&db, "user_accounts").await.expect("approver").rowid;
    let cert = factory
        .create(&db, "certification_types")
        .await
        .expect("certification type")
        .rowid;
    let person = factory.create(&db, "personnel").await.expect("person").rowid;
    let permit = factory.create(&db, "work_permits").await.expect("permit").rowid;
    let work_order = factory.create(&db, "work_orders").await.expect("work order").rowid;
    let account = factory.create(&db, "user_accounts").await.expect("account").rowid;
    // A login without a personnel record (the factory links one).
    exec(
        &db,
        "UPDATE user_accounts SET personnel_id = NULL WHERE id = ?",
        vec![account.into()],
    )
    .await;
    let requirement = |kind: &str| QualificationRequirementUpsertInput {
        id: None,
        expected_row_version: None,
        target_kind: kind.into(),
        work_order_id: None,
        target_code: None,
        certification_type_id: Some(cert),
        skill_reference_value_id: None,
        min_proficiency_level: None,
        coverage: None,
        notes: None,
        is_active: None,
    };
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            target_code: Some("isolation_apply".into()),
            ..requirement("loto_role")
        },
        approver,
    )
    .await
    .expect("role requirement");
    upsert_requirement(
        &db,
        QualificationRequirementUpsertInput {
            work_order_id: Some(work_order),
            coverage: Some("each".into()),
            ..requirement("work_order")
        },
        approver,
    )
    .await
    .expect("work order requirement");
    let input = QualificationOverrideInput {
        reason: "Chargé de consignation absent, sous contrôle du responsable".into(),
        approved_by_id: approver,
    };

    let person_override = enforce_permit_role(&db, person, permit, "isolation_apply", Some(&input))
        .await
        .expect("permit-role override")
        .expect("override recorded");
    let account_override = record_account_work_order_override(&db, account, work_order, &input)
        .await
        .expect("account override");

    for (override_id, target_type, target_id, personnel_id, user_id) in [
        (person_override, "permit_role", permit, Some(person), None),
        (account_override, "work_order", work_order, None, Some(account)),
    ] {
        let sync_id = scalar_opt_text(
            &db,
            "SELECT entity_sync_id FROM qualification_overrides WHERE id = ?",
            vec![override_id.into()],
        )
        .await
        .expect("override sync id");
        let of_event = |expr: &str| {
            format!(
                "SELECT {expr} FROM audit_events WHERE action_code = 'qualification.override' \
                 AND json_extract(details_json, '$.qualification_override.sync_id') = ?"
            )
        };
        assert_eq!(
            scalar_i64(&db, &of_event("COUNT(*)"), vec![sync_id.clone().into()]).await,
            1,
            "{target_type}: one audit names the override row by its sync id"
        );
        let text = |expr: &'static str| {
            let db = &db;
            let sql = of_event(expr);
            let sync_id = sync_id.clone();
            async move { scalar_opt_text(db, &sql, vec![sync_id.into()]).await }
        };
        assert_eq!(text("target_type").await.as_deref(), Some(target_type));
        assert_eq!(text("target_id").await, Some(target_id.to_string()));
        assert_eq!(
            text("CAST(json_extract(details_json, '$.qualification_override.personnel_id') AS TEXT)").await,
            personnel_id.map(|v| v.to_string()),
            "{target_type}: the personnel record"
        );
        assert_eq!(
            text("CAST(json_extract(details_json, '$.qualification_override.user_id') AS TEXT)").await,
            user_id.map(|v| v.to_string()),
            "{target_type}: the login"
        );
        assert_eq!(
            text(
                "CAST((json_type(details_json, '$.override_id') IS NOT NULL) \
                 + (json_type(details_json, '$.personnel_id') IS NOT NULL) \
                 + (json_type(details_json, '$.user_id') IS NOT NULL) AS TEXT)"
            )
            .await
            .as_deref(),
            Some("0"),
            "{target_type}: no local id left at the top level"
        );
        let hash = text("chain_hash").await.expect("sealed");
        assert!(!hash.starts_with('L'), "{target_type}: sealed in its sync form: {hash}");
    }
    assert!(verify_audit_chain(&db).await.expect("verify").intact);
}
