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
    // Plant FMECA rating grids (severity / occurrence / detection, criticality matrix).
    SettingDefinition {
        key: crate::reliability::study::scales::SETTING_KEY,
        category: "reliability",
        scope: "tenant",
        shipped_value_json: crate::reliability::study::scales::DEFAULT_RATING_SCALES_JSON,
        kind: SettingKind::ModuleManaged,
        editor: SettingEditor::Dedicated,
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

fn invalid(key: &str, message: impl Into<String>) -> AppError {
    AppError::ValidationFailed(vec![format!("{key}: {}", message.into())])
}

/// Check a value against its definition and return the canonical JSON to store.
pub fn validate_value(def: &SettingDefinition, value_json: &str) -> AppResult<String> {
    let value: serde_json::Value =
        serde_json::from_str(value_json.trim()).map_err(|_| invalid(def.key, "the value is not valid JSON"))?;
    match def.kind {
        SettingKind::Choice(options) => {
            let Some(s) = value.as_str() else {
                return Err(invalid(def.key, "expected one of the listed values"));
            };
            if !options.contains(&s) {
                return Err(invalid(def.key, format!("'{s}' is not one of: {}", options.join(", "))));
            }
            Ok(serde_json::to_string(s)?)
        }
        SettingKind::Integer { min, max } => {
            let n = integer_of(&value).ok_or_else(|| invalid(def.key, "expected a whole number"))?;
            if !(min..=max).contains(&n) {
                return Err(invalid(def.key, format!("must be between {min} and {max} (got {n})")));
            }
            Ok(n.to_string())
        }
        SettingKind::Month => {
            let n = integer_of(&value).ok_or_else(|| invalid(def.key, "expected a month number"))?;
            if !(1..=12).contains(&n) {
                return Err(invalid(def.key, format!("must be a month between 1 and 12 (got {n})")));
            }
            Ok(n.to_string())
        }
        SettingKind::CurrencyCode => {
            let Some(s) = value.as_str() else {
                return Err(invalid(def.key, "expected a currency code such as EUR"));
            };
            let code = crate::inventory::fx::normalize_currency_code(s)?;
            Ok(serde_json::to_string(&code)?)
        }
        SettingKind::DocumentHeader => {
            let Some(obj) = value.as_object() else {
                return Err(invalid(def.key, "expected a header object"));
            };
            if obj.get("version").and_then(serde_json::Value::as_i64) != Some(1) {
                return Err(invalid(def.key, "unsupported header version (expected 1)"));
            }
            let html = obj.get("html").and_then(serde_json::Value::as_str).unwrap_or("");
            if html.len() > 200_000 {
                return Err(invalid(def.key, "the header is too large (200 KB maximum)"));
            }
            Ok(serde_json::to_string(&value)?)
        }
        SettingKind::ModuleManaged => Err(invalid(
            def.key,
            match def.editor {
                SettingEditor::NotInUse => "this setting is not used by the application and cannot be changed",
                _ => "this setting is changed from its own screen",
            },
        )),
    }
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
    /// `choice` | `integer` | `currency` | `month` | `document_header` | `module`
    pub kind: String,
    pub options: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
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

fn kind_view(kind: SettingKind) -> (&'static str, Vec<String>, Option<i64>, Option<i64>) {
    match kind {
        SettingKind::Choice(options) => ("choice", options.iter().map(|s| (*s).to_string()).collect(), None, None),
        SettingKind::Integer { min, max } => ("integer", Vec::new(), Some(min), Some(max)),
        SettingKind::CurrencyCode => ("currency", Vec::new(), None, None),
        SettingKind::Month => ("month", Vec::new(), Some(1), Some(12)),
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

        let daily = definition("backup.retention_daily").expect("defined");
        assert!(validate_value(daily, "0").is_err(), "at least one daily backup is kept");

        let unused = definition("appearance.density").expect("defined");
        assert!(
            validate_value(unused, "\"compact\"").is_err(),
            "unused settings are not writable"
        );
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
