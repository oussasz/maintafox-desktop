//! Shared urgency / priority scale of requests (DI) and work orders (OT): `WORK.URGENCY`.
//!
//! One five-level industrial scale, used by the request intake, the request SLA, the work-order
//! planning and backlog, the analytics and the inspection findings:
//!
//! | rank | code        | FR (product label)        | EN                     |
//! |------|-------------|---------------------------|------------------------|
//! | 1    | `emergency` | Urgence (arrêt immédiat)  | Emergency              |
//! | 2    | `urgent`    | Urgent                    | Urgent                 |
//! | 3    | `high`      | Haute                     | High                   |
//! | 4    | `normal`    | Normale / planifiée       | Normal / planned       |
//! | 5    | `low`       | Basse / opportunité       | Low / opportunity      |
//!
//! Rank 1 is the most urgent (P1 … P5). The codes are fixed by Maintafox: the SLA engine, the
//! planning order and the analytics depend on them, so the domain is a SYSTEM domain
//! (`reference::audience`): no level can be added, removed, recoded or deactivated, and the
//! domain does not appear in the generic Données de référence editor.
//!
//! **Company attributes.** What differs from one plant to the next is declared here as the
//! company-editable attributes of each level, maintained through [`update_urgency_level`] (the
//! "Échelle d'urgence" screen, permission `ref.manage`) and nowhere else:
//! - the French label (`reference_values.label`) and the English label (`metadata_json.label_en`);
//! - the target response time, target completion time and escalation (at-risk) threshold, in
//!   hours (`metadata_json.response_target_hours` / `completion_target_hours` /
//!   `escalation_threshold_hours`).
//!
//! The product ships a starter set of target times, flagged `targets_source = "starter"` until
//! the plant saves the level (then `"company"`). The emergency / high / normal / low starter times
//! are the request SLA times Maintafox already shipped for critical / high / medium / low
//! (1 h / 8 h, 4 h / 24 h, 24 h / 72 h, 72 h / 168 h); `urgent` had no earlier value and gets
//! 2 h / 16 h, between emergency and high. The seeder only inserts missing levels; it never
//! rewrites labels or targets a plant has set.
//!
//! **Legacy codes.** Before migration 198, OT used `urgency_levels` (very_low … critical) and DI
//! the governed `DI.PRIORITY` list (low / medium / high / critical). Both map to this scale by
//! [`legacy_urgency_code`].

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;

/// Reference domain of the shared urgency scale.
pub const URGENCY_DOMAIN: &str = "WORK.URGENCY";

/// Stable gate code of a refused urgency-scale change (translated under `errors:gateError`).
pub const GATE_URGENCY_SCALE_INVALID: &str = "GATE_URGENCY_SCALE_INVALID";

/// One level of the shared scale. The order of the variants is the rank (most urgent first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrgencyCode {
    Emergency,
    Urgent,
    High,
    Normal,
    Low,
}

