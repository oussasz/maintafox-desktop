//! Service layer for lookup domain and value management.
//! Wraps lookup_repository with business rules:
//! - Prevents insertion into locked or non-extensible domains
//! - Validates code format (alphanumeric + underscore, uppercase, max 64 chars)
//! - Flushes relevant caches on mutation (Phase 2 will add caching here)

use crate::errors::{AppError, AppResult};
use crate::repository::lookup_repository::{
    self, LookupDomainFilter, LookupDomainSummary, LookupValueOption, LookupValueRecord,
};
use crate::repository::{Page, PageRequest};
use sea_orm::DatabaseConnection;

pub async fn list_domains(
    db: &DatabaseConnection,
    filter: LookupDomainFilter,
    page: PageRequest,
) -> AppResult<Page<LookupDomainSummary>> {
    lookup_repository::list_lookup_domains(db, &filter, &page).await
}

pub async fn get_domain_values(db: &DatabaseConnection, domain_key: &str) -> AppResult<Vec<LookupValueOption>> {
    lookup_repository::get_domain_values(db, domain_key, true).await
}

pub async fn get_value_by_id(db: &DatabaseConnection, value_id: i32) -> AppResult<LookupValueRecord> {
    lookup_repository::get_value_by_id(db, value_id).await
}

/// Validates a lookup value code before insertion.
/// Code must be: ASCII uppercase, digits, underscores only; 1–64 characters.
pub fn validate_code(code: &str) -> AppResult<()> {
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::ValidationFailed(vec![
            "Le code doit comporter entre 1 et 64 caractères.".into(),
        ]));
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AppError::ValidationFailed(vec![
            "Le code ne peut contenir que des majuscules, chiffres et tirets bas (ex : CORRECTIVE, TYPE_A).".into(),
        ]));
    }
    Ok(())
}

/// Checks that a domain is extensible before allowing value insertion.
pub async fn assert_domain_extensible(db: &DatabaseConnection, domain_key: &str) -> AppResult<()> {
    let sql = "SELECT is_extensible, is_locked FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL";
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [domain_key.into()],
        ))
        .await?;

    match row {
        None => Err(AppError::NotFound {
            entity: "lookup_domain".into(),
            id: domain_key.to_string(),
        }),
        Some(r) => {
            let is_extensible: i32 = r.try_get("", "is_extensible").unwrap_or(0);
            let is_locked: i32 = r.try_get("", "is_locked").unwrap_or(0);
            if is_locked == 1 {
                return Err(AppError::Permission {
                    action: "insert_value".into(),
                    resource: format!("lookup_domain:{domain_key}"),
                });
            }
            if is_extensible == 0 {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Le domaine '{domain_key}' n'est pas extensible."
                )]));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_code_accepts_valid_codes() {
        assert!(validate_code("CORRECTIVE").is_ok());
        assert!(validate_code("TYPE_A").is_ok());
        assert!(validate_code("CODE_123").is_ok());
    }

    #[test]
    fn validate_code_rejects_empty() {
        assert!(validate_code("").is_err());
    }

    #[test]
    fn validate_code_rejects_lowercase() {
        assert!(validate_code("corrective").is_err());
    }

    #[test]
    fn validate_code_rejects_spaces() {
        assert!(validate_code("TYPE A").is_err());
    }

    #[test]
    fn validate_code_rejects_over_64_chars() {
        let long = "A".repeat(65);
        assert!(validate_code(&long).is_err());
    }

    #[test]
    fn validate_code_accepts_exactly_64_chars() {
        let exact = "A".repeat(64);
        assert!(validate_code(&exact).is_ok());
    }
}
