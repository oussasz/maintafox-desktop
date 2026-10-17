//! FMECA rating grids of the plant, kept in Données de référence: severity, occurrence and
//! detection on 1–10, and the criticality matrix zones.
//!
//! Four SYSTEM domains (fixed codes: the FMECA arithmetic depends on them), on the model of the
//! shared urgency scale (`reference::urgency_scale`):
//! - `RELIABILITY.FMECA_SEVERITY` / `_OCCURRENCE` / `_DETECTION`: codes `1`…`10`; the French
//!   definition is the value's label, `metadata_json` holds the English one and the level's
//!   attribute — the RCM consequence class (severity), the lowest failures per equipment-year
//!   (occurrence), whether the failure is hidden (detection);
//! - `RELIABILITY.FMECA_CRITICALITY_ZONE`: codes `unacceptable` / `reduce` / `monitor` /
//!   `acceptable` with their lowest class product; the matrix class count is the domain's
//!   `validation_rules_json` (`{"matrix_classes":5}`).
//!
//! The domains are seeded on every device (seeded identities agree across machines) from the
//! plant's former setting `ram.fmeca_rating_scales`, else from the product grids below; existing
//! values are never overwritten. When the former setting exists but cannot be read (malformed or
//! incomplete), the product grids are put in place FLAGGED for review (`RatingScales::review`):
//! the grids screen (Données de référence › Échelles AMDEC) says so until a reliability manager
//! reviews and saves them. The grids are edited in
//! Données de référence › Échelles AMDEC (`ram.manage`, as before) and read here by the FMECA
//! and RCM steps.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::settings;

/// Former home of the grids (tenant setting): read once to seed the reference domains.
pub const SETTING_KEY: &str = "ram.fmeca_rating_scales";

pub const SEVERITY_DOMAIN: &str = "RELIABILITY.FMECA_SEVERITY";
pub const OCCURRENCE_DOMAIN: &str = "RELIABILITY.FMECA_OCCURRENCE";
pub const DETECTION_DOMAIN: &str = "RELIABILITY.FMECA_DETECTION";
pub const ZONE_DOMAIN: &str = "RELIABILITY.FMECA_CRITICALITY_ZONE";

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
    /// Set when the grids must be reviewed: `former_grids_unreadable` — the plant's former setting
    /// could not be read, so the product grids were put in place; saving the grids clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
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
        for levels in [&self.severity, &self.occurrence, &self.detection] {
            let mut scores: Vec<i64> = levels.iter().map(|l| l.score).collect();
            scores.sort_unstable();
            if scores != (1..=10).collect::<Vec<_>>() {
                return Err(invalid("RATING_SCALE_SCORES"));
            }
            if levels
                .iter()
                .any(|l| l.label_fr.trim().is_empty() || l.label_en.trim().is_empty())
            {
                return Err(invalid("RATING_SCALE_LABEL"));
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

// ── Storage in Données de référence ────────────────────────────────────────────────────

/// What a level keeps besides its French label.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LevelMeta {
    #[serde(default)]
    label_en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    consequence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_per_year: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_product: Option<i64>,
}

const LEVEL_DOMAINS: [(&str, &str); 3] = [
    (SEVERITY_DOMAIN, "Gravité AMDEC (1–10)"),
    (OCCURRENCE_DOMAIN, "Occurrence AMDEC (1–10)"),
    (DETECTION_DOMAIN, "Détection AMDEC (1–10)"),
];

const ZONE_LABELS: [(&str, &str, &str); 4] = [
    (ZONE_UNACCEPTABLE, "Inacceptable", "Unacceptable"),
    (ZONE_REDUCE, "À réduire", "Reduce"),
    (ZONE_MONITOR, "À surveiller", "Monitor"),
    (ZONE_ACCEPTABLE, "Acceptable", "Acceptable"),
];

fn decode(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("FMECA rating grid row: {e}"))
}

fn seeded_value_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

/// Review flag: the former setting could not be read; the product grids stand in until reviewed.
pub const REVIEW_FORMER_GRIDS_UNREADABLE: &str = "former_grids_unreadable";

