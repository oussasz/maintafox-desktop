//! Catalog of the tenant/device settings stored in `app_settings`: one definition per key with
//! its shipped value ("Valeur livrée"), the kind of value it holds and how it is edited.
//!
//! The seeder writes the shipped values from here (missing rows only), the Settings screen lists
//! the editable definitions from here, and `set_setting` (IPC) accepts only keys defined here,
//! validated against their kind. Rows written by modules through their own commands (rating
//! scales, notification and licence state) are not edited through the generic setting command.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// What a setting holds, and so how it is validated and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// A JSON string, one of the listed values.
    Choice(&'static [&'static str]),
    /// A JSON integer within `min..=max`.
    Integer { min: i64, max: i64 },
    /// A JSON number within `min..=max`, kept to 6 decimals. The bounds are in millionths
    /// (`1_000_000` = 1) so the catalog stays exact and comparable.
    Decimal { min_millionths: i64, max_millionths: i64 },
    /// A [`SettingKind::Decimal`] that may be blank: never seeded (no row = blank, never read as
    /// zero). Its shipped value is JSON `null`; writing `null` deletes the row.
    OptionalDecimal { min_millionths: i64, max_millionths: i64 },
    /// An ISO 4217 code (three letters) as a JSON string.
    CurrencyCode,
    /// A month 1..=12 as a JSON integer.
    Month,
    /// The printed document header (JSON object), edited by its own panel.
    DocumentHeader,
    /// Edited by a dedicated screen of its module (not by the generic setting command).
    ModuleManaged,
}

/// How a setting is presented in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEditor {
    /// Listed and edited in its Settings section.
    Section,
    /// Edited by a dedicated panel (document header) or module screen.
    Dedicated,
    /// Stored and seeded, but nothing in the application reads it: not shown, not writable.
    NotInUse,
}

#[derive(Debug, Clone, Copy)]
pub struct SettingDefinition {
    pub key: &'static str,
    /// Settings section the key belongs to.
    pub category: &'static str,
    pub scope: &'static str,
    /// Value shipped with the application (written when the row is missing).
    pub shipped_value_json: &'static str,
    pub kind: SettingKind,
    pub editor: SettingEditor,
    /// `high` settings need a fresh step-up to change.
    pub risk: &'static str,
}

const LANGUAGES: &[&str] = &["fr", "en"];

/// Shortest reason accepted for a high-risk setting change or a site rule change (it is the audit
/// trail's "why").
pub const MIN_REASON_CHARS: usize = 10;

