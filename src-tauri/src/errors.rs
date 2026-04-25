use thiserror::Error;

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

    #[error("Validation failed: {0:?}")]
    ValidationFailed(Vec<String>),

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

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Serialize `AppError` to JSON for the Tauri IPC boundary.
/// Frontend receives: `{ "code": "NOT_FOUND", "message": "...", "details": null }`
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        let code = match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::Auth(_) => "AUTH_ERROR",
            Self::TenantScopeViolation(_) => "TENANT_SCOPE_VIOLATION",
            Self::SessionClaimInvalid(_) => "SESSION_CLAIM_INVALID",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::ValidationFailed(_) => "VALIDATION_FAILED",
            Self::SyncError(_) => "SYNC_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Permission { .. } => "PERMISSION_DENIED",
            Self::PermissionDenied(_) => "PERMISSION_DENIED",
            Self::LicenseDenied { .. } => "LICENSE_DENIED",
            Self::StepUpRequired => "STEP_UP_REQUIRED",
            Self::AccountLocked { .. } => "ACCOUNT_LOCKED",
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
            _ => self.to_string(),
        };
        state.serialize_field("message", &message)?;
        state.end()
    }
}

/// Convenience alias used by all command and service functions.
pub type AppResult<T> = Result<T, AppError>;
