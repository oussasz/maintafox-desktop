use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamExpertSignOff {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub method_category: String,
    pub target_ref: Option<String>,
    pub title: String,
    pub reviewer_name: String,
    pub reviewer_role: String,
    pub status: String,
    pub signed_at: Option<String>,
    pub notes: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRamExpertSignOffInput {
    pub equipment_id: i64,
    pub method_category: String,
    pub target_ref: Option<String>,
    pub title: String,
    pub reviewer_name: Option<String>,
    pub reviewer_role: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRamExpertSignOffInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub reviewer_name: Option<String>,
    pub reviewer_role: Option<String>,
    pub notes: Option<String>,
    pub target_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignRamExpertReviewInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub reviewer_name: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RamExpertSignOffsFilter {
    pub equipment_id: Option<i64>,
    pub method_category: Option<String>,
    pub limit: Option<i64>,
}