/// Every setting row the application ships. Order = display order inside a section.
pub const SETTINGS_CATALOG: &[SettingDefinition] = &[
    // ── Localisation & finance calendar ────────────────────────────────────────────────────
    SettingDefinition {
        key: "locale.primary_language",
        category: "localization",
        scope: "tenant",
        shipped_value_json: r#""fr""#,
        kind: SettingKind::Choice(LANGUAGES),
        editor: SettingEditor::Section,
        risk: "low",
    },
    SettingDefinition {
        key: "locale.fallback_language",
        category: "localization",
        scope: "tenant",
        shipped_value_json: r#""en""#,
        kind: SettingKind::Choice(LANGUAGES),
        editor: SettingEditor::Section,
        risk: "low",
    },
    SettingDefinition {
        key: crate::inventory::fx::CURRENCY_SETTING,
        category: "localization",
        scope: "tenant",
        shipped_value_json: r#""DZD""#,
        kind: SettingKind::CurrencyCode,
        editor: SettingEditor::Section,
        risk: "low",
    },
    // Month the company's fiscal year starts in (budget periods, settlement, period close).
    SettingDefinition {
        key: crate::finance::fiscal::FISCAL_START_SETTING,
        category: "localization",
        scope: "tenant",
        shipped_value_json: r"1",
        kind: SettingKind::Month,
        editor: SettingEditor::Section,
        risk: "low",
    },
    // ── Work orders ─────────────────────────────────────────────────────────────────────────
    // PRD §6.5: a closed WO may be reopened within this many days of its closure.
    SettingDefinition {
        key: crate::wo::closeout::SETTING_REOPEN_WINDOW_DAYS,
        category: "work_orders",
        scope: "tenant",
        shipped_value_json: r"30",
        kind: SettingKind::Integer { min: 0, max: 3650 },
        editor: SettingEditor::Section,
        risk: "low",
    },
    // ── Inventory ───────────────────────────────────────────────────────────────────────────
    // Four-eyes tolerance of a stock count variance (D11, migration 259): above it ("critique"), the
    // variance is approved by someone other than the counter. Above = any threshold set is exceeded
    // (`>`); both blank = every variance (`inventory::count_four_eyes`). Not seeded: blank until set.
    SettingDefinition {
        key: crate::inventory::count_four_eyes::SETTING_VALUE_THRESHOLD,
        category: "inventory",
        scope: "tenant",
        shipped_value_json: "null",
        kind: SettingKind::OptionalDecimal {
            min_millionths: 0,
            max_millionths: 1_000_000_000_000_000,
        },
        editor: SettingEditor::Section,
        risk: "low",
    },
    SettingDefinition {
        key: crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD,
        category: "inventory",
        scope: "tenant",
        shipped_value_json: "null",
        kind: SettingKind::OptionalDecimal {
            min_millionths: 0,
            max_millionths: 1_000_000_000_000,
        },
        editor: SettingEditor::Section,
        risk: "low",
    },
    // Stock reconciliation: a gap between the stock ledger and the stored balance that is at
    // least this much (in the article's unit) is a break. Shipped 0.01, the value the
    // reconciliation screen used (migration 232). Above zero: a zero tolerance would make every
    // line a break.
    SettingDefinition {
        key: crate::inventory::controls::SETTING_RECONCILIATION_DRIFT_BREAK,
        category: "inventory",
        scope: "tenant",
        shipped_value_json: r"0.01",
        kind: SettingKind::Decimal {
            min_millionths: 1,
            max_millionths: 1_000_000_000,
        },
        editor: SettingEditor::Section,
        risk: "low",
    },
    // ── Reliability ─────────────────────────────────────────────────────────────────────────
    SettingDefinition {
        key: "ram.fmeca_rpn_critical_threshold",
        category: "reliability",
        scope: "tenant",
        shipped_value_json: r"150",
        // RPN = severity × occurrence × detection, each 1..10.
        kind: SettingKind::Integer { min: 1, max: 1000 },
        editor: SettingEditor::Section,
        risk: "low",
    },
    // Former home of the FMECA rating grids: read once to seed Données de référence
    // (`reliability::study::scales::ensure_fmeca_rating_scales`), then no longer used.
    SettingDefinition {
        key: crate::reliability::study::scales::SETTING_KEY,
        category: "reliability",
        scope: "tenant",
        shipped_value_json: crate::reliability::study::scales::DEFAULT_RATING_SCALES_JSON,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    // ── Backups (grandfather-father-son retention of scheduled backups) ──────────────────────
    SettingDefinition {
        key: "backup.retention_daily",
        category: "backup",
        scope: "tenant",
        shipped_value_json: r"7",
        // At least one daily backup is kept: 0 everywhere would prune every scheduled backup.
        kind: SettingKind::Integer { min: 1, max: 366 },
        editor: SettingEditor::Section,
        risk: "low",
    },
    SettingDefinition {
        key: "backup.retention_weekly",
        category: "backup",
        scope: "tenant",
        shipped_value_json: r"4",
        kind: SettingKind::Integer { min: 0, max: 260 },
        editor: SettingEditor::Section,
        risk: "low",
    },
    SettingDefinition {
        key: "backup.retention_monthly",
        category: "backup",
        scope: "tenant",
        shipped_value_json: r"12",
        kind: SettingKind::Integer { min: 0, max: 120 },
        editor: SettingEditor::Section,
        risk: "low",
    },
    // ── Printed documents ───────────────────────────────────────────────────────────────────
    SettingDefinition {
        key: super::branding::DOCUMENT_HEADER_SETTING_KEY,
        category: "documents",
        scope: "tenant",
        shipped_value_json: r#"{"version":1,"content_json":{"type":"doc","content":[{"type":"paragraph"}]},"html":"","logo_relative_path":null}"#,
        kind: SettingKind::DocumentHeader,
        editor: SettingEditor::Dedicated,
        risk: "low",
    },
    // ── Shipped but read by nothing (kept so existing rows stay valid; not offered) ─────────
    // Dates and numbers follow the interface language; there is no theme, density, text size,
    // update channel or log retention reader.
    SettingDefinition {
        key: "locale.date_format",
        category: "localization",
        scope: "tenant",
        shipped_value_json: r#""DD/MM/YYYY""#,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "locale.number_format",
        category: "localization",
        scope: "tenant",
        shipped_value_json: r#""fr-FR""#,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "locale.week_start_day",
        category: "localization",
        scope: "tenant",
        shipped_value_json: r"1",
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "appearance.color_mode",
        category: "appearance",
        scope: "tenant",
        shipped_value_json: r#""light""#,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "appearance.density",
        category: "appearance",
        scope: "tenant",
        shipped_value_json: r#""standard""#,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "appearance.text_scale",
        category: "appearance",
        scope: "tenant",
        shipped_value_json: r"1.0",
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "updater.release_channel",
        category: "system",
        scope: "device",
        shipped_value_json: r#""stable""#,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "updater.auto_check",
        category: "system",
        scope: "device",
        shipped_value_json: r"true",
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
    SettingDefinition {
        key: "diagnostics.log_retention_days",
        category: "system",
        scope: "device",
        shipped_value_json: r"30",
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::NotInUse,
        risk: "low",
    },
];

pub fn definition(key: &str) -> Option<&'static SettingDefinition> {
    SETTINGS_CATALOG.iter().find(|d| d.key == key)
}

/// Plant parameters removed from the catalog whose change history keeps its values (the last value
/// was written there when the key was retired): `inventory.count_critical_variance`, replaced by
/// the four-eyes tolerance (migration 259).
pub const RETIRED_PLANT_PARAMETERS: &[&str] = &["inventory.count_critical_variance"];

/// The seeder writes this setting's shipped value when its row is missing; an optional setting is
/// never seeded (no row = blank).
pub fn is_seeded(def: &SettingDefinition) -> bool {
    !matches!(def.kind, SettingKind::OptionalDecimal { .. })
}

/// Plant policy domains (`policy_snapshots`) whose change history keeps the snapshot values.
pub const POLICY_DOMAINS_WITH_VALUE_HISTORY: &[&str] = &["session", "backup"];

/// The change history may keep the old / new value of this key: plant parameters edited in
/// Settings and the plant policies. Everything else (licence onboarding with its activation
/// token and machine fingerprint, bootstrap state, module-managed rows, unused rows) is recorded
/// by hash only — the history is synced to every device and readable by settings administrators.
pub fn keeps_value_history(key_or_domain: &str) -> bool {
    POLICY_DOMAINS_WITH_VALUE_HISTORY.contains(&key_or_domain)
        || RETIRED_PLANT_PARAMETERS.contains(&key_or_domain)
        || key_or_domain.starts_with(RBAC_HISTORY_PREFIX)
        || is_plant_parameter(key_or_domain)
}

/// History key prefix of the security settings (`rbac_settings`: password policy, lockout).
pub const RBAC_HISTORY_PREFIX: &str = "rbac.";

/// A catalog key edited in Settings (a section or the document header panel).
pub fn is_plant_parameter(key: &str) -> bool {
    definition(key).is_some_and(|d| d.kind != SettingKind::ModuleManaged && d.editor != SettingEditor::NotInUse)
}

/// Tags the printed document header may contain (same list as the editor's sanitizer,
/// `src/lib/document-header-sanitize.ts`).
const HEADER_TAGS: &[&str] = &[
    "p", "br", "strong", "b", "em", "i", "u", "s", "h1", "h2", "h3", "ul", "ol", "li", "span", "div", "img", "table",
    "thead", "tbody", "tr", "th", "td",
];
const HEADER_ATTRIBUTES: &[&str] = &[
    "class", "style", "src", "alt", "width", "height", "align", "colspan", "rowspan",
];

/// Check the header HTML against the editor's allowlist: only listed tags and attributes, no
/// comments, no script URLs or CSS expressions. The editor sanitizes before saving, so a header
/// that fails here did not come from it.
pub fn validate_header_html(html: &str) -> Result<(), String> {
    let lower = html.to_ascii_lowercase();
    for forbidden in ["<!--", "javascript:", "vbscript:", "expression(", "<![cdata["] {
        if lower.contains(forbidden) {
            return Err(format!("the header contains '{forbidden}', which is not allowed"));
        }
    }
    let bytes = lower.as_bytes();
    let mut i = 0;
    while let Some(offset) = lower[i..].find('<') {
        let start = i + offset + 1;
        let mut j = start;
        if bytes.get(j) == Some(&b'/') {
            j += 1;
        }
        let name_start = j;
        while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
            j += 1;
        }
        let name = &lower[name_start..j];
        if name.is_empty() || !HEADER_TAGS.contains(&name) {
            return Err(format!("the tag <{name}> is not allowed in the header"));
        }
        // Attributes until the closing '>'.
        loop {
            while j < bytes.len() && (bytes[j].is_ascii_whitespace() || bytes[j] == b'/') {
                j += 1;
            }
            match bytes.get(j) {
                None => return Err("the header has an unclosed tag".into()),
                Some(b'>') => break,
                _ => {}
            }
            let attr_start = j;
            while j < bytes.len() && !bytes[j].is_ascii_whitespace() && !matches!(bytes[j], b'=' | b'>' | b'/') {
                j += 1;
            }
            let attr = &lower[attr_start..j];
            if !HEADER_ATTRIBUTES.contains(&attr) {
                return Err(format!("the attribute '{attr}' is not allowed in the header"));
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b'=') {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                match bytes.get(j) {
                    Some(&q) if q == b'"' || q == b'\'' => {
                        let Some(end) = lower[j + 1..].find(q as char) else {
                            return Err("the header has an unclosed attribute value".into());
                        };
                        j += end + 2;
                    }
                    _ => {
                        while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' {
                            j += 1;
                        }
                    }
                }
            }
        }
        i = j + 1;
    }
    Ok(())
}

