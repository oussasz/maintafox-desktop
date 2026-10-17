//! FMECA rating scales of the plant (setting `ram.fmeca_rating_scales`, tenant): severity,
//! occurrence and detection on 1–10, and the 5 × 5 criticality matrix zones.
//!
//! The setting is seeded with the scales below (maintenance FMECA practice: severity by
//! consequence class, occurrence by failures per equipment-year, detection by the chance of
//! seeing the failure coming) and edited by a reliability manager. Each severity level carries
//! its RCM consequence class and each detection level says whether the failure is hidden, so an
//! FMECA row pre-answers the RCM questions.

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::settings;

pub const SETTING_KEY: &str = "ram.fmeca_rating_scales";

/// Seeded value of [`SETTING_KEY`] (`db::seeder::DEFAULT_SETTINGS`).
pub const DEFAULT_RATING_SCALES_JSON: &str = r#"{"version":1,
"severity":[
{"score":10,"consequence":"safety","label_fr":"Accident grave possible sans signe avant-coureur","label_en":"Possible serious injury without warning"},
{"score":9,"consequence":"safety","label_fr":"Blessure ou dépassement d'une limite environnementale","label_en":"Injury or breach of an environmental limit"},
{"score":8,"consequence":"operational","label_fr":"Arrêt de production de plus de 4 h","label_en":"Production stop longer than 4 h"},
{"score":7,"consequence":"operational","label_fr":"Arrêt de production de 1 à 4 h","label_en":"Production stop of 1 to 4 h"},
{"score":6,"consequence":"operational","label_fr":"Arrêt de moins d'1 h ou cadence réduite de plus de moitié","label_en":"Stop under 1 h or output reduced by more than half"},
{"score":5,"consequence":"operational","label_fr":"Cadence ou qualité dégradée","label_en":"Reduced output or quality"},
{"score":4,"consequence":"non_operational","label_fr":"Fonction dégradée sans perte de production, réparation coûteuse","label_en":"Degraded function without production loss, costly repair"},
{"score":3,"consequence":"non_operational","label_fr":"Réparation mineure","label_en":"Minor repair"},
{"score":2,"consequence":"non_operational","label_fr":"Effet négligeable","label_en":"Negligible effect"},
{"score":1,"consequence":"non_operational","label_fr":"Aucun effet","label_en":"No effect"}],
"occurrence":[
{"score":10,"min_per_year":52,"label_fr":"Plus d'une fois par semaine","label_en":"More than once a week"},
{"score":9,"min_per_year":12,"label_fr":"Environ une fois par mois","label_en":"About once a month"},
{"score":8,"min_per_year":4,"label_fr":"Environ une fois par trimestre","label_en":"About once a quarter"},
{"score":7,"min_per_year":2,"label_fr":"Environ deux fois par an","label_en":"About twice a year"},
{"score":6,"min_per_year":0.7,"label_fr":"Environ une fois par an","label_en":"About once a year"},
{"score":5,"min_per_year":0.3,"label_fr":"Tous les 2 à 3 ans","label_en":"Every 2 to 3 years"},
{"score":4,"min_per_year":0.1,"label_fr":"Tous les 3 à 10 ans","label_en":"Every 3 to 10 years"},
{"score":3,"min_per_year":0.03,"label_fr":"Tous les 10 à 30 ans","label_en":"Every 10 to 30 years"},
{"score":2,"min_per_year":0.01,"label_fr":"Tous les 30 à 100 ans","label_en":"Every 30 to 100 years"},
{"score":1,"min_per_year":0,"label_fr":"Moins d'une fois en 100 ans","label_en":"Less than once in 100 years"}],
"detection":[
{"score":10,"hidden":true,"label_fr":"Invisible : aucun moyen de la voir avant d'en subir les effets","label_en":"Hidden: no way to see it before its effects"},
{"score":9,"hidden":true,"label_fr":"Cachée en fonctionnement normal, trouvée seulement par un essai","label_en":"Hidden in normal operation, found only by a test"},
{"score":8,"hidden":false,"label_fr":"Vue seulement au moment de la défaillance","label_en":"Seen only when it fails"},
{"score":7,"hidden":false,"label_fr":"Très peu de chances de la voir venir lors des rondes","label_en":"Very low chance of seeing it coming on rounds"},
{"score":6,"hidden":false,"label_fr":"Signes perçus par l'opérateur (bruit, odeur)","label_en":"Signs sensed by the operator (noise, smell)"},
{"score":5,"hidden":false,"label_fr":"Inspection périodique avec chance moyenne","label_en":"Periodic inspection with a moderate chance"},
{"score":4,"hidden":false,"label_fr":"Mesure périodique (vibration, thermographie)","label_en":"Periodic measurement (vibration, thermography)"},
{"score":3,"hidden":false,"label_fr":"Surveillance continue avec alarme","label_en":"Continuous monitoring with alarm"},
{"score":2,"hidden":false,"label_fr":"Protection automatique avant la défaillance","label_en":"Automatic protection before failure"},
{"score":1,"hidden":false,"label_fr":"Détection certaine, sécurité positive","label_en":"Certain detection, fail-safe"}],
"matrix":{"classes":5,"zones":[
{"min_product":15,"zone":"unacceptable"},
{"min_product":8,"zone":"reduce"},
{"min_product":4,"zone":"monitor"},
{"min_product":0,"zone":"acceptable"}]}}"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingLevel {
    pub score: i64,
    pub label_fr: String,
    pub label_en: String,
    /// Severity: RCM consequence class (`safety` | `operational` | `non_operational`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consequence: Option<String>,
    /// Occurrence: lowest failures per equipment-year of the level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_per_year: Option<f64>,
    /// Detection: the failure is hidden from operators in normal operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatrixZone {
    pub min_product: i64,
    /// `unacceptable` | `reduce` | `monitor` | `acceptable`.
    pub zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatrixSpec {
    pub classes: i64,
    pub zones: Vec<MatrixZone>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingScales {
    pub version: i64,
    pub severity: Vec<RatingLevel>,
    pub occurrence: Vec<RatingLevel>,
    pub detection: Vec<RatingLevel>,
    pub matrix: MatrixSpec,
}

pub const ZONE_UNACCEPTABLE: &str = "unacceptable";
pub const ZONE_REDUCE: &str = "reduce";
pub const ZONE_MONITOR: &str = "monitor";
pub const ZONE_ACCEPTABLE: &str = "acceptable";

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

impl RatingScales {
    /// Scales must rate every score 1–10 once, occurrence thresholds must fall with the score,
    /// and every severity level must name an RCM consequence class.
    pub fn validate(&self) -> AppResult<()> {
        for (name, levels) in [
            ("severity", &self.severity),
            ("occurrence", &self.occurrence),
            ("detection", &self.detection),
        ] {
            let mut scores: Vec<i64> = levels.iter().map(|l| l.score).collect();
            scores.sort_unstable();
            if scores != (1..=10).collect::<Vec<_>>() {
                return Err(invalid(&format!("RATING_SCALE_SCORES:{name}")));
            }
            if levels
                .iter()
                .any(|l| l.label_fr.trim().is_empty() || l.label_en.trim().is_empty())
            {
                return Err(invalid(&format!("RATING_SCALE_LABEL:{name}")));
            }
        }
        if self.severity.iter().any(|l| {
            !matches!(
                l.consequence.as_deref(),
                Some("safety" | "operational" | "non_operational")
            )
        }) {
            return Err(invalid("RATING_SCALE_CONSEQUENCE"));
        }
        let mut occ: Vec<(i64, f64)> = self
            .occurrence
            .iter()
            .map(|l| (l.score, l.min_per_year.unwrap_or(f64::NAN)))
            .collect();
        occ.sort_by_key(|o| o.0);
        if occ.iter().any(|o| !o.1.is_finite() || o.1 < 0.0) || occ.windows(2).any(|w| w[1].1 <= w[0].1) {
            return Err(invalid("RATING_SCALE_OCCURRENCE_THRESHOLDS"));
        }
        if self.detection.iter().any(|l| l.hidden.is_none()) {
            return Err(invalid("RATING_SCALE_DETECTION_HIDDEN"));
        }
        if self.matrix.classes < 2 || self.matrix.classes > 10 || self.matrix.zones.is_empty() {
            return Err(invalid("RATING_SCALE_MATRIX"));
        }
        Ok(())
    }

    fn level(levels: &[RatingLevel], score: i64) -> Option<&RatingLevel> {
        levels.iter().find(|l| l.score == score)
    }

    #[must_use]
    pub fn severity_level(&self, score: i64) -> Option<&RatingLevel> {
        Self::level(&self.severity, score)
    }

    #[must_use]
    pub fn detection_level(&self, score: i64) -> Option<&RatingLevel> {
        Self::level(&self.detection, score)
    }

    /// Occurrence score of a failure frequency (failures per equipment-year): the highest level
    /// whose threshold the frequency reaches.
    #[must_use]
    pub fn occurrence_score(&self, per_year: f64) -> i64 {
        let mut levels: Vec<&RatingLevel> = self.occurrence.iter().collect();
        levels.sort_by_key(|l| std::cmp::Reverse(l.score));
        levels
            .iter()
            .find(|l| per_year >= l.min_per_year.unwrap_or(0.0))
            .map_or(1, |l| l.score)
    }

    /// Matrix class (1..classes) of a 1–10 score.
    #[must_use]
    pub fn class_of(&self, score: i64) -> i64 {
        let c = self.matrix.classes.max(1);
        ((score.clamp(1, 10) * c + 9) / 10).clamp(1, c)
    }

    /// Risk zone of a severity / occurrence pair (classes multiplied).
    #[must_use]
    pub fn zone(&self, severity: i64, occurrence: i64) -> String {
        let product = self.class_of(severity) * self.class_of(occurrence);
        let mut zones = self.matrix.zones.clone();
        zones.sort_by_key(|z| std::cmp::Reverse(z.min_product));
        zones
            .iter()
            .find(|z| product >= z.min_product)
            .map_or_else(|| ZONE_ACCEPTABLE.to_string(), |z| z.zone.clone())
    }
}

#[must_use]
pub fn default_scales() -> RatingScales {
    serde_json::from_str(DEFAULT_RATING_SCALES_JSON).expect("seeded FMECA rating scales parse")
}

/// Scales of the plant: the tenant setting, or the seeded value while the setting is absent.
pub async fn load_scales(db: &DatabaseConnection) -> AppResult<RatingScales> {
    let Some(s) = settings::get_setting(db, SETTING_KEY, "tenant").await? else {
        return Ok(default_scales());
    };
    let scales: RatingScales =
        serde_json::from_str(&s.setting_value_json).map_err(|_| invalid("RATING_SCALE_SETTING_INVALID"))?;
    scales.validate()?;
    Ok(scales)
}

pub async fn save_scales(db: &DatabaseConnection, scales: &RatingScales, actor_id: i32) -> AppResult<RatingScales> {
    scales.validate()?;
    let json = serde_json::to_string(scales)?;
    settings::set_setting(
        db,
        SETTING_KEY,
        "tenant",
        &json,
        Some(actor_id),
        "FMECA rating scales updated from the reliability module",
    )
    .await?;
    load_scales(db).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_scales_are_valid_and_read_the_mockup_example() {
        let s = default_scales();
        s.validate().expect("valid");
        // Bearing of the approved mockup: about once a year → 6; G 8 · O 6 → classes 4 · 3 = 12.
        assert_eq!(s.occurrence_score(1.0), 6);
        assert_eq!(s.occurrence_score(0.7), 6);
        assert_eq!(s.occurrence_score(0.69), 5);
        assert_eq!(s.occurrence_score(60.0), 10);
        assert_eq!(s.occurrence_score(0.0), 1);
        assert_eq!(
            (s.class_of(8), s.class_of(6), s.class_of(1), s.class_of(10)),
            (4, 3, 1, 5)
        );
        assert_eq!(s.zone(8, 6), ZONE_REDUCE);
        assert_eq!(s.zone(10, 10), ZONE_UNACCEPTABLE);
        assert_eq!(s.zone(2, 3), ZONE_ACCEPTABLE);
        assert_eq!(s.zone(4, 4), ZONE_MONITOR);
        assert_eq!(
            s.severity_level(9).and_then(|l| l.consequence.as_deref()),
            Some("safety")
        );
        assert_eq!(s.detection_level(9).and_then(|l| l.hidden), Some(true));
    }

    #[test]
    fn invalid_scales_are_refused() {
        let mut s = default_scales();
        s.occurrence[0].min_per_year = Some(0.5);
        assert!(s.validate().is_err(), "thresholds must fall with the score");
        let mut s = default_scales();
        s.severity.pop();
        assert!(s.validate().is_err());
        let mut s = default_scales();
        s.severity[0].consequence = Some("other".into());
        assert!(s.validate().is_err());
    }
}
