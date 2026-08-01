//! DI reference catalog membership checks (equipment-parity).
//!
//! Origin and symptom are Category B domains (`DI.ORIGIN`, `DI.SYMPTOM`).
//! Create/update writes must resolve against the latest published active set —
//! free-text codes / unknown ids are rejected.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

const DOMAIN_ORIGIN: &str = "DI.ORIGIN";
const DOMAIN_SYMPTOM: &str = "DI.SYMPTOM";
const DOMAIN_REQUEST_TYPE: &str = "DI.REQUEST_TYPE";
const DOMAIN_DISPOSITION: &str = "DI.DISPOSITION";

/// Default request type when callers omit one (seeded Category A code).
pub const DEFAULT_DI_REQUEST_TYPE: &str = "repair";

/// Normalize and validate `origin_type` against published active `DI.ORIGIN` by code.
///
/// Returns the trimmed code as stored on the DI row. Tenant-extended Category B
/// codes are accepted once published; the closed [`crate::di::domain::DiOriginType`]
/// enum is **not** the create-time authority.
pub async fn validate_di_origin_code(db: &impl ConnectionTrait, origin_type: &str) -> AppResult<String> {
    let code = origin_type.trim().to_string();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le type d'origine est obligatoire.".into()
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) AND rv.is_active = 1",
            [DOMAIN_ORIGIN.into(), code.clone().into()],
        ))
        .await?;

    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Origine '{code}' introuvable dans le référentiel {DOMAIN_ORIGIN} (valeur publiée active requise)."
        )]));
    }

    Ok(code)
}

/// Normalize and validate `request_type` against published active `DI.REQUEST_TYPE` by code.
pub async fn validate_di_request_type(db: &impl ConnectionTrait, request_type: &str) -> AppResult<String> {
    let code = request_type.trim().to_string();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le type de demande est obligatoire.".into()
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) AND rv.is_active = 1",
            [DOMAIN_REQUEST_TYPE.into(), code.clone().into()],
        ))
        .await?;

    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type de demande '{code}' introuvable dans le référentiel {DOMAIN_REQUEST_TYPE} (valeur publiée active requise)."
        )]));
    }

    Ok(code)
}

/// Validate disposition code against published active `DI.DISPOSITION`.
/// If the domain has no published values yet, accepts the code (migration edge).
pub async fn validate_di_disposition(db: &impl ConnectionTrait, disposition_code: &str) -> AppResult<String> {
    let code = disposition_code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code de disposition est obligatoire.".into(),
        ]));
    }

    let any = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' AND rv.is_active = 1 \
             LIMIT 1",
            [DOMAIN_DISPOSITION.into()],
        ))
        .await?;
    if any.is_none() {
        return Ok(code);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) AND rv.is_active = 1",
            [DOMAIN_DISPOSITION.into(), code.clone().into()],
        ))
        .await?;

    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Disposition '{code}' introuvable dans le référentiel {DOMAIN_DISPOSITION}."
        )]));
    }

    Ok(code)
}

/// Validate `symptom_code_id` against published active `DI.SYMPTOM` by id.
pub async fn validate_di_symptom_id(db: &impl ConnectionTrait, symptom_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND rv.id = ? AND rv.is_active = 1",
            [DOMAIN_SYMPTOM.into(), symptom_id.into()],
        ))
        .await?;

    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Symptôme (id={symptom_id}) introuvable dans le référentiel {DOMAIN_SYMPTOM} (valeur publiée active requise)."
        )]));
    }

    Ok(())
}

/// Require a non-null symptom and validate catalog membership (create path).
pub async fn require_di_symptom_id(db: &impl ConnectionTrait, symptom_code_id: Option<i64>) -> AppResult<i64> {
    let id = symptom_code_id.ok_or_else(|| AppError::ValidationFailed(vec!["Le symptôme est obligatoire.".into()]))?;
    validate_di_symptom_id(db, id).await?;
    Ok(id)
}

/// Resolve a published active `DI.SYMPTOM` id by code (for system-generated DIs).
pub async fn resolve_di_symptom_id_by_code(db: &impl ConnectionTrait, code: &str) -> AppResult<Option<i64>> {
    let code = code.trim();
    if code.is_empty() {
        return Ok(None);
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) AND rv.is_active = 1",
            [DOMAIN_SYMPTOM.into(), code.into()],
        ))
        .await?;

    match row {
        Some(r) => {
            Ok(Some(r.try_get::<i64>("", "id").map_err(|e| {
                AppError::Internal(anyhow::anyhow!("di symptom id decode: {e}"))
            })?))
        }
        None => Ok(None),
    }
}

/// Resolve a fallback published symptom for system intake (inspection / PM).
///
/// Prefers an exact code match, then seeded `performance_drop`.
pub async fn resolve_system_di_symptom_id(db: &impl ConnectionTrait, preferred_code: &str) -> AppResult<i64> {
    if let Some(id) = resolve_di_symptom_id_by_code(db, preferred_code).await? {
        return Ok(id);
    }
    if let Some(id) = resolve_di_symptom_id_by_code(db, "performance_drop").await? {
        return Ok(id);
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Aucun symptôme publié actif dans {DOMAIN_SYMPTOM} pour l'intake système."
    )]))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use super::*;

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.expect("connect");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("pragma");
        crate::migrations::Migrator::up(&db, None).await.expect("migrate");
        crate::db::seeder::seed_system_data(&db).await.expect("seed");
        db
    }

    async fn symptom_id(db: &sea_orm::DatabaseConnection, code: &str) -> i64 {
        resolve_di_symptom_id_by_code(db, code)
            .await
            .expect("lookup")
            .expect("seeded symptom")
    }

    #[tokio::test]
    async fn valid_origin_and_symptom_succeed() {
        let db = setup().await;
        let origin = validate_di_origin_code(&db, "operator").await.expect("origin");
        assert_eq!(origin, "operator");
        let sid = symptom_id(&db, "vibration").await;
        require_di_symptom_id(&db, Some(sid)).await.expect("symptom");
    }

    #[tokio::test]
    async fn unknown_origin_fails() {
        let db = setup().await;
        let err = validate_di_origin_code(&db, "not-a-real-origin")
            .await
            .expect_err("must fail");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn unknown_symptom_id_fails() {
        let db = setup().await;
        let err = validate_di_symptom_id(&db, 9_999_999).await.expect_err("must fail");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn null_symptom_fails_on_require() {
        let db = setup().await;
        let err = require_di_symptom_id(&db, None).await.expect_err("must fail");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn tenant_style_origin_accepted_when_published() {
        let db = setup().await;
        // Insert an extra published origin (Category B open catalog).
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, 'contractor', 'Sous-traitant', NULL, 99, NULL, NULL, \
                    'di_origin', NULL, 1, '{}' \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE d.code = 'DI.ORIGIN'"
                .to_string(),
        ))
        .await
        .expect("insert tenant origin");

        let code = validate_di_origin_code(&db, "contractor")
            .await
            .expect("Category B code must be accepted");
        assert_eq!(code, "contractor");
    }
}
