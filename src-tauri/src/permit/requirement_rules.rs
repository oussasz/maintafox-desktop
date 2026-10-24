//! Which work needs a permit — the plant's rules, applied to every work order when it is created,
//! whatever creates it: a request (DI) converted, a direct creation, a PM occurrence, an inspection
//! anomaly or a reliability action (owner decision D2 (a), audit finding AU-04: the hazard comes
//! from the equipment and the job, not from how the work order was created). Requests carry no
//! permit field; the work order is flagged (`di::conversion`, `wo::queries::create_work_order_in`).
//! A received work order new here that one of these rules flags here but that arrives without the
//! flag is re-checked (`permit::sync_check::inbound_permit_rule_check`).
//!
//! `PERMIT.REQUIREMENT_RULES` is a COMPANY list of Données de référence. Each value is one rule;
//! its `metadata_json` says what it matches and which permit it calls for:
//!
//! ```json
//! { "equipment_class_code": "PUMP", "work_type_code": "corrective_immediate", "permit_type_code": "HOT_WORK" }
//! ```
//!
//! - `equipment_class_code`: a code of the published `EQUIPMENT.CLASS` list (optional);
//! - `work_type_code`: a code of the published `WORK.ORDER_TYPES` list (optional);
//! - at least one of the two; a rule naming both matches only when both match;
//! - `permit_type_code`: a permit type (`permit_types.code`), required.
//!
//! The list ships empty: with no rule nothing is flagged by a rule, and the conversion says so
//! (`PERMIT_RULES_NONE_CONFIGURED`). A rule is checked when it is saved and when its list is
//! validated for publishing; a published rule that no longer reads (received from another PC,
//! or a class removed since) is reported by the conversion, never guessed.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const DOMAIN: &str = "PERMIT.REQUIREMENT_RULES";

/// Stable code: a rule that does not say what it matches or which permit it needs.
pub const CODE_RULE_INVALID: &str = "PERMIT_RULE_INVALID";

/// Conversion outcome codes (`PermitFlagOutcome::reason_code`).
pub mod reasons {
    /// A rule matched: the work order needs a permit.
    pub const RULE_MATCHED: &str = "PERMIT_RULE_MATCHED";
    /// Rules exist but none matches this equipment class and work type.
    pub const NO_RULE_MATCHED: &str = "PERMIT_RULES_NO_MATCH";
    /// The plant has no rule: nothing is flagged by a rule.
    pub const NONE_CONFIGURED: &str = "PERMIT_RULES_NONE_CONFIGURED";
}

pub fn is_rules_domain(domain_code: &str) -> bool {
    domain_code.trim().eq_ignore_ascii_case(DOMAIN)
}

/// The list (COMPANY, empty) with its published set. Idempotent (integrity pass, migration 233).
pub async fn ensure_permit_requirement_rules(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        DOMAIN,
        "Règles d'exigence de permis",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, DOMAIN).await
}

/// What a rule matches and the permit it calls for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSpec {
    pub equipment_class_code: Option<String>,
    pub work_type_code: Option<String>,
    pub permit_type_code: String,
}

fn text(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The rule written in `metadata_json`, or why it does not read.
pub fn parse_rule(metadata_json: Option<&str>) -> Result<RuleSpec, String> {
    let obj = metadata_json
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| "the rule has no settings".to_string())?;
    let equipment_class_code = text(&obj, "equipment_class_code");
    let work_type_code = text(&obj, "work_type_code");
    if equipment_class_code.is_none() && work_type_code.is_none() {
        return Err("the rule names neither an equipment class nor a work type".into());
    }
    let permit_type_code = text(&obj, "permit_type_code").ok_or_else(|| "the rule names no permit type".to_string())?;
    Ok(RuleSpec {
        equipment_class_code,
        work_type_code,
        permit_type_code,
    })
}

