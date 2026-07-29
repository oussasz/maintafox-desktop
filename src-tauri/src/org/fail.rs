//! Org-facing validation helpers (stable codes + English fallback for i18n).

use std::collections::HashMap;

use crate::errors::{issue_params, org_validation_failed, AppError};

#[inline]
pub fn fail(code: &str, message: impl Into<String>) -> AppError {
    org_validation_failed(code, message, HashMap::new())
}

#[inline]
pub fn fail_params(code: &str, message: impl Into<String>, pairs: &[(&str, String)]) -> AppError {
    org_validation_failed(code, message, issue_params(pairs))
}
