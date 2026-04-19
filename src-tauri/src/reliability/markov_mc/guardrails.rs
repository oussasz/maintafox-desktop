use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GuardrailFlags {
    pub monte_carlo_enabled: bool,
    pub markov_enabled: bool,
    pub mc_max_trials: i64,
    pub markov_max_states: i64,
}

impl Default for GuardrailFlags {
    fn default() -> Self {
        Self {
            monte_carlo_enabled: true,
            markov_enabled: true,
            mc_max_trials: 1_000_000,
            markov_max_states: 128,
        }
    }
}

pub async fn load_guardrails(db: &DatabaseConnection) -> AppResult<GuardrailFlags> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT flags_json FROM ram_advanced_guardrails WHERE id = 1".to_string(),
        ))
        .await?;
    let Some(r) = row else {
        return Ok(GuardrailFlags::default());
    };
    let s: String = r.try_get("", "flags_json").map_err(|e| AppError::SyncError(format!("flags_json: {e}")))?;
    serde_json::from_str(&s).map_err(|e| AppError::ValidationFailed(vec![format!("guardrails JSON: {e}")]))
}

pub async fn save_guardrails(db: &DatabaseConnection, flags: &GuardrailFlags) -> AppResult<()> {
    let json = serde_json::to_string(flags).map_err(|e| AppError::ValidationFailed(vec![e.to_string()]))?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO ram_advanced_guardrails (id, flags_json, updated_at) VALUES (1, ?, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET flags_json = excluded.flags_json, updated_at = datetime('now')",
        [json.into()],
    ))
    .await?;
    Ok(())
}