/// A refused setting value: `GATE_SET_<RULE>:key=<setting key>: <key>: <english detail>`.
fn invalid(key: &str, code: &str, message: impl Into<String>) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}:key={key}: {key}: {}", message.into())])
}

/// Same refusal with the offending numeric value and the allowed bounds as parameters.
fn invalid_range(key: &str, code: &str, min: f64, max: f64, got: f64, message: String) -> AppError {
    AppError::ValidationFailed(vec![format!(
        "{code}:key={key}:min={min}:max={max}:value={got}: {key}: {message}"
    )])
}

/// Check a value against its definition and return the canonical JSON to store.
pub fn validate_value(def: &SettingDefinition, value_json: &str) -> AppResult<String> {
    let value: serde_json::Value = serde_json::from_str(value_json.trim())
        .map_err(|_| invalid(def.key, "GATE_SET_VALUE_JSON_INVALID", "the value is not valid JSON"))?;
    match def.kind {
        SettingKind::Choice(options) => {
            let Some(s) = value.as_str() else {
                return Err(invalid(
                    def.key,
                    "GATE_SET_VALUE_NOT_ALLOWED",
                    "expected one of the listed values",
                ));
            };
            if !options.contains(&s) {
                return Err(invalid(
                    def.key,
                    "GATE_SET_VALUE_NOT_ALLOWED",
                    format!("'{s}' is not one of: {}", options.join(", ")),
                ));
            }
            Ok(serde_json::to_string(s)?)
        }
        SettingKind::Integer { min, max } => {
            let n = integer_of(&value)
                .ok_or_else(|| invalid(def.key, "GATE_SET_VALUE_NOT_INTEGER", "expected a whole number"))?;
            if !(min..=max).contains(&n) {
                return Err(invalid_range(
                    def.key,
                    "GATE_SET_VALUE_OUT_OF_RANGE",
                    min as f64,
                    max as f64,
                    n as f64,
                    format!("must be between {min} and {max} (got {n})"),
                ));
            }
            Ok(n.to_string())
        }
        SettingKind::Decimal {
            min_millionths,
            max_millionths,
        } => bounded_decimal(def.key, &value, min_millionths, max_millionths),
        SettingKind::OptionalDecimal {
            min_millionths,
            max_millionths,
        } => {
            // Blank: JSON null or an empty string.
            if value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty()) {
                return Ok("null".to_string());
            }
            bounded_decimal(def.key, &value, min_millionths, max_millionths)
        }
        SettingKind::Month => {
            let n = integer_of(&value)
                .ok_or_else(|| invalid(def.key, "GATE_SET_VALUE_NOT_INTEGER", "expected a month number"))?;
            if !(1..=12).contains(&n) {
                return Err(invalid(
                    def.key,
                    "GATE_SET_MONTH_RANGE",
                    format!("must be a month between 1 and 12 (got {n})"),
                ));
            }
            Ok(n.to_string())
        }
        SettingKind::CurrencyCode => {
            let Some(s) = value.as_str() else {
                return Err(invalid(
                    def.key,
                    "GATE_SET_CURRENCY_INVALID",
                    "expected a currency code such as EUR",
                ));
            };
            let code = crate::inventory::fx::normalize_currency_code(s)?;
            Ok(serde_json::to_string(&code)?)
        }
        SettingKind::DocumentHeader => {
            let Some(obj) = value.as_object() else {
                return Err(invalid(def.key, "GATE_SET_HEADER_INVALID", "expected a header object"));
            };
            if obj.get("version").and_then(serde_json::Value::as_i64) != Some(1) {
                return Err(invalid(
                    def.key,
                    "GATE_SET_HEADER_INVALID",
                    "unsupported header version (expected 1)",
                ));
            }
            let html = obj.get("html").and_then(serde_json::Value::as_str).unwrap_or("");
            if html.len() > 200_000 {
                return Err(invalid(
                    def.key,
                    "GATE_SET_HEADER_INVALID",
                    "the header is too large (200 KB maximum)",
                ));
            }
            validate_header_html(html).map_err(|m| invalid(def.key, "GATE_SET_HEADER_INVALID", m))?;
            Ok(serde_json::to_string(&value)?)
        }
        SettingKind::ModuleManaged => Err(invalid(
            def.key,
            match def.editor {
                SettingEditor::NotInUse => "GATE_SET_NOT_IN_USE",
                _ => "GATE_SET_OWN_SCREEN",
            },
            match def.editor {
                SettingEditor::NotInUse => "this setting is not used by the application and cannot be changed",
                _ => "this setting is changed from its own screen",
            },
        )),
    }
}