async fn published_code_exists(db: &impl ConnectionTrait, domain: &str, code: &str) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rv.code = ? AND rv.is_active = 1",
            [domain.into(), code.into()],
        ))
        .await?
        .is_some())
}

/// Why a rule cannot be used (None when it reads and every code it names exists).
pub async fn rule_problem(db: &impl ConnectionTrait, metadata_json: Option<&str>) -> AppResult<Option<String>> {
    let rule = match parse_rule(metadata_json) {
        Ok(r) => r,
        Err(e) => return Ok(Some(e)),
    };
    if let Some(class) = &rule.equipment_class_code {
        if !published_code_exists(db, crate::assets::taxonomy_reference::DOMAIN_CLASS, class).await? {
            return Ok(Some(format!(
                "equipment class '{class}' is not in the published class list"
            )));
        }
    }
    if let Some(work_type) = &rule.work_type_code {
        if !published_code_exists(db, crate::wo::types::WORK_ORDER_TYPES_DOMAIN, work_type).await? {
            return Ok(Some(format!(
                "work type '{work_type}' is not in the published work type list"
            )));
        }
    }
    let permit_known = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM permit_types WHERE code = ?",
            [rule.permit_type_code.clone().into()],
        ))
        .await?
        .is_some();
    if !permit_known {
        return Ok(Some(format!("permit type '{}' does not exist", rule.permit_type_code)));
    }
    Ok(None)
}

/// A rule as saved (editor): it must read and name existing codes. Returns the canonical JSON.
pub async fn checked_rule_metadata(
    db: &impl ConnectionTrait,
    code: &str,
    metadata_json: Option<&str>,
) -> AppResult<String> {
    if let Some(problem) = rule_problem(db, metadata_json).await? {
        return Err(org_validation_failed(
            CODE_RULE_INVALID,
            format!("Permit rule '{code}': {problem}."),
            crate::errors::issue_params(&[("code", code.to_string()), ("detail", problem)]),
        ));
    }
    let rule = parse_rule(metadata_json).map_err(|e| crate::errors::AppError::ValidationFailed(vec![e]))?;
    // Keep whatever else the plant wrote (labels, notes); write the three keys canonically.
    let mut obj = metadata_json
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert(
        "equipment_class_code".into(),
        rule.equipment_class_code
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    obj.insert(
        "work_type_code".into(),
        rule.work_type_code
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    obj.insert(
        "permit_type_code".into(),
        serde_json::Value::String(rule.permit_type_code),
    );
    Ok(serde_json::Value::Object(obj).to_string())
}

/// What the conversion decided about the permit, for the conversion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermitFlagOutcome {
    /// The new work order requires a permit (a rule matched, or the request's safety flag).
    pub requires_permit: bool,
    /// What the rules said: `PERMIT_RULE_MATCHED`, `PERMIT_RULES_NO_MATCH` or
    /// `PERMIT_RULES_NONE_CONFIGURED`.
    pub reason_code: String,
    /// The rule that matched (its code in `PERMIT.REQUIREMENT_RULES`).
    pub rule_code: Option<String>,
    /// The permit type the matched rule calls for.
    pub permit_type_code: Option<String>,
    /// The request was flagged as a safety concern (this flags the work order too, as before).
    pub safety_flag: bool,
    /// Published rules that could not be read or name codes that no longer exist: not applied.
    pub unreadable_rule_codes: Vec<String>,
}

/// The equipment's class code: its `EQUIPMENT.CLASS` value, else its legacy class.
async fn equipment_class_code(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Option<String>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(rc.code, ec.code) AS code FROM equipment e \
               LEFT JOIN reference_values rc ON rc.id = e.equipment_class_ref_id \
               LEFT JOIN equipment_classes ec ON ec.id = e.class_id \
              WHERE e.id = ?",
            [equipment_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "code").ok().flatten()))
}

