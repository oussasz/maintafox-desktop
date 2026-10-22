//! Every audit target type the application writes is either sent by the sync codec (mapped in the
//! `audit_events` target mapping of `sync::row_spec`) or kept on its device on purpose (listed in
//! that entity's row filter). Otherwise its events are sealed in their local form and never leave
//! the PC, without anyone deciding so (`audit::chain::seal` logs it as an error): this test makes a
//! new unmapped type fail CI instead.
//!
//! The written types are read from the source: the `target_type` / `entity_type` literal of every
//! `AuditEventInput { … }` / `AuditEvent { … }` built outside tests, plus the types set at run time,
//! listed below with where they come from.

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
];

/// Types kept on their device by an action-code term of the row filter (not by their type).
const KEPT_BY_ACTION: &[(&str, &str)] = &[("notification", "notification.safety_notice_acknowledged")];

/// Known gaps, reported to their domain leads (not decided here): written with a target the codec
/// cannot encode, so their events stay on the PC that wrote them (stub only) and `seal` logs an
/// error. Each must be mapped (if the target travels) or added to the row filter (if it is kept on
/// purpose), then removed from this list.
const KNOWN_GAPS: &[(&str, &str)] = &[
    ("work_order_type", "wo::types — work-order type changes"),
    ("qualification_requirement", "qualification::requirements"),
    ("personnel_availability_block", "personnel::availability"),
    ("permit_role", "qualification::gate — override in a permit role"),
];

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

/// Literal target types of the audit events built in the application's (non-test) source.
fn written_literal_types() -> BTreeSet<String> {
    let construct = regex::Regex::new(r"\bAuditEvent(?:Input)?\s*\{").expect("regex");
    let field = regex::Regex::new(r#"\b(?:target_type|entity_type):\s*Some\(\s*"([a-z_]+)""#).expect("regex");
    let test_module = regex::Regex::new(r"#\[cfg\(test\)\]\r?\nmod ").expect("regex");
    let mut files = Vec::new();
    source_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut types = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read source");
        // Inline test modules sit at the end of their file.
        let text = test_module.split(&text).next().unwrap_or_default();
        for m in construct.find_iter(text) {
            if text[..m.start()].ends_with("struct ") {
                continue;
            }
            let window: String = text[m.end()..].chars().take(1200).collect();
            // The first field of this construction (before the next construction starts).
            let window = construct.split(&window).next().unwrap_or_default();
            if let Some(c) = field.captures(window) {
                types.insert(c[1].to_string());
            }
        }
    }
    types
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
    let literal = written_literal_types();
    assert!(
        literal.contains("user_account") && literal.contains("stock_location_article"),
        "the source scan finds the audit constructions: {literal:?}"
    );
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