/// A decimal within `min..=max` (millionths), kept to 6 decimals, as canonical JSON.
fn bounded_decimal(
    key: &str,
    value: &serde_json::Value,
    min_millionths: i64,
    max_millionths: i64,
) -> AppResult<String> {
    let n = decimal_of(value).ok_or_else(|| invalid(key, "GATE_SET_VALUE_NOT_NUMBER", "expected a number"))?;
    let millionths = (n * 1_000_000.0).round();
    #[allow(clippy::cast_precision_loss)]
    let (lo, hi) = (min_millionths as f64, max_millionths as f64);
    if !(lo..=hi).contains(&millionths) {
        return Err(invalid_range(
            key,
            "GATE_SET_VALUE_OUT_OF_RANGE",
            lo / 1_000_000.0,
            hi / 1_000_000.0,
            n,
            format!(
                "must be between {} and {} (got {n})",
                lo / 1_000_000.0,
                hi / 1_000_000.0
            ),
        ));
    }
    Ok(serde_json::to_string(&(millionths / 1_000_000.0))?)
}

/// A finite number given as a JSON number or a numeric string.
fn decimal_of(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|f: &f64| f.is_finite())
}

/// A decimal setting's value, checked against its catalog bounds. A missing or out-of-bounds
/// value is an error: nothing is computed on a guessed tolerance.
pub async fn decimal_setting<C: ConnectionTrait>(db: &C, key: &str) -> AppResult<f64> {
    let def = definition(key)
        .filter(|d| matches!(d.kind, SettingKind::Decimal { .. }))
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("{key}: not a decimal setting of the catalog")]))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = ? AND setting_scope = ?",
            [def.key.into(), def.scope.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("{key}: the setting is missing")]))?;
    let raw: String = row.try_get("", "setting_value_json")?;
    let canonical = validate_value(def, &raw)?;
    serde_json::from_str(&canonical).map_err(|_| invalid(key, "GATE_SET_VALUE_NOT_NUMBER", "expected a number"))
}

