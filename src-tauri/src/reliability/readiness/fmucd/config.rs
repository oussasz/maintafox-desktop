use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Versioned FMUCD → readiness mapping (see `research/readiness-poc/config/fmucd_mapping.v1.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct FmucdMappingConfig {
    pub version: u32,
    pub dataset: DatasetMeta,
    pub asset_key: AssetKeyConfig,
    pub eligibility: EligibilityConfig,
    pub dimensions: DimensionsConfig,
    #[serde(default)]
    pub mapping_mode: MappingModeConfig,
    pub evaluation: EvaluationConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatasetMeta {
    pub name: String,
    pub citation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetKeyConfig {
    pub fields: Vec<String>,
    pub separator: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EligibilityConfig {
    pub eligible_when_ppm_upm_equals: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DimensionsConfig {
    pub equipment_identified_requires: Vec<String>,
    pub interval_start_field: String,
    pub interval_end_field: String,
    pub failure_mode_primary_field: String,
    pub failure_mode_fallback_field: String,
    pub corrective_evidence_field: String,
}

/// Controls strict vs relaxed proxy rules for mapping sensitivity.
#[derive(Debug, Clone, Deserialize)]
pub struct MappingModeConfig {
    /// `primary_only` | `primary_or_fallback` | `relaxed`
    #[serde(default = "default_failure_mode_rule")]
    pub failure_mode_rule: String,
    /// `both_dates` | `any_date`
    #[serde(default = "default_interval_rule")]
    pub interval_rule: String,
}

impl Default for MappingModeConfig {
    fn default() -> Self {
        Self {
            failure_mode_rule: default_failure_mode_rule(),
            interval_rule: default_interval_rule(),
        }
    }
}

fn default_failure_mode_rule() -> String {
    "primary_or_fallback".to_string()
}

fn default_interval_rule() -> String {
    "both_dates".to_string()
}

impl MappingModeConfig {
    #[must_use]
    pub fn failure_mode_strict(&self) -> bool {
        self.failure_mode_rule == "primary_only"
    }

    #[must_use]
    pub fn failure_mode_relaxed(&self) -> bool {
        self.failure_mode_rule == "relaxed"
    }

    #[must_use]
    pub fn interval_any_date(&self) -> bool {
        self.interval_rule == "any_date"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluationConfig {
    pub min_sample_n: i64,
    pub exposure_hours_external: f64,
}

impl FmucdMappingConfig {
    pub fn from_toml_file(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read mapping config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse mapping TOML {}", path.display()))
    }
}

/// Build asset key from row fields per config.
#[must_use]
pub fn build_asset_key(
    config: &AssetKeyConfig,
    fields: &HashMap<String, String>,
) -> Option<String> {
    let parts: Vec<String> = config
        .fields
        .iter()
        .map(|name| {
            fields
                .get(name)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
        })
        .collect::<Option<Vec<_>>>()?;
    if parts.len() != config.fields.len() {
        return None;
    }
    Some(parts.join(&config.separator))
}
