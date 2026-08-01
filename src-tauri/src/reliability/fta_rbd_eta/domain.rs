use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtaModel {
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
pub struct CreateFtaModelInput {
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFtaModelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FtaModelsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbdModel {
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
pub struct CreateRbdModelInput {
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRbdModelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RbdModelsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTreeModel {
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
pub struct CreateEventTreeModelInput {
    pub equipment_id: i64,
    pub title: String,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEventTreeModelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub graph_json: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventTreeModelsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}