/// An optional decimal setting's value: `None` when blank (no row), never read as zero. A stored
/// value is checked against its catalog bounds (out of bounds is an error: nothing is computed on a
/// guessed tolerance).
pub async fn optional_decimal_setting<C: ConnectionTrait>(db: &C, key: &str) -> AppResult<Option<f64>> {
    let def = definition(key)
        .filter(|d| matches!(d.kind, SettingKind::OptionalDecimal { .. }))
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!("{key}: not an optional decimal setting of the catalog")])
        })?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = ? AND setting_scope = ?",
            [def.key.into(), def.scope.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    let raw: String = row.try_get("", "setting_value_json")?;
    let canonical = validate_value(def, &raw)?;
    serde_json::from_str::<Option<f64>>(&canonical)
        .map_err(|_| invalid(key, "GATE_SET_VALUE_NOT_NUMBER", "expected a number"))
}

fn integer_of(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Why a setting cannot change right now (stored data depends on its current value), if so.
pub async fn lock_reason<C: ConnectionTrait>(db: &C, key: &str) -> AppResult<Option<String>> {
    if key == crate::finance::fiscal::FISCAL_START_SETTING && crate::finance::fiscal::fiscal_start_in_use(db).await? {
        return Ok(Some(
            "GATE_FIN_FISCAL_START_LOCKED: budget actuals or closed periods already exist; changing the start \
             month would move every past posting to another month"
                .into(),
        ));
    }
    if key == crate::inventory::fx::CURRENCY_SETTING && crate::inventory::fx::tenant_currency_in_use(db).await? {
        return Ok(Some(
            "GATE_FIN_CURRENCY_LOCKED: stock, receipts or postings are already valued in the tenant currency; \
             changing it would relabel every stored amount"
                .into(),
        ));
    }
    Ok(None)
}

/// The shipped value differs from `current` (compared as JSON values, so `1` equals `1.0`).
pub fn differs_from_shipped(def: &SettingDefinition, current_json: &str) -> bool {
    let shipped: Option<serde_json::Value> = serde_json::from_str(def.shipped_value_json).ok();
    let current: Option<serde_json::Value> = serde_json::from_str(current_json).ok();
    match (shipped, current) {
        (Some(s), Some(c)) => {
            if let (Some(a), Some(b)) = (s.as_f64(), c.as_f64()) {
                (a - b).abs() > f64::EPSILON
            } else {
                s != c
            }
        }
        _ => def.shipped_value_json.trim() != current_json.trim(),
    }
}

/// Role names that may change settings (hold `adm.settings` through an active role).
pub async fn roles_allowed_to_change<C: ConnectionTrait>(db: &C) -> AppResult<Vec<String>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT r.name FROM roles r \
             INNER JOIN role_permissions rp ON rp.role_id = r.id \
             INNER JOIN permissions p ON p.id = rp.permission_id \
             WHERE p.name = ? AND r.deleted_at IS NULL AND r.status != 'retired' \
             ORDER BY r.name",
            [crate::rbac::permissions::ADM_SETTINGS.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect())
}

