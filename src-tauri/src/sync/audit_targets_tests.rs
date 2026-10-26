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
const KNOWN_GAPS: &[(&str, &str)] = &[(
    "permit_role",
    "qualification::gate — override in a permit role; routed by the Coordinator, pending",
)];

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
