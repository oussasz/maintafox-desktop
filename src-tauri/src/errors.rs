use thiserror::Error;

/// Unified application error type. All service and command functions return
/// `AppResult<T>` rather than mixing error types across the IPC boundary.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Authentication error: {0}")]
    Auth(String),

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
            Self::NotFound { .. } => "NOT_FOUND",
            Self::ValidationFailed(_) => "VALIDATION_FAILED",
            Self::SyncError(_) => "SYNC_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Permission { .. } => "PERMISSION_DENIED",
            Self::Internal(_) => "INTERNAL_ERROR",
        };
        state.serialize_field("code", code)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// Convenience alias used by all command and service functions.
pub type AppResult<T> = Result<T, AppError>;