/// One editable setting as shown in Settings.
#[derive(Debug, Clone, Serialize)]
pub struct SettingDefinitionView {
    pub key: String,
    pub category: String,
    pub scope: String,
    /// `choice` | `integer` | `decimal` | `optional_decimal` | `currency` | `month` |
    /// `document_header` | `module`
    pub kind: String,
    pub options: Vec<String>,
    /// Bounds of an `integer`, `decimal` or `month` value.
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub shipped_value_json: String,
    pub current_value_json: Option<String>,
    pub differs_from_shipped: bool,
    pub editor: String,
    pub risk: String,
    /// Permission needed to change it.
    pub required_permission: String,
    pub locked_reason: Option<String>,
    pub last_modified_at: Option<String>,
    pub last_modified_by: Option<String>,
}

/// Settings + who may change them, for the Settings screen.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsOverview {
    pub definitions: Vec<SettingDefinitionView>,
    pub roles_allowed_to_change: Vec<String>,
}

#[allow(clippy::cast_precision_loss)]
fn kind_view(kind: SettingKind) -> (&'static str, Vec<String>, Option<f64>, Option<f64>) {
    match kind {
        SettingKind::Choice(options) => ("choice", options.iter().map(|s| (*s).to_string()).collect(), None, None),
        SettingKind::Integer { min, max } => ("integer", Vec::new(), Some(min as f64), Some(max as f64)),
        SettingKind::Decimal {
            min_millionths,
            max_millionths,
        } => (
            "decimal",
            Vec::new(),
            Some(min_millionths as f64 / 1_000_000.0),
            Some(max_millionths as f64 / 1_000_000.0),
        ),
        SettingKind::OptionalDecimal {
            min_millionths,
            max_millionths,
        } => (
            "optional_decimal",
            Vec::new(),
            Some(min_millionths as f64 / 1_000_000.0),
            Some(max_millionths as f64 / 1_000_000.0),
        ),
        SettingKind::CurrencyCode => ("currency", Vec::new(), None, None),
        SettingKind::Month => ("month", Vec::new(), Some(1.0), Some(12.0)),
        SettingKind::DocumentHeader => ("document_header", Vec::new(), None, None),
        SettingKind::ModuleManaged => ("module", Vec::new(), None, None),
    }
}

