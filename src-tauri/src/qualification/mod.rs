pub mod catalog;
pub mod domain;
pub mod expiry_alerts;
pub mod gate;
pub mod matrix;
pub mod queries;
pub mod readiness;
pub mod requirements;
pub(crate) mod sync_check;
pub mod training;

#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::errors::AppError;

/// Coded, translatable validation error (`training:errors.<code>` on the client). `message` is
/// the English fallback shown when the client has no translation for `code`.
pub(crate) fn issue(code: &str, message: &str, params: &[(&str, String)]) -> AppError {
    let params: HashMap<String, String> = params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
    crate::errors::org_validation_failed(code, message, params)
}
