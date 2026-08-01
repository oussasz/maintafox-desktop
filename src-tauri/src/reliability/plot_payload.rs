//! Canonical JSON for KPI snapshot charts (frontend Recharts) + dataset hash binding.

use serde_json::json;

pub fn build_kpi_plot_payload_json(
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
    dataset_hash_sha256: &str,
    exposure_hours: f64,
    event_count: i64,
    mtbf: Option<f64>,
    mttr: Option<f64>,
    availability: Option<f64>,
    failure_rate: Option<f64>,
    repeat_failure_rate: Option<f64>,
) -> String {
    fn bar_val(v: Option<f64>) -> f64 {
        v.filter(|x| x.is_finite()).unwrap_or(0.0)
    }

    json!({
        "spec_version": 1,
        "dataset_hash_sha256": dataset_hash_sha256,
        "equipment_id": equipment_id,
        "period_start": period_start,
        "period_end": period_end,
        "exposure_hours": exposure_hours,
        "event_count": event_count,
        "series": [
            {"key": "mtbf", "label": "MTBF (h)", "value": mtbf},
            {"key": "mttr", "label": "MTTR (h)", "value": mttr},
            {"key": "availability", "label": "Availability", "value": availability},
            {"key": "failure_rate", "label": "λ (failures/h)", "value": failure_rate},
            {"key": "repeat_failure_rate", "label": "Repeat rate", "value": repeat_failure_rate},
        ],
        "bar_chart": [
            {"name": "MTBF", "value": bar_val(mtbf)},
            {"name": "MTTR", "value": bar_val(mttr)},
            {"name": "A", "value": bar_val(availability)},
            {"name": "λ", "value": bar_val(failure_rate)},
            {"name": "rép.", "value": bar_val(repeat_failure_rate)},
        ]
    })
    .to_string()
}
