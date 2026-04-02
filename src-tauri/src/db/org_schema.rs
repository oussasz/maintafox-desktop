use sea_orm::DatabaseConnection;

use crate::errors::AppResult;

/// Confirms the org schema tables exist by running trivial COUNT queries.
/// Called during startup validation (Sprint S3 of this sub-phase).
pub async fn verify_org_tables(db: &DatabaseConnection) -> AppResult<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    for table in &["org_structure_models", "org_node_types", "org_nodes"] {
        let sql = format!("SELECT COUNT(*) FROM {};", table);
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await?;
    }

    Ok(())
}
