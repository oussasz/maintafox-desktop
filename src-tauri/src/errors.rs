use std::collections::HashMap;
use std::fmt;

use serde::Serialize;
use thiserror::Error;

/// Structured validation issue for IPC + i18n (org and shared use).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AppValidationIssue {
    pub code: String,
    /// `"error"` (blocking) or `"warning"` (informational).
    #[serde(default = "default_error_severity")]
    pub severity: String,
    /// English fallback when the client has no i18n key for `code`.
    pub message: String,
    pub related_id: Option<i64>,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

fn default_error_severity() -> String {
    "error".to_string()
}

impl AppValidationIssue {
    pub fn error(code: impl Into<String>, message: impl Into<String>, params: HashMap<String, String>) -> Self {
        Self {
            code: code.into(),
            severity: "error".to_string(),
            message: message.into(),
            related_id: None,
            params,
        }
    }

    pub fn error_with_related(
        code: impl Into<String>,
        message: impl Into<String>,
        related_id: Option<i64>,
        params: HashMap<String, String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: "error".to_string(),
            message: message.into(),
            related_id,
            params,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>, params: HashMap<String, String>) -> Self {
        Self {
            code: code.into(),
            severity: "warning".to_string(),
            message: message.into(),
            related_id: None,
            params,
        }
    }
}

fn format_validation_failed_msgs(msgs: &[String]) -> String {
    if msgs.is_empty() {
        "Validation failed".to_string()
    } else {
        format!("Validation failed: {}", msgs.join("; "))
    }
}

fn format_org_validation_failed(issues: &[AppValidationIssue]) -> String {
    let joined: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
    if joined.is_empty() {
        "Validation failed".to_string()
    } else {
        format!("Validation failed: {}", joined.join("; "))
    }
}

/// Unified application error type. All service and command functions return
/// `AppResult<T>` rather than mixing error types across the IPC boundary.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Tenant scope violation: {0}")]
    TenantScopeViolation(String),

    #[error("Session claim invalid: {0}")]
    SessionClaimInvalid(String),

    #[error("Record not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("{}", format_validation_failed_msgs(.0))]
    ValidationFailed(Vec<String>),

    /// Org-module structured validation (stable codes + params for FE i18n).
    #[error("{}", format_org_validation_failed(.0))]
    OrgValidationFailed(Vec<AppValidationIssue>),

    #[error("Sync error: {0}")]
    SyncError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Permission denied: action '{action}' on resource '{resource}'")]
    Permission { action: String, resource: String },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("License denied [{reason_code}]: {message}")]
    LicenseDenied { reason_code: String, message: String },

    #[error("Step-up verification required for this action")]
    StepUpRequired,

    #[error("Account locked until {until}")]
    AccountLocked { until: String },

    /// Idle-locked session: user must unlock, not re-authenticate as if session were gone.
    #[error("Session locked: {0}")]
    SessionLocked(String),

    /// PIN unlock is disabled for this session (too many wrong PINs): unlock with the password.
    /// Who may unlock is unchanged; the code lets the lock screen switch to password mode.
    #[error("PIN désactivé, utilisez le mot de passe.")]
    PinDisabled,

    /// The account must change its password before any other command (server-enforced).
    #[error("{0}")]
    PasswordChangeRequired(String),

    /// Encrypted database cannot be opened: key missing from the OS credential store or wrong.
    /// The UI offers recovery-key entry (`DATABASE_KEY_UNAVAILABLE`).
    #[error("Database key unavailable: {0}")]
    DatabaseKeyUnavailable(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Build a single-issue org validation error.
pub fn org_validation_failed(code: &str, message: impl Into<String>, params: HashMap<String, String>) -> AppError {
    AppError::OrgValidationFailed(vec![AppValidationIssue::error(code, message, params)])
}

/// Build a multi-issue org validation error.
pub fn org_validation_failed_issues(issues: Vec<AppValidationIssue>) -> AppError {
    AppError::OrgValidationFailed(issues)
}

/// Convenience for building params from `&[(&str, String)]`.
pub fn issue_params(pairs: &[(&str, String)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// Serialize `AppError` to JSON for the Tauri IPC boundary.
/// Frontend receives: `{ "code": "NOT_FOUND", "message": "...", "details": null | [...] }`
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 3)?;
        let code = match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::Auth(_) => "AUTH_ERROR",
            Self::TenantScopeViolation(_) => "TENANT_SCOPE_VIOLATION",
            Self::SessionClaimInvalid(_) => "SESSION_CLAIM_INVALID",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::ValidationFailed(_) | Self::OrgValidationFailed(_) => "VALIDATION_FAILED",
            Self::SyncError(_) => "SYNC_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Permission { .. } => "PERMISSION_DENIED",
            Self::PermissionDenied(_) => "PERMISSION_DENIED",
            Self::LicenseDenied { .. } => "LICENSE_DENIED",
            Self::StepUpRequired => "STEP_UP_REQUIRED",
            Self::DatabaseKeyUnavailable(_) => "DATABASE_KEY_UNAVAILABLE",
            Self::AccountLocked { .. } => "ACCOUNT_LOCKED",
            Self::SessionLocked(_) => "SESSION_LOCKED",
            Self::PinDisabled => "PIN_DISABLED",
            Self::PasswordChangeRequired(_) => "PASSWORD_CHANGE_REQUIRED",
            Self::Internal(_) => "INTERNAL_ERROR",
        };
        state.serialize_field("code", code)?;

        // Desktop app: surface actionable detail for Internal (DB mapping, FK chains, etc.).
        // Cap length to avoid huge payloads; other variants already use self.to_string().
        const INTERNAL_MSG_MAX: usize = 800;
        let message = match self {
            Self::Internal(err) => {
                let detail = format!("{err:#}");
                let mut s = format!("Erreur interne : {detail}");
                if s.len() > INTERNAL_MSG_MAX {
                    s.truncate(INTERNAL_MSG_MAX);
                    s.push('…');
                }
                s
            }
            Self::ValidationFailed(msgs) => format_validation_failed_msgs(msgs),
            Self::OrgValidationFailed(issues) => format_org_validation_failed(issues),
            _ => self.to_string(),
        };
        state.serialize_field("message", &message)?;

        let details: Option<serde_json::Value> = match self {
            Self::ValidationFailed(msgs) => Some(serde_json::Value::Array(
                msgs.iter().map(|m| validation_detail(m)).collect(),
            )),
            Self::OrgValidationFailed(issues) => Some(serde_json::to_value(issues).unwrap_or(serde_json::Value::Null)),
            _ => None,
        };
        state.serialize_field("details", &details)?;
        state.end()
    }
}