/// Apply the plant's rules to a new work order: its equipment and work type. The single evaluation
/// for every origin (conversion, direct creation, PM, inspection anomaly, reliability action) and
/// for the re-check of a received work order. `safety_flag` (the source request's) keeps the
/// requirement whatever the rules say: a rule only adds one.
pub async fn evaluate(
    db: &impl ConnectionTrait,
    equipment_id: Option<i64>,
    work_type_id: i64,
    safety_flag: bool,
) -> AppResult<PermitFlagOutcome> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.metadata_json FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rv.is_active = 1 \
                AND rs.id = (SELECT rs2.id FROM reference_sets rs2 \
                              WHERE rs2.domain_id = d.id AND rs2.status = 'published' \
                              ORDER BY rs2.version_no DESC LIMIT 1) \
              ORDER BY rv.sort_order ASC, rv.id ASC",
            [DOMAIN.into()],
        ))
        .await?;
    let mut outcome = PermitFlagOutcome {
        requires_permit: safety_flag,
        reason_code: reasons::NONE_CONFIGURED.into(),
        rule_code: None,
        permit_type_code: None,
        safety_flag,
        unreadable_rule_codes: Vec::new(),
    };
    if rows.is_empty() {
        return Ok(outcome);
    }
    outcome.reason_code = reasons::NO_RULE_MATCHED.into();

    let class = match equipment_id {
        Some(id) => equipment_class_code(db, id).await?,
        None => None,
    };
    let work_type: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM reference_values WHERE id = ?",
            [work_type_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<String>("", "code").ok());

    for row in rows {
        let code: String = row.try_get("", "code")?;
        let metadata: Option<String> = row.try_get("", "metadata_json")?;
        if rule_problem(db, metadata.as_deref()).await?.is_some() {
            outcome.unreadable_rule_codes.push(code);
            continue;
        }
        let Ok(rule) = parse_rule(metadata.as_deref()) else {
            continue;
        };
        let class_ok = rule
            .equipment_class_code
            .as_ref()
            .is_none_or(|c| class.as_deref() == Some(c.as_str()));
        let type_ok = rule
            .work_type_code
            .as_ref()
            .is_none_or(|t| work_type.as_deref() == Some(t.as_str()));
        if class_ok && type_ok && outcome.rule_code.is_none() {
            outcome.requires_permit = true;
            outcome.reason_code = reasons::RULE_MATCHED.into();
            outcome.rule_code = Some(code);
            outcome.permit_type_code = Some(rule.permit_type_code);
        }
    }
    Ok(outcome)
}

/// A rule added to the plant's published list, as the editor would have saved it (tests of every
/// work-order origin).
#[cfg(test)]
pub(crate) async fn add_published_rule(db: &impl ConnectionTrait, code: &str, metadata: serde_json::Value) {
    ensure_permit_requirement_rules(db).await.expect("rules list");
    let checked = checked_rule_metadata(db, code, Some(&metadata.to_string()))
        .await
        .expect("valid rule");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active, metadata_json) \
         SELECT rs.id, ?, ?, 1, 1, ? FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
        [code.into(), code.into(), checked.into(), DOMAIN.into()],
    ))
    .await
    .expect("rule");
}

#[cfg(test)]
mod tests {
    use super::parse_rule;

    #[test]
    fn a_rule_names_what_it_matches_and_a_permit_type() {
        let ok = parse_rule(Some(r#"{"equipment_class_code":"PUMP","permit_type_code":"HOT_WORK"}"#)).expect("ok");
        assert_eq!(ok.equipment_class_code.as_deref(), Some("PUMP"));
        assert_eq!(ok.work_type_code, None);
        assert!(
            parse_rule(Some(r#"{"permit_type_code":"HOT_WORK"}"#)).is_err(),
            "matches nothing"
        );
        assert!(parse_rule(Some(r#"{"work_type_code":"x"}"#)).is_err(), "no permit type");
        assert!(parse_rule(Some("not json")).is_err());
        assert!(parse_rule(None).is_err());
    }
}
