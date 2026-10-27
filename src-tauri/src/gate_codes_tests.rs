//! Every refusal a user can read is coded.
//!
//! A refusal built as `AppError::ValidationFailed(vec![...])` must start with a gate code
//! (`GATE_<MODULE>_<NAME>[:key=value]: sentence`): the screen shows the text written for that code in
//! `src/i18n/<locale>/errors.json` (`gateError`), in the user's language, instead of the raw
//! English sentence. This test reads the source (outside tests) and fails on a refusal whose first
//! string literal does not start with `GATE_`, unless it is listed in `ALLOWLIST` with its reason.
//!
//! A message that begins with a format placeholder (`{code}: ...`, `{GATE_X}: ...`) is coded at run
//! time and counts as coded.

use std::path::{Path, PathBuf};

/// Refusals that are deliberately not coded. `(file, literal prefix)`: a file entry ending with `/`
/// covers the whole folder, an empty prefix covers every refusal of the file. One reason per group.
const ALLOWLIST: &[(&str, &str)] = &[
    // Licence / activation / entitlements: only stored-data, parse and lineage faults stay uncoded.
    ("license/queries.rs", "Failed to decode license field"),
    ("license/security.rs", "Failed to decode licensing security field"),
    ("license/security.rs", "{field} must be RFC3339"),
    ("activation/queries.rs", "Failed to decode activation field"),
    ("activation/queries.rs", "Invalid policy_snapshot_json"),
    ("activation/queries.rs", "Invalid anchor_hashes_json"),
    ("activation/queries.rs", "offline_grace_until missing"),
    ("activation/queries.rs", "Invalid offline_grace_until"),
    ("entitlements/queries.rs", "Failed to decode entitlement field"),
    (
        "entitlements/queries.rs",
        "Failed to validate previous envelope lineage",
    ),
    ("entitlements/queries.rs", "previous_envelope_id must reference"),
    // Permit / lock requirement of a work order: queued with the permit module's lead.
    ("wo/workflow/actions/permit_requirement.rs", ""),
    // Internal parse / serialisation failures: the text is the library error, not a rule.
    ("", "layout_json: "),
    ("", "overview_kpis_json: "),
    ("", "details_json: "),
    ("", "cron: "),
    ("", "guardrails JSON: "),
    ("", "Invalid header JSON: "),
    ("", "job input json: "),
    ("", "work_order_statuses."),
    ("", "report subscriber id out of range"),
    // Internal: stored config / parse fault.
    ("backup/scheduler.rs", "{key} must be a non-negative integer"),
    ("settings/catalog.rs", "{key}: not a decimal setting"),
    ("settings/catalog.rs", "{key}: the setting is missing"),
    (
        "settings/catalog.rs",
        "{key}: not an optional decimal setting of the catalog",
    ),
    ("sync/row_codec.rs", "{table}: invalid payload_json"),
    // Machine code + payload, not yet translated by a screen text.
    ("commands/reliability_study.rs", "ADVANCED_TOOL_DISABLED"),
    ("commands/settings.rs", "SETTING_REASON_REQUIRED"),
    ("", "READINESS_DENY:"),
    ("", "FMECA_ITEM_REVIEW_REQUIRED"),
    ("", "ASSIGNEE_UNAVAILABLE"),
    // Demo / golden-data seeds: developer tools, never reachable from a normal screen.
    ("db/rams_presentation_seed.rs", ""),
    ("db/rams_sql_demo_seed.rs", ""),
    // Internal: corrupt stored data / technical, not a rule the user can act on.
    ("data_integrity/repair.rs", "segment_id_"),
    ("sync/media.rs", "media bytes must not be empty"),
    ("sync/media.rs", "media content hash mismatch"),
    // PDF export: fonts of the host machine, an environment fault rather than a user rule.
    ("", "Font {source}"),
    ("", "No embeddable TrueType font"),
];

/// Source files outside the test files (`tests.rs`, `*_tests.rs`, `test_support.rs`).
fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "migrations") {
                continue;
            }
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !(name.ends_with("tests.rs") || name.contains("_tests") || name == "test_support.rs") {
                out.push(path);
            }
        }
    }
}