impl UrgencyCode {
    pub const ALL: [UrgencyCode; 5] = [Self::Emergency, Self::Urgent, Self::High, Self::Normal, Self::Low];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emergency => "emergency",
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    /// 1 = most urgent … 5 = least urgent.
    pub const fn rank(self) -> i64 {
        match self {
            Self::Emergency => 1,
            Self::Urgent => 2,
            Self::High => 3,
            Self::Normal => 4,
            Self::Low => 5,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "emergency" => Some(Self::Emergency),
            "urgent" => Some(Self::Urgent),
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

impl std::fmt::Display for UrgencyCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Mapping of the codes stored before migration 198 (OT `urgency_levels.code`, DI `DI.PRIORITY`
/// codes, PM finding severities) to the shared scale, by meaning of the old label:
///
/// | old code (OT label / DI label)            | shared level |
/// |-------------------------------------------|--------------|
/// | `critical` (OT « Urgente », DI « Critique ») | `emergency` |
/// | `high` (« Haute »)                         | `high`       |
/// | `medium` (« Normale »)                     | `normal`     |
/// | `low` (« Basse »)                          | `low`        |
/// | `very_low` (OT « Très basse »)             | `low`        |
///
/// A code that already belongs to the shared scale maps to itself. `urgent` has no older
/// equivalent: it is a new level between emergency and high.
pub fn legacy_urgency_code(old: &str) -> Option<UrgencyCode> {
    match old.trim().to_ascii_lowercase().as_str() {
        "critical" => Some(UrgencyCode::Emergency),
        "high" => Some(UrgencyCode::High),
        "medium" => Some(UrgencyCode::Normal),
        "low" | "very_low" => Some(UrgencyCode::Low),
        other => UrgencyCode::parse(other),
    }
}

/// Urgency of a request or work order raised from an inspection finding of severity 1 … 5
/// (risk-based prioritisation: one severity step per urgency level).
pub fn severity_urgency(severity: i64) -> UrgencyCode {
    match severity {
        i64::MIN..=1 => UrgencyCode::Low,
        2 => UrgencyCode::Normal,
        3 => UrgencyCode::High,
        4 => UrgencyCode::Urgent,
        _ => UrgencyCode::Emergency,
    }
}

/// Product definition of a level: identity, product labels, colour and starter target times.
struct LevelSeed {
    code: UrgencyCode,
    label_fr: &'static str,
    label_en: &'static str,
    color_hex: &'static str,
    description: &'static str,
    response_h: i64,
    completion_h: i64,
    escalation_h: i64,
}

const LEVEL_SEEDS: &[LevelSeed] = &[
    LevelSeed {
        code: UrgencyCode::Emergency,
        label_fr: "Urgence",
        label_en: "Emergency",
        color_hex: "#DC2626",
        description: "Danger pour les personnes ou l'environnement, ou arrêt de production : intervention immédiate.",
        response_h: 1,
        completion_h: 8,
        escalation_h: 4,
    },
    LevelSeed {
        code: UrgencyCode::Urgent,
        label_fr: "Urgent",
        label_en: "Urgent",
        color_hex: "#F97316",
        description: "Risque de panne ou de perte de production à court terme : à traiter dans la journée.",
        response_h: 2,
        completion_h: 16,
        escalation_h: 8,
    },
    LevelSeed {
        code: UrgencyCode::High,
        label_fr: "Haute",
        label_en: "High",
        color_hex: "#F59E0B",
        description: "Dégradation constatée : à planifier en priorité dans le prochain programme.",
        response_h: 4,
        completion_h: 24,
        escalation_h: 8,
    },
    LevelSeed {
        code: UrgencyCode::Normal,
        label_fr: "Normale",
        label_en: "Normal",
        color_hex: "#3B82F6",
        description: "Travail planifié dans le programme habituel.",
        response_h: 24,
        completion_h: 72,
        escalation_h: 48,
    },
    LevelSeed {
        code: UrgencyCode::Low,
        label_fr: "Basse",
        label_en: "Low",
        color_hex: "#64748B",
        description: "À réaliser à l'occasion (arrêt programmé, temps disponible).",
        response_h: 72,
        completion_h: 168,
        escalation_h: 120,
    },
];

/// Where the target times of a level come from.
pub const TARGETS_SOURCE_STARTER: &str = "starter";
pub const TARGETS_SOURCE_COMPANY: &str = "company";
/// Copied by migration 198 from the request SLA rule that covered every origin and criticality.
pub const TARGETS_SOURCE_DI_SLA_RULE: &str = "di_sla_rule";

/// `metadata_json` of a level: origin marker plus the company attributes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LevelMetadata {
    pub origin: String,
    pub label_en: String,
    pub response_target_hours: Option<i64>,
    pub completion_target_hours: Option<i64>,
    pub escalation_threshold_hours: Option<i64>,
    pub targets_source: String,
}

impl LevelMetadata {
    fn to_json(&self) -> AppResult<String> {
        serde_json::to_string(self).map_err(|e| AppError::Internal(anyhow::anyhow!("urgency metadata: {e}")))
    }
}

fn seed_metadata(seed: &LevelSeed) -> LevelMetadata {
    LevelMetadata {
        origin: "system".into(),
        label_en: seed.label_en.into(),
        response_target_hours: Some(seed.response_h),
        completion_target_hours: Some(seed.completion_h),
        escalation_threshold_hours: Some(seed.escalation_h),
        targets_source: TARGETS_SOURCE_STARTER.into(),
    }
}

fn seeded_sync_id(code: UrgencyCode) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{URGENCY_DOMAIN}:{}", code.as_str()))
}

/// Ensures the SYSTEM domain, its published set and the five levels exist. Missing levels are
/// inserted with the product labels and the starter targets; existing levels are never touched.
pub async fn ensure_urgency_scale(conn: &impl ConnectionTrait) -> AppResult<()> {
    crate::reference::system_catalog_integrity::ensure_domain(
        conn,
        URGENCY_DOMAIN,
        "Échelle d'urgence (DI / OT)",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    crate::reference::system_catalog_integrity::ensure_published_set(conn, URGENCY_DOMAIN).await?;
    for seed in LEVEL_SEEDS {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values \
             (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
              semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, ?, ?, ?, NULL, 'urgency_level', NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
                AND NOT EXISTS ( \
                    SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                     WHERE s2.domain_id = d.id AND LOWER(TRIM(rv.code)) = ?) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [
                seeded_sync_id(seed.code).into(),
                seed.code.as_str().into(),
                seed.label_fr.into(),
                seed.description.into(),
                seed.code.rank().into(),
                seed.color_hex.into(),
                seed_metadata(seed).to_json()?.into(),
                URGENCY_DOMAIN.into(),
                seed.code.as_str().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

// ─── Reads ──────────────────────────────────────────────────────────────────────

/// One level as the screens, the SLA engine and the planning read it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UrgencyLevel {
    /// `reference_values.id` (what `work_orders.urgency_id` and `schedule_candidates.priority_id` hold).
    pub id: i64,
    pub code: String,
    /// 1 = most urgent … 5 = least urgent.
    pub rank: i64,
    pub label_fr: String,
    pub label_en: String,
    pub description: Option<String>,
    pub color_hex: Option<String>,
    pub response_target_hours: Option<i64>,
    pub completion_target_hours: Option<i64>,
    pub escalation_threshold_hours: Option<i64>,
    /// `starter` (product values, to review), `company` (saved by the plant) or `di_sla_rule`.
    pub targets_source: String,
}

const LEVEL_SELECT: &str =
    "SELECT rv.id, rv.code, rv.sort_order, rv.label, rv.description, rv.color_hex, rv.metadata_json \
       FROM reference_values rv \
       JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
       JOIN reference_domains d ON d.id = rs.domain_id \
      WHERE UPPER(TRIM(d.code)) = 'WORK.URGENCY'";

fn decode(e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WORK.URGENCY row decode: {e}"))
}

fn map_level(row: &QueryResult) -> AppResult<UrgencyLevel> {
    let code: String = row.try_get("", "code").map_err(decode)?;
    let parsed = UrgencyCode::parse(&code)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("WORK.URGENCY holds an unknown level '{code}'")))?;
    let raw_meta: Option<String> = row.try_get("", "metadata_json").map_err(decode)?;
    let meta: Option<LevelMetadata> = raw_meta.as_deref().and_then(|m| serde_json::from_str(m).ok());
    let label_fr: String = row.try_get("", "label").map_err(decode)?;
    Ok(UrgencyLevel {
        id: row.try_get("", "id").map_err(decode)?,
        code: parsed.as_str().to_string(),
        rank: parsed.rank(),
        label_en: meta
            .as_ref()
            .map(|m| m.label_en.clone())
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| label_fr.clone()),
        label_fr,
        description: row.try_get("", "description").map_err(decode)?,
        color_hex: row.try_get("", "color_hex").map_err(decode)?,
        response_target_hours: meta.as_ref().and_then(|m| m.response_target_hours),
        completion_target_hours: meta.as_ref().and_then(|m| m.completion_target_hours),
        escalation_threshold_hours: meta.as_ref().and_then(|m| m.escalation_threshold_hours),
        targets_source: meta
            .map(|m| m.targets_source)
            .unwrap_or_else(|| TARGETS_SOURCE_STARTER.to_string()),
    })
}

/// The five levels, most urgent first.
pub async fn list_urgency_levels(conn: &impl ConnectionTrait) -> AppResult<Vec<UrgencyLevel>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{LEVEL_SELECT} ORDER BY rv.sort_order ASC, rv.id ASC"),
        ))
        .await?;
    let mut out = rows.iter().map(map_level).collect::<AppResult<Vec<_>>>()?;
    out.sort_by_key(|l| l.rank);
    Ok(out)
}