/// Every setting that is in use (editable in a section or by a dedicated panel), with its
/// current and shipped values, lock state and last change.
pub async fn overview(db: &sea_orm::DatabaseConnection) -> AppResult<SettingsOverview> {
    let mut definitions = Vec::new();
    for def in SETTINGS_CATALOG.iter().filter(|d| d.editor != SettingEditor::NotInUse) {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT s.setting_value_json, \
                        COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', s.last_modified_at), s.last_modified_at) AS at, \
                        COALESCE(u.display_name, u.username) AS by_name \
                 FROM app_settings s LEFT JOIN user_accounts u ON u.id = s.last_modified_by_id \
                 WHERE s.setting_key = ? AND s.setting_scope = ?",
                [def.key.into(), def.scope.into()],
            ))
            .await?;
        let (current, at, by) = match row {
            Some(r) => (
                Some(r.try_get::<String>("", "setting_value_json")?),
                r.try_get::<Option<String>>("", "at")?,
                r.try_get::<Option<String>>("", "by_name")?,
            ),
            None => (None, None, None),
        };
        let (kind, options, min, max) = kind_view(def.kind);
        definitions.push(SettingDefinitionView {
            key: def.key.to_string(),
            category: def.category.to_string(),
            scope: def.scope.to_string(),
            kind: kind.to_string(),
            options,
            min,
            max,
            shipped_value_json: def.shipped_value_json.to_string(),
            differs_from_shipped: current.as_deref().is_some_and(|c| differs_from_shipped(def, c)),
            current_value_json: current,
            editor: match def.editor {
                SettingEditor::Section => "section",
                SettingEditor::Dedicated => "dedicated",
                SettingEditor::NotInUse => "not_in_use",
            }
            .to_string(),
            risk: def.risk.to_string(),
            required_permission: crate::rbac::permissions::ADM_SETTINGS.to_string(),
            locked_reason: lock_reason(db, def.key).await?,
            last_modified_at: at,
            last_modified_by: by,
        });
    }
    Ok(SettingsOverview {
        definitions,
        roles_allowed_to_change: roles_allowed_to_change(db).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keys_are_unique() {
        let mut keys: Vec<&str> = SETTINGS_CATALOG.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate setting key in the catalog");
    }

    #[test]
    fn shipped_values_pass_their_own_validation() {
        for def in SETTINGS_CATALOG.iter().filter(|d| d.kind != SettingKind::ModuleManaged) {
            validate_value(def, def.shipped_value_json)
                .unwrap_or_else(|e| panic!("shipped value of {} is invalid: {e}", def.key));
        }
    }

    #[test]
    fn values_are_validated_and_canonicalised() {
        let reopen = definition("wo.reopen_window_days").expect("defined");
        assert_eq!(validate_value(reopen, "45").unwrap(), "45");
        assert_eq!(validate_value(reopen, "\"45\"").unwrap(), "45");
        assert!(validate_value(reopen, "-1").is_err());
        assert!(validate_value(reopen, "1.5").is_err());

        let lang = definition("locale.primary_language").expect("defined");
        assert!(validate_value(lang, "\"de\"").is_err());
        assert_eq!(validate_value(lang, "\"en\"").unwrap(), "\"en\"");

        let currency = definition("locale.currency_code").expect("defined");
        assert_eq!(validate_value(currency, "\"eur\"").unwrap(), "\"EUR\"");
        assert!(validate_value(currency, "\"EURO\"").is_err());

        let month = definition("locale.fiscal_year_start_month").expect("defined");
        assert_eq!(validate_value(month, "\"7\"").unwrap(), "7");
        assert!(validate_value(month, "13").is_err());

        assert!(
            definition("inventory.count_critical_variance").is_none(),
            "retired (migration 259)"
        );
        let value = definition("inventory.count_four_eyes_value_threshold").expect("defined");
        assert_eq!(validate_value(value, "500").unwrap(), "500.0");
        assert_eq!(validate_value(value, "0").unwrap(), "0.0", "zero is a value, not blank");
        assert_eq!(validate_value(value, "null").unwrap(), "null", "blank");
        assert_eq!(validate_value(value, "\"\"").unwrap(), "null", "blank");
        assert!(validate_value(value, "-1").is_err(), "below the minimum");
        assert!(validate_value(value, "\"abc\"").is_err());
        let percent = definition("inventory.count_four_eyes_percent_threshold").expect("defined");
        assert_eq!(validate_value(percent, "\"2.5\"").unwrap(), "2.5");
        assert!(!is_seeded(percent) && !is_seeded(value), "never seeded");
        let drift = definition("inventory.reconciliation_drift_break").expect("defined");
        assert_eq!(validate_value(drift, "0.01").unwrap(), "0.01");
        assert_eq!(
            validate_value(drift, "0.0100004").unwrap(),
            "0.01",
            "kept to 6 decimals"
        );
        assert!(
            validate_value(drift, "0").is_err(),
            "zero would make every line a break"
        );
        assert!(validate_value(drift, "1001").is_err(), "above the maximum");

        let daily = definition("backup.retention_daily").expect("defined");
        assert!(validate_value(daily, "0").is_err(), "at least one daily backup is kept");

        let unused = definition("appearance.density").expect("defined");
        assert!(
            validate_value(unused, "\"compact\"").is_err(),
            "unused settings are not writable"
        );
    }

    #[test]
    fn value_history_is_kept_for_plant_parameters_only() {
        assert!(keeps_value_history("wo.reopen_window_days"));
        assert!(keeps_value_history("documents.default_header"));
        assert!(keeps_value_history("session"));
        assert!(!keeps_value_history("product.license_onboarding"));
        assert!(!keeps_value_history("appearance.density"), "not in use");
        assert!(
            keeps_value_history("inventory.count_critical_variance"),
            "retired plant parameter"
        );
        assert!(!keeps_value_history(crate::reliability::study::scales::SETTING_KEY));
    }

    #[test]
    fn header_html_follows_the_editor_allowlist() {
        assert!(validate_header_html(
            r#"<p class="x">ACME <strong>Oran</strong></p><img src="data:image/png;base64,AA" alt="logo">"#
        )
        .is_ok());
        assert!(validate_header_html("<table><tbody><tr><td colspan='2'>x</td></tr></tbody></table><br/>").is_ok());
        assert!(validate_header_html("<script>alert(1)</script>").is_err());
        assert!(validate_header_html(r#"<img src=x onerror="alert(1)">"#).is_err());
        assert!(validate_header_html(r#"<a href="javascript:alert(1)">x</a>"#).is_err());
        assert!(validate_header_html(r#"<p style="background:url(javascript:x)">x</p>"#).is_err());
        assert!(validate_header_html("<iframe src='x'></iframe>").is_err());
        assert!(
            validate_header_html("plain text, 3 < 4 is fine?").is_err(),
            "a bare '<' is refused"
        );
        assert!(validate_header_html("").is_ok());
    }

    #[test]
    fn shipped_comparison_ignores_number_formatting() {
        let scale = definition("appearance.text_scale").expect("defined");
        assert!(!differs_from_shipped(scale, "1"));
        let reopen = definition("wo.reopen_window_days").expect("defined");
        assert!(differs_from_shipped(reopen, "31"));
        assert!(!differs_from_shipped(reopen, "30"));
    }
}
