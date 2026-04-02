use sea_orm::DatabaseConnection;

use crate::errors::AppResult;

/// Confirms the reference domain tables exist by running a trivial COUNT query.
/// Called during startup validation (Sprint S3 of this sub-phase).
pub async fn verify_reference_domain_tables(db: &DatabaseConnection) -> AppResult<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    for table in &["lookup_domains", "lookup_values", "lookup_value_aliases"] {
        let sql = format!("SELECT COUNT(*) FROM {table};");
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await?;
    }

    Ok(())
}