/// The level of `code`, `None` when the code is not a level of the scale.
pub async fn get_urgency_level(conn: &impl ConnectionTrait, code: &str) -> AppResult<Option<UrgencyLevel>> {
    let Some(parsed) = UrgencyCode::parse(code) else {
        return Ok(None);
    };
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LEVEL_SELECT} AND LOWER(TRIM(rv.code)) = ? ORDER BY rs.version_no DESC LIMIT 1"),
            [parsed.as_str().into()],
        ))
        .await?;
    row.as_ref().map(map_level).transpose()
}

/// The level stored under `reference_values.id = id`, `None` when the id is not a level.
pub async fn get_urgency_level_by_id(conn: &impl ConnectionTrait, id: i64) -> AppResult<Option<UrgencyLevel>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LEVEL_SELECT} AND rv.id = ?"),
            [id.into()],
        ))
        .await?;
    row.as_ref().map(map_level).transpose()
}

fn unknown_level(raw: &str) -> AppError {
    AppError::ValidationFailed(vec![format!(
        "{GATE_URGENCY_SCALE_INVALID}: '{raw}' n'est pas un niveau de l'échelle d'urgence \
         (emergency, urgent, high, normal, low)."
    )])
}

/// Validates a level code given by a user or a caller and returns it normalised.
pub fn validate_urgency_code(raw: &str) -> AppResult<UrgencyCode> {
    UrgencyCode::parse(raw).ok_or_else(|| unknown_level(raw))
}

