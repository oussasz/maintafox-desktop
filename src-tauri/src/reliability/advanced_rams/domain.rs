use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeibullFitRunInput {
    pub equipment_id: i64,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeibullFitRecord {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub n_points: i64,
    pub inter_arrival_hours_json: String,
    pub beta: Option<f64>,
    pub eta: Option<f64>,
    pub beta_ci_low: Option<f64>,
    pub beta_ci_high: Option<f64>,
    pub eta_ci_low: Option<f64>,
    pub eta_ci_high: Option<f64>,
    pub adequate_sample: bool,
    pub message: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FmecaAnalysis {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub title: String,
    pub boundary_definition: String,
    pub status: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFmecaAnalysisInput {
    pub equipment_id: i64,
    pub title: String,
    pub boundary_definition: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFmecaAnalysisInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub boundary_definition: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FmecaAnalysesFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FmecaItem {
    pub id: i64,
    pub entity_sync_id: String,
    pub analysis_id: i64,
    pub component_id: Option<i64>,
    pub functional_failure: String,
    pub failure_mode_id: Option<i64>,
    pub failure_effect: String,
    pub severity: i64,
    pub occurrence: i64,
    pub detectability: i64,
    pub rpn: i64,
    pub recommended_action: String,
    pub current_control: String,
    pub linked_pm_plan_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub revised_rpn: Option<i64>,
    #[serde(default)]
    pub source_ram_ishikawa_diagram_id: Option<i64>,
    #[serde(default)]
    pub source_ishikawa_flow_node_id: Option<String>,
    pub row_version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertFmecaItemInput {
    pub id: Option<i64>,
    pub analysis_id: i64,
    pub expected_row_version: Option<i64>,
    pub component_id: Option<i64>,
    pub functional_failure: Option<String>,
    pub failure_mode_id: Option<i64>,
    pub failure_effect: Option<String>,
    pub severity: i64,
    pub occurrence: i64,
    pub detectability: i64,
    pub recommended_action: Option<String>,
    pub current_control: Option<String>,
    pub linked_pm_plan_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub revised_rpn: Option<i64>,
    #[serde(default)]
    pub source_ram_ishikawa_diagram_id: Option<i64>,
    #[serde(default)]
    pub source_ishikawa_flow_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RcmStudy {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub title: String,
    pub status: String,
    pub row_version: i64,
    pub created_at: String,
    pub created_by_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRcmStudyInput {
    pub equipment_id: i64,
    pub title: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRcmStudyInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RcmStudiesFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RcmDecision {
    pub id: i64,
    pub entity_sync_id: String,
    pub study_id: i64,
    pub function_description: String,
    pub functional_failure: String,
    pub failure_mode_id: Option<i64>,
    pub consequence_category: String,
    pub selected_tactic: String,
    pub justification: String,
    pub review_due_at: Option<String>,
    pub linked_pm_plan_id: Option<i64>,
    pub row_version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertRcmDecisionInput {
    pub id: Option<i64>,
    pub study_id: i64,
    pub expected_row_version: Option<i64>,
    pub function_description: Option<String>,
    pub functional_failure: Option<String>,
    pub failure_mode_id: Option<i64>,
    pub consequence_category: Option<String>,
    pub selected_tactic: String,
    pub justification: Option<String>,
    pub review_due_at: Option<String>,
    pub linked_pm_plan_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FmecaSoCell {
    pub severity: i64,
    pub occurrence: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FmecaSeverityOccurrenceMatrix {
    pub equipment_id: i64,
    pub cells: Vec<FmecaSoCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FmecaItemsEquipmentFilter {
    pub equipment_id: i64,
    pub severity: Option<i64>,
    pub occurrence: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FmecaItemWithContext {
    #[serde(flatten)]
    pub item: FmecaItem,
    pub analysis_title: String,
    pub equipment_id: i64,
    pub spare_stock_total: Option<f64>,
    pub inventory_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityRulIndicator {
    pub equipment_id: i64,
    pub weibull_beta: Option<f64>,
    pub weibull_eta_hours: Option<f64>,
    pub reliability_at_t: Option<f64>,
    pub predicted_rul_hours: Option<f64>,
    pub t_hours: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamIshikawaDiagram {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub title: String,
    pub flow_json: String,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertRamIshikawaDiagramInput {
    pub id: Option<i64>,
    pub equipment_id: i64,
    pub expected_row_version: Option<i64>,
    pub title: Option<String>,
    pub flow_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RamIshikawaDiagramsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}