/// The first string literal inside the `[...]` that starts `text` (the text starts right after the
/// `[`), unescaped for line continuations. `None` when the first element is not a literal
/// (a variable, a call): such a refusal forwards a message built elsewhere.
fn first_literal(text: &str) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let (mut depth, mut i) = (1usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let mut lit = String::new();
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        if bytes.get(i + 1) == Some(&b'\n') || bytes.get(i + 1) == Some(&b'\r') {
                            i += 1;
                            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                                i += 1;
                            }
                            continue;
                        }
                        lit.push('\\');
                        lit.push(*bytes.get(i + 1)? as char);
                        i += 2;
                        continue;
                    }
                    let ch_len = text[i..].chars().next()?.len_utf8();
                    lit.push_str(&text[i..i + ch_len]);
                    i += ch_len;
                }
                return Some((lit, i + 1));
            }
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return None;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether a message starting with a format placeholder is coded at run time: the placeholder is a
/// code (`{code}`, `{gate}`, `{head}`, a `{..GATE..}` constant, a constant of the same file whose
/// value is a gate code) or a bare `{}` whose first format argument (`rest` follows the literal) is
/// a gate constant.
fn placeholder_is_code(literal: &str, rest: &str, file: &str) -> bool {
    let Some(end) = literal.find('}') else {
        return false;
    };
    let name = &literal[1..end];
    match name {
        "code" | "gate" | "head" => true,
        "" => {
            let arg: String = rest
                .trim_start_matches(|c: char| c.is_whitespace() || c == ',')
                .chars()
                .take_while(|c| *c != ',' && *c != ')')
                .collect();
            arg.contains("GATE_") || arg.contains("_codes::")
        }
        n => n.contains("GATE") || gate_constant(file, n),
    }
}

/// Whether `name` is a string constant of `file` whose value is a gate code
/// (`const NAME: &str = "GATE_...";`).
fn gate_constant(file: &str, name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && regex::Regex::new(&format!(r#"const\s+{name}\s*:\s*&(?:'static\s+)?str\s*=\s*"GATE_"#))
            .expect("regex")
            .is_match(file)
}

/// Byte ranges of the inline `#[cfg(test)] mod x { ... }` bodies (braces matched, string and
/// character literals skipped), so production code after a test module is still scanned.
fn test_module_ranges(text: &str) -> Vec<(usize, usize)> {
    let header = regex::Regex::new(r"#\[cfg\(test\)\]\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+\w+\s*\{").expect("regex");
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for m in header.find_iter(text) {
        let (mut depth, mut i) = (1usize, m.end());
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
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        out.push((m.start(), i));
    }
    out
}

/// Index of the first `ALLOWLIST` entry that covers this refusal.
fn allowed_by(rel: &str, literal: &str) -> Option<usize> {
    ALLOWLIST
        .iter()
        .position(|(file, prefix)| rel.starts_with(file) && literal.starts_with(prefix))
}

#[test]
fn every_user_facing_refusal_is_gate_coded() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    let site = regex::Regex::new(r"ValidationFailed\(\s*vec!\s*\[").expect("regex");
    let files_scanned = files.len();
    assert!(files_scanned > 100, "the source scan found only {files_scanned} files");

    let mut offenders: Vec<String> = Vec::new();
    let mut used = vec![false; ALLOWLIST.len()];
    for path in files {
        let rel = path
            .strip_prefix(&src)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path).expect("read source");
        let skipped = test_module_ranges(&text);
        for m in site.find_iter(&text) {
            let at = m.start();
            if skipped.iter().any(|(from, to)| at >= *from && at <= *to) {
                continue;
            }
            let args = &text[m.end()..];
            let Some((literal, end)) = first_literal(args) else {
                continue;
            };
            if literal.starts_with("GATE_") {
                continue;
            }
            if literal.starts_with('{') && placeholder_is_code(&literal, &args[end..], &text) {
                continue;
            }
            if let Some(entry) = allowed_by(&rel, &literal) {
                used[entry] = true;
                continue;
            }
            let line = text[..at].matches('\n').count() + 1;
            let shown: String = literal.chars().take(80).collect();
            offenders.push(format!("  {rel}:{line}  \"{shown}\""));
        }
    }

    assert!(
        offenders.is_empty(),
        "{} refusal(s) shown to the user are not coded:\n{}\n\n\
         Start the message with a gate code, `GATE_<MODULE>_<NAME>[:key=value]: original sentence` \
         (parameters are numbers, ids or codes only, never free text), and add its French and \
         English text under `gateError` in src/i18n/fr/errors.json and src/i18n/en/errors.json. \
         A refusal that must stay uncoded (internal parse failure, machine code, developer seed) \
         goes in ALLOWLIST in src-tauri/src/gate_codes_tests.rs with its reason.",
        offenders.len(),
        offenders.join("\n")
    );

    // A refusal that got its code leaves a dead entry behind: the list must stay the minimum.
    let stale: Vec<String> = ALLOWLIST
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|((file, prefix), _)| format!("  (\"{file}\", \"{prefix}\")"))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWLIST entries that no longer cover any uncoded refusal (remove them):\n{}",
        stale.join("\n")
    );
}