/// `reference_values.id` of a level (what work orders store).
pub async fn urgency_value_id(conn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    get_urgency_level(conn, code)
        .await?
        .map(|l| l.id)
        .ok_or_else(|| unknown_level(code))
}

/// Validates an `urgency_id` stored on a work order or a planning candidate.
pub async fn assert_urgency_value_id(conn: &impl ConnectionTrait, id: i64) -> AppResult<UrgencyLevel> {
    get_urgency_level_by_id(conn, id)
        .await?
        .ok_or_else(|| unknown_level(&format!("id={id}")))
}

// ─── Company attributes ─────────────────────────────────────────────────────────

/// What a plant may change on a level. Every field is required: the screen sends the full level.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUrgencyLevelInput {
    pub label_fr: String,
    pub label_en: String,
    pub response_target_hours: i64,
    pub completion_target_hours: i64,
    pub escalation_threshold_hours: i64,
}

fn refuse(message: impl Into<String>) -> AppError {
    AppError::ValidationFailed(vec![format!("{GATE_URGENCY_SCALE_INVALID}: {}", message.into())])
}

fn clean_label(raw: &str, field: &str) -> AppResult<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(refuse(format!("{field} obligatoire.")));
    }
    if s.chars().count() > 80 {
        return Err(refuse(format!("{field} trop long (80 caractères maximum).")));
    }
    Ok(s.to_string())
}

/// Saves the company attributes of one level. Codes, ranks, colours and the number of levels are
/// fixed by the product. The target times must be positive, the response target must not exceed
/// the completion target, the escalation threshold must fall within the completion target, and a
/// more urgent level may not get longer targets than a less urgent one.
pub async fn update_urgency_level(
    db: &DatabaseConnection,
    code: &str,
    input: UpdateUrgencyLevelInput,
) -> AppResult<UrgencyLevel> {
    let level_code = validate_urgency_code(code)?;
    let label_fr = clean_label(&input.label_fr, "Libellé (FR)")?;
    let label_en = clean_label(&input.label_en, "Label (EN)")?;
    let (response, completion, escalation) = (
        input.response_target_hours,
        input.completion_target_hours,
        input.escalation_threshold_hours,
    );
    if response <= 0 || completion <= 0 || escalation <= 0 {
        return Err(refuse("Les délais cibles doivent être supérieurs à zéro."));
    }
    if response > completion {
        return Err(refuse(
            "Le délai de prise en charge ne peut pas dépasser le délai de réalisation.",
        ));
    }
    if escalation > completion {
        return Err(refuse(
            "Le seuil d'alerte doit être atteint avant le délai de réalisation.",
        ));
    }

    let levels = list_urgency_levels(db).await?;
    let current = levels
        .iter()
        .find(|l| l.code == level_code.as_str())
        .ok_or_else(|| unknown_level(code))?;
    for other in levels.iter().filter(|l| l.code != current.code) {
        let (Some(o_resp), Some(o_comp)) = (other.response_target_hours, other.completion_target_hours) else {
            continue;
        };
        let more_urgent = other.rank < current.rank;
        let inverted = if more_urgent {
            o_resp > response || o_comp > completion
        } else {
            o_resp < response || o_comp < completion
        };
        if inverted {
            return Err(refuse(format!(
                "Les délais doivent croître de l'urgence la plus forte à la plus faible : \
                 le niveau « {} » a {} h / {} h.",
                other.label_fr, o_resp, o_comp
            )));
        }
    }

    let meta = LevelMetadata {
        origin: "system".into(),
        label_en,
        response_target_hours: Some(response),
        completion_target_hours: Some(completion),
        escalation_threshold_hours: Some(escalation),
        targets_source: TARGETS_SOURCE_COMPANY.into(),
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET label = ?, metadata_json = ? WHERE id = ?",
        [label_fr.into(), meta.to_json()?.into(), current.id.into()],
    ))
    .await?;

    get_urgency_level_by_id(db, current.id)
        .await?
        .ok_or_else(|| unknown_level(code))
}