/// A refusal sentence that starts with a stable rule code is sent with that code and its
/// parameters, so the screen translates it from the code and never shows the sentence itself.
///
/// Forms read: `CODE`, `CODE: sentence`, `CODE:key=value,key2=value2` and
/// `CODE:key=value: sentence`. A code is upper-case words joined by underscores (`GATE_FIN_X`,
/// `STUDY_STALE`); anything else is an uncoded sentence and has no code.
pub(crate) fn split_coded_refusal(raw: &str) -> Option<(String, HashMap<String, String>)> {
    let raw = raw.trim();
    let code_end = raw.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(raw.len());
    let code = &raw[..code_end];
    let is_word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let valid =
        code.contains('_') && code.starts_with(|c: char| c.is_ascii_uppercase()) && code.split('_').all(is_word);
    if !valid {
        return None;
    }
    let mut params = HashMap::new();
    if let Some(rest) = raw[code_end..].strip_prefix(':') {
        let segment = rest.split(|c: char| c == ':' || c.is_whitespace()).next().unwrap_or("");
        if segment.contains('=') && !segment.is_empty() {
            for pair in segment.split(',') {
                if let Some((k, v)) = pair.split_once('=') {
                    if !k.is_empty() {
                        params.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
    }
    Some((code.to_string(), params))
}

fn validation_detail(raw: &str) -> serde_json::Value {
    match split_coded_refusal(raw) {
        Some((code, params)) => serde_json::json!({ "message": raw, "code": code, "params": params }),
        None => serde_json::json!({ "message": raw }),
    }
}

impl fmt::Display for AppValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Convenience alias used by all command and service functions.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod coded_refusal_tests {
    use super::*;

    #[test]
    fn a_bare_code_is_a_code() {
        let (code, params) = split_coded_refusal("STUDY_STALE").expect("coded");
        assert_eq!(code, "STUDY_STALE");
        assert!(params.is_empty());
    }

    #[test]
    fn a_code_with_a_sentence_keeps_only_the_code() {
        let (code, params) = split_coded_refusal("GATE_FIN_PERIOD_CLOSED: période clôturée").expect("coded");
        assert_eq!(code, "GATE_FIN_PERIOD_CLOSED");
        assert!(params.is_empty());
    }

    #[test]
    fn parameters_ride_with_the_code() {
        let (code, params) = split_coded_refusal("GATE_FIN_MONTH_INVALID:min_len=10,max=3: sentence").expect("coded");
        assert_eq!(code, "GATE_FIN_MONTH_INVALID");
        assert_eq!(params.get("min_len").map(String::as_str), Some("10"));
        assert_eq!(params.get("max").map(String::as_str), Some("3"));
    }

    #[test]
    fn a_plain_sentence_has_no_code() {
        for plain in [
            "Heures de main-d'oeuvre requises.",
            "Title is required",
            "OT",
            "WO close-out blocked",
            "Validation failed: X_Y",
            "",
            "lower_case_code",
            "_LEADING_UNDERSCORE",
            "TRAILING_",
        ] {
            assert!(split_coded_refusal(plain).is_none(), "{plain:?} must have no code");
        }
    }

    #[test]
    fn edges_of_the_parameter_part_are_safe() {
        // Leading blanks, a reason that is not key=value, a value that holds '=', an empty key,
        // parameters with no sentence, and non-ASCII text after the code.
        let (code, params) = split_coded_refusal("  READINESS_DENY:MISSING_EXPOSURE  ").expect("coded");
        assert_eq!(code, "READINESS_DENY");
        assert!(params.is_empty());
        let (_, params) = split_coded_refusal("GATE_FIN_MONTH_INVALID:k=a=b").expect("coded");
        assert_eq!(params.get("k").map(String::as_str), Some("a=b"));
        let (_, params) = split_coded_refusal("GATE_FIN_MONTH_INVALID:=v,n=1").expect("coded");
        assert_eq!(params.len(), 1);
        let (code, _) = split_coded_refusal("GATE_FIN_MONTH_INVALID: période clôturée").expect("coded");
        assert_eq!(code, "GATE_FIN_MONTH_INVALID");
    }

    #[test]
    fn the_ipc_detail_carries_code_and_params_and_keeps_the_text() {
        let detail = validation_detail("GATE_FIN_PERIOD_CLOSED:n=4: texte");
        assert_eq!(detail["code"], "GATE_FIN_PERIOD_CLOSED");
        assert_eq!(detail["params"]["n"], "4");
        assert_eq!(detail["message"], "GATE_FIN_PERIOD_CLOSED:n=4: texte");
        let plain = validation_detail("Title is required");
        assert!(plain.get("code").is_none());
        assert_eq!(plain["message"], "Title is required");
    }
}