/// Grids to seed: the former setting (the plant's edits); the product grids when there is none;
/// the product grids flagged for review when it cannot be read.
async fn seed_source(conn: &impl ConnectionTrait) -> AppResult<RatingScales> {
    let Some(stored) = settings::get_setting(conn, SETTING_KEY, "tenant").await? else {
        return Ok(default_scales());
    };
    match serde_json::from_str::<RatingScales>(&stored.setting_value_json) {
        Ok(grids) if grids.validate().is_ok() => Ok(grids),
        _ => {
            tracing::warn!("ram.fmeca_rating_scales cannot be read: product grids seeded, flagged for review");
            Ok(RatingScales {
                review: Some(REVIEW_FORMER_GRIDS_UNREADABLE.to_string()),
                ..default_scales()
            })
        }
    }
}

/// The zone domain's rules: the matrix class count, and the review flag while one stands.
fn zone_rules(classes: i64, review: Option<&str>) -> String {
    match review {
        Some(r) => serde_json::json!({ "matrix_classes": classes, "review": r }),
        None => serde_json::json!({ "matrix_classes": classes }),
    }
    .to_string()
}

async fn insert_value_if_missing(
    conn: &impl ConnectionTrait,
    domain: &str,
    code: &str,
    label: &str,
    sort_order: i64,
    meta: &LevelMeta,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
          semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, 'fmeca_rating', NULL, 1, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE d.code = ? \
            AND NOT EXISTS ( \
                SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                 WHERE s2.domain_id = d.id AND rv.code = ?) \
          ORDER BY rs.version_no DESC LIMIT 1",
        [
            seeded_value_id(domain, code).into(),
            code.into(),
            label.into(),
            sort_order.into(),
            serde_json::to_string(meta)?.into(),
            domain.into(),
            code.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Ensures the four SYSTEM domains, their published sets and every level (startup integrity).
/// Missing levels come from the former setting or the product grids; existing ones are kept.
pub async fn ensure_fmeca_rating_scales(conn: &impl ConnectionTrait) -> AppResult<()> {
    let source = seed_source(conn).await?;
    for (domain, name) in LEVEL_DOMAINS
        .iter()
        .chain([(ZONE_DOMAIN, "Zones de criticité AMDEC")].iter())
    {
        crate::reference::system_catalog_integrity::ensure_domain(
            conn,
            domain,
            name,
            "flat",
            "system_seeded",
            0,
            DomainAudience::System,
        )
        .await?;
        crate::reference::system_catalog_integrity::ensure_published_set(conn, domain).await?;
    }
    for (domain, levels) in [
        (SEVERITY_DOMAIN, &source.severity),
        (OCCURRENCE_DOMAIN, &source.occurrence),
        (DETECTION_DOMAIN, &source.detection),
    ] {
        for l in levels {
            let meta = LevelMeta {
                label_en: l.label_en.clone(),
                consequence: l.consequence.clone(),
                min_per_year: l.min_per_year,
                hidden: l.hidden,
                min_product: None,
            };
            insert_value_if_missing(conn, domain, &l.score.to_string(), &l.label_fr, 11 - l.score, &meta).await?;
        }
    }
    for (rank, (code, label_fr, label_en)) in ZONE_LABELS.iter().enumerate() {
        let min_product = source
            .matrix
            .zones
            .iter()
            .find(|z| z.zone == *code)
            .map(|z| z.min_product);
        let Some(min_product) = min_product else { continue };
        let meta = LevelMeta {
            label_en: (*label_en).to_string(),
            min_product: Some(min_product),
            ..LevelMeta::default()
        };
        insert_value_if_missing(
            conn,
            ZONE_DOMAIN,
            code,
            label_fr,
            i64::try_from(rank).unwrap_or(0) + 1,
            &meta,
        )
        .await?;
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET validation_rules_json = ? WHERE code = ? AND validation_rules_json IS NULL",
        [
            zone_rules(source.matrix.classes, source.review.as_deref()).into(),
            ZONE_DOMAIN.into(),
        ],
    ))
    .await?;
    Ok(())
}

const VALUES_SELECT: &str = "SELECT rv.id, rv.code, rv.label, rv.metadata_json FROM reference_values rv \
       JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
       JOIN reference_domains d ON d.id = rs.domain_id \
      WHERE d.code = ? AND rv.is_active = 1";

struct StoredValue {
    id: i64,
    code: String,
    label: String,
    meta: LevelMeta,
}

async fn stored_values(conn: &impl ConnectionTrait, domain: &str) -> AppResult<Vec<StoredValue>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            VALUES_SELECT,
            [domain.into()],
        ))
        .await?;
    rows.iter()
        .map(|r: &QueryResult| {
            let raw: Option<String> = r.try_get("", "metadata_json").map_err(decode)?;
            Ok(StoredValue {
                id: r.try_get("", "id").map_err(decode)?,
                code: r.try_get("", "code").map_err(decode)?,
                label: r.try_get("", "label").map_err(decode)?,
                meta: match raw.as_deref() {
                    Some(m) => serde_json::from_str(m).map_err(|_| invalid("RATING_SCALE_SETTING_INVALID"))?,
                    None => LevelMeta::default(),
                },
            })
        })
        .collect()
}