/// Target times of a level as stored, for the SLA fallback and the work-order due dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelTargets {
    pub response_hours: i64,
    pub completion_hours: i64,
    pub escalation_hours: i64,
}

impl UrgencyLevel {
    /// The three target times, when all are set.
    pub fn targets(&self) -> Option<LevelTargets> {
        Some(LevelTargets {
            response_hours: self.response_target_hours?,
            completion_hours: self.completion_target_hours?,
            escalation_hours: self.escalation_threshold_hours?,
        })
    }
}

/// Writes the target times of a level from migration code (no validation of the neighbours:
/// the values come from an existing request SLA rule).
pub(crate) async fn set_level_targets_from_migration(
    conn: &impl ConnectionTrait,
    code: UrgencyCode,
    targets: LevelTargets,
    source: &str,
) -> AppResult<()> {
    let Some(level) = get_urgency_level(conn, code.as_str()).await? else {
        return Err(unknown_level(code.as_str()));
    };
    let meta = LevelMetadata {
        origin: "system".into(),
        label_en: level.label_en.clone(),
        response_target_hours: Some(targets.response_hours),
        completion_target_hours: Some(targets.completion_hours),
        escalation_threshold_hours: Some(targets.escalation_hours),
        targets_source: source.to_string(),
    };
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET metadata_json = ? WHERE id = ?",
        [meta.to_json()?.into(), level.id.into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_codes_map_by_meaning() {
        assert_eq!(legacy_urgency_code("critical"), Some(UrgencyCode::Emergency));
        assert_eq!(legacy_urgency_code("high"), Some(UrgencyCode::High));
        assert_eq!(legacy_urgency_code("medium"), Some(UrgencyCode::Normal));
        assert_eq!(legacy_urgency_code("low"), Some(UrgencyCode::Low));
        assert_eq!(legacy_urgency_code("very_low"), Some(UrgencyCode::Low));
        assert_eq!(legacy_urgency_code("urgent"), Some(UrgencyCode::Urgent));
        assert_eq!(legacy_urgency_code("whatever"), None);
    }

    #[test]
    fn severity_uses_the_whole_scale() {
        let mapped: Vec<_> = (1..=5).map(severity_urgency).collect();
        assert_eq!(
            mapped,
            vec![
                UrgencyCode::Low,
                UrgencyCode::Normal,
                UrgencyCode::High,
                UrgencyCode::Urgent,
                UrgencyCode::Emergency
            ]
        );
    }

    #[test]
    fn ranks_follow_declaration_order() {
        for (i, c) in UrgencyCode::ALL.iter().enumerate() {
            assert_eq!(c.rank(), i as i64 + 1);
            assert_eq!(UrgencyCode::parse(c.as_str()), Some(*c));
        }
    }

    #[test]
    fn starter_targets_increase_with_rank() {
        for pair in LEVEL_SEEDS.windows(2) {
            assert!(pair[0].code.rank() < pair[1].code.rank());
            assert!(pair[0].response_h <= pair[1].response_h);
            assert!(pair[0].completion_h <= pair[1].completion_h);
            assert!(pair[0].escalation_h <= pair[0].completion_h);
        }
    }
}
