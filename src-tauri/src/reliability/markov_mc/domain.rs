use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McModel {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: String,
    pub trials: i64,
    pub seed: Option<i64>,
    pub result_json: String,
    pub status: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMcModelInput {
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: Option<String>,
    pub trials: Option<i64>,
    pub seed: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMcModelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub graph_json: Option<String>,
    pub trials: Option<i64>,
    pub seed: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McModelsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovModel {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: String,
    pub result_json: String,
    pub status: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMarkovModelInput {
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMarkovModelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarkovModelsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}