/// The plant's grids, from Données de référence. A missing or unreadable grid is refused (never
/// replaced by product values without saying so).
pub async fn load_scales(db: &DatabaseConnection) -> AppResult<RatingScales> {
    let mut grids = Vec::with_capacity(3);
    for (domain, _) in LEVEL_DOMAINS {
        let mut levels = Vec::new();
        for v in stored_values(db, domain).await? {
            let score: i64 = v
                .code
                .trim()
                .parse()
                .map_err(|_| invalid("RATING_SCALE_SETTING_INVALID"))?;
            levels.push(RatingLevel {
                score,
                label_fr: v.label,
                label_en: v.meta.label_en,
                consequence: v.meta.consequence,
                min_per_year: v.meta.min_per_year,
                hidden: v.meta.hidden,
            });
        }
        levels.sort_by_key(|l| std::cmp::Reverse(l.score));
        grids.push(levels);
    }
    let mut zones = stored_values(db, ZONE_DOMAIN)
        .await?
        .into_iter()
        .map(|v| {
            Ok(MatrixZone {
                min_product: v.meta.min_product.ok_or_else(|| invalid("RATING_SCALE_MATRIX"))?,
                zone: v.code,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    // Highest threshold first, as the product grids list them.
    zones.sort_by_key(|z| std::cmp::Reverse(z.min_product));
    let rules = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT validation_rules_json FROM reference_domains WHERE code = ?",
            [ZONE_DOMAIN.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "validation_rules_json").ok().flatten())
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
        .ok_or_else(|| invalid("RATING_SCALE_MATRIX"))?;
    let classes = rules
        .get("matrix_classes")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| invalid("RATING_SCALE_MATRIX"))?;
    let review = rules
        .get("review")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let mut it = grids.into_iter();
    let scales = RatingScales {
        version: 1,
        review,
        severity: it.next().unwrap_or_default(),
        occurrence: it.next().unwrap_or_default(),
        detection: it.next().unwrap_or_default(),
        matrix: MatrixSpec { classes, zones },
    };
    scales.validate()?;
    Ok(scales)
}

/// Saves the grids' company attributes (labels, consequence, thresholds, hidden flag, zone
/// thresholds). Scores, zone codes and the class count are fixed by the product.
pub async fn save_scales(db: &DatabaseConnection, scales: &RatingScales, _actor_id: i32) -> AppResult<RatingScales> {
    scales.validate()?;
    let current = load_scales(db).await?;
    if scales.matrix.classes != current.matrix.classes {
        return Err(invalid("RATING_SCALE_MATRIX"));
    }
    let tx = db.begin().await?;
    for (domain, levels) in [
        (SEVERITY_DOMAIN, &scales.severity),
        (OCCURRENCE_DOMAIN, &scales.occurrence),
        (DETECTION_DOMAIN, &scales.detection),
    ] {
        let stored = stored_values(&tx, domain).await?;
        for l in levels {
            let v = stored
                .iter()
                .find(|v| v.code.trim() == l.score.to_string())
                .ok_or_else(|| invalid("RATING_SCALE_SETTING_INVALID"))?;
            let meta = LevelMeta {
                label_en: l.label_en.trim().to_string(),
                consequence: l.consequence.clone(),
                min_per_year: l.min_per_year,
                hidden: l.hidden,
                min_product: None,
            };
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE reference_values SET label = ?, metadata_json = ? WHERE id = ?",
                [
                    l.label_fr.trim().into(),
                    serde_json::to_string(&meta)?.into(),
                    v.id.into(),
                ],
            ))
            .await?;
        }
    }
    let stored_zones = stored_values(&tx, ZONE_DOMAIN).await?;
    for z in &scales.matrix.zones {
        let v = stored_zones
            .iter()
            .find(|v| v.code == z.zone)
            .ok_or_else(|| invalid("RATING_SCALE_MATRIX"))?;
        let meta = LevelMeta {
            min_product: Some(z.min_product),
            ..v.meta.clone()
        };
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET metadata_json = ? WHERE id = ?",
            [serde_json::to_string(&meta)?.into(), v.id.into()],
        ))
        .await?;
    }
    // Saved by a reliability manager: the grids are reviewed.
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET validation_rules_json = ? WHERE code = ?",
        [zone_rules(current.matrix.classes, None).into(), ZONE_DOMAIN.into()],
    ))
    .await?;
    tx.commit().await?;
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

    async fn fresh_db() -> DatabaseConnection {
        let db = crate::db::test_db::seeded().await;
        // Startup integrity may already have seeded the grids: start from empty domains.
        db.execute_unprepared(
            "DELETE FROM reference_values WHERE set_id IN (SELECT rs.id FROM reference_sets rs              JOIN reference_domains d ON d.id = rs.domain_id WHERE d.code LIKE 'RELIABILITY.FMECA_%')",
        )
        .await
        .expect("clear");
        db.execute_unprepared(
            "UPDATE reference_domains SET validation_rules_json = NULL WHERE code LIKE 'RELIABILITY.FMECA_%'",
        )
        .await
        .expect("clear rules");
        db
    }

    #[tokio::test]
    async fn grids_live_in_reference_data_seeded_from_the_former_setting() {
        let db = fresh_db().await;
        // The plant had edited its grids in the former setting: they are kept.
        let mut edited = default_scales();
        edited.severity[0].label_fr = "Accident mortel possible".into();
        settings::set_setting(
            &db,
            SETTING_KEY,
            "tenant",
            &serde_json::to_string(&edited).unwrap(),
            None,
            "test",
        )
        .await
        .expect("setting");
        ensure_fmeca_rating_scales(&db).await.expect("ensure");
        ensure_fmeca_rating_scales(&db).await.expect("idempotent");
        let loaded = load_scales(&db).await.expect("load");
        assert_eq!(
            loaded.severity_level(10).map(|l| l.label_fr.as_str()),
            Some("Accident mortel possible")
        );
        assert_eq!(loaded.occurrence_score(1.0), 6);
        assert_eq!(loaded.matrix, default_scales().matrix);
        assert_eq!(loaded.zone(8, 6), ZONE_REDUCE);
    }

    #[tokio::test]
    async fn saved_grids_are_read_back_and_the_class_count_is_fixed() {
        let db = fresh_db().await;
        ensure_fmeca_rating_scales(&db).await.expect("ensure");
        let mut s = load_scales(&db).await.expect("load");
        s.detection[0].label_en = "Hidden until it fails".into();
        let six = s.occurrence.iter_mut().find(|l| l.score == 6).expect("level 6");
        six.min_per_year = Some(0.8);
        let saved = save_scales(&db, &s, 1).await.expect("save");
        assert_eq!(
            saved.detection_level(10).map(|l| l.label_en.as_str()),
            Some("Hidden until it fails")
        );
        assert_eq!(saved.occurrence_score(0.75), 5);
        let mut other = saved.clone();
        other.matrix.classes = 4;
        assert!(save_scales(&db, &other, 1).await.is_err(), "the class count is fixed");
    }

    #[tokio::test]
    async fn an_unreadable_former_setting_seeds_product_grids_flagged_for_review() {
        let db = fresh_db().await;
        settings::set_setting(
            &db,
            SETTING_KEY,
            "tenant",
            r#"{"version":1,"severity":[]}"#,
            None,
            "test",
        )
        .await
        .expect("setting");
        ensure_fmeca_rating_scales(&db).await.expect("ensure");
        let loaded = load_scales(&db).await.expect("load");
        assert_eq!(loaded.review.as_deref(), Some(REVIEW_FORMER_GRIDS_UNREADABLE));
        assert_eq!(loaded.severity, default_scales().severity);
        // A reliability manager reviews and saves: the flag is cleared.
        let saved = save_scales(&db, &loaded, 1).await.expect("save");
        assert_eq!(saved.review, None);
    }

    #[tokio::test]
    async fn a_missing_grid_is_refused_not_replaced() {
        let db = fresh_db().await;
        assert!(load_scales(&db).await.is_err());
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
