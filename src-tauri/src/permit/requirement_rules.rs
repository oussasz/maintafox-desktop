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
    /// Published rules whose settings do not read (no class or work type, no permit type, not
    /// JSON): not applied. Logged and recorded in the work order's history and audit trail.
    pub unreadable_rule_codes: Vec<String>,
    /// Published rules that read but name a code not held here (an equipment class or work type not
    /// in its published list, a permit type not received yet): applied as written, so a rule that
    /// matches flags the work order (fail closed; the start gate asks for its permit type once it is
    /// here). Recorded in the work order's history and audit trail so they are fixed.
    #[serde(default)]
    pub unresolved_rule_codes: Vec<String>,
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

/// One rule of the published list, read once for every work order it is applied to.
#[derive(Debug, Clone)]
pub struct PublishedRule {
    pub code: String,
    /// The rule, or `None` when its settings do not read (never applied).
    pub spec: Option<RuleSpec>,
    /// It reads but names a code not held here (applied as written, recorded).
    pub unresolved: bool,
}

/// The plant's published rule list, in its order: sort order, then code (the same on every PC
/// holding the same list, never a local id).
#[derive(Debug, Clone, Default)]
pub struct PublishedRules(pub Vec<PublishedRule>);

impl PublishedRules {
    /// The active rules of the latest published set of the list.
    pub async fn load(db: &impl ConnectionTrait) -> AppResult<Self> {
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
                  ORDER BY rv.sort_order ASC, rv.code ASC",
                [DOMAIN.into()],
            ))
            .await?;
        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            let code: String = row.try_get("", "code")?;
            let metadata: Option<String> = row.try_get("", "metadata_json")?;
            let spec = parse_rule(metadata.as_deref()).ok();
            let unresolved = spec.is_some() && rule_problem(db, metadata.as_deref()).await?.is_some();
            rules.push(PublishedRule { code, spec, unresolved });
        }
        Ok(Self(rules))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Where `code` stands in the list's order (`None`: not in the list here).
    pub fn position(&self, code: &str) -> Option<usize> {
        self.0.iter().position(|r| r.code == code)
    }

    /// The permit type a rule of the list calls for (`None`: not in the list, or does not read).
    pub fn permit_type_of(&self, code: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|r| r.code == code)
            .and_then(|r| r.spec.as_ref())
            .map(|s| s.permit_type_code.as_str())
    }
}

/// Apply the plant's rules to a new work order: its equipment and work type. The single evaluation
/// for every origin (conversion, direct creation, PM, inspection anomaly, reliability action) and
/// for the re-check of a received work order. `safety_flag` (the source request's) keeps the
/// requirement whatever the rules say: a rule only adds one.
///
/// The first matching rule in the list's order (its sort order, then its code: the same on every PC
/// holding the same list, never a local id) is the one recorded.
pub async fn evaluate(
    db: &impl ConnectionTrait,
    equipment_id: Option<i64>,
    work_type_id: i64,
    safety_flag: bool,
) -> AppResult<PermitFlagOutcome> {
    let rules = PublishedRules::load(db).await?;
    evaluate_with(db, &rules, equipment_id, work_type_id, safety_flag).await
}

/// [`evaluate`] with the list already read (several work orders judged against one list).
pub async fn evaluate_with(
    db: &impl ConnectionTrait,
    rules: &PublishedRules,
    equipment_id: Option<i64>,
    work_type_id: i64,
    safety_flag: bool,
) -> AppResult<PermitFlagOutcome> {
    let mut outcome = PermitFlagOutcome {
        requires_permit: safety_flag,
        reason_code: reasons::NONE_CONFIGURED.into(),
        rule_code: None,
        permit_type_code: None,
        safety_flag,
        unreadable_rule_codes: Vec::new(),
        unresolved_rule_codes: Vec::new(),
    };
    if rules.is_empty() {
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

    for published in &rules.0 {
        let code = published.code.clone();
        let Some(rule) = published.spec.clone() else {
            outcome.unreadable_rule_codes.push(code);
            continue;
        };
        if published.unresolved {
            outcome.unresolved_rule_codes.push(code.clone());
        }
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

/// Rules that did not read, or that name a code not held here, when they are evaluated for a work
/// order (`PermitFlagOutcome::unreadable_rule_codes` / `unresolved_rule_codes`): logged, and recorded
/// in the work order's history (`permit_rules_unreadable`) and audit trail, so the work order can be
/// checked and the rules fixed. Nothing when every rule read.
pub async fn record_rule_problems(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    actor_id: Option<i64>,
    outcome: &PermitFlagOutcome,
) -> AppResult<()> {
    if outcome.unreadable_rule_codes.is_empty() && outcome.unresolved_rule_codes.is_empty() {
        return Ok(());
    }
    tracing::warn!(
        target: "maintafox",
        wo_id,
        unreadable = ?outcome.unreadable_rule_codes,
        unresolved = ?outcome.unresolved_rule_codes,
        "permit rules: published rules that do not read were not applied; rules naming codes not held here were \
         applied as written"
    );
    let (status, _, _) = crate::wo::workflow::transition::load_wo_status(conn, wo_id).await?;
    let payload = serde_json::json!({
        "unreadable_rule_codes": outcome.unreadable_rule_codes,
        "unresolved_rule_codes": outcome.unresolved_rule_codes,
        "applied_rule_code": outcome.rule_code,
    })
    .to_string();
    crate::wo::workflow::events::emit_action_event(
        conn,
        wo_id,
        "permit_rules_unreadable",
        actor_id,
        Some(&status),
        Some(&status),
        Some(&payload),
    )
    .await?;
    let mut summary = String::from("Règles de permis à corriger");
    if !outcome.unreadable_rule_codes.is_empty() {
        summary.push_str(&format!(
            " — illisibles, non appliquées : {}",
            outcome.unreadable_rule_codes.join(", ")
        ));
    }
    if !outcome.unresolved_rule_codes.is_empty() {
        summary.push_str(&format!(
            " — codes inconnus ici, appliquées telles qu'écrites : {}",
            outcome.unresolved_rule_codes.join(", ")
        ));
    }
    crate::wo::audit::insert_wo_change_event(
        conn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "permit_rules_unreadable".into(),
            actor_id,
            summary: Some(summary),
            details_json: Some(payload),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    Ok(())
}

/// Why a plant rule was recorded on a work order that already existed ([`stamp_matching_rule`]).
pub mod stamp_causes {
    /// Its equipment or work type changed in a draft edit (`wo::queries::update_wo_draft_fields`).
    pub const DRAFT_EDIT: &str = "draft_edit";
    /// The plant's rule list changed on this PC (a rule added, changed or published).
    pub const RULES_CHANGED: &str = "rules_changed";
    /// The plant's rule list was received by synchronisation.
    pub const RULES_RECEIVED: &str = "rules_received";
    /// The work order was received (new here, or changed) without the rule that matches it here.
    pub const WORK_ORDER_RECEIVED: &str = "work_order_received";
    /// The catch-up at start-up and after each synchronisation pass ([`super::catch_up`]): a work
    /// order a rule matches now (its equipment's class changed, a stamp missed before an upgrade).
    pub const CATCH_UP: &str = "catch_up";
    /// The work order was received naming another rule than the one held here, while not started:
    /// both PCs keep the same rule ([`super::converge_received_rule`], audit finding AU-04 R2-S1).
    pub const RULES_CONVERGED: &str = "rules_converged";
}

/// The version of the published rule list (recorded with a rule written on an existing work order).
async fn published_rule_set_version(conn: &impl ConnectionTrait) -> AppResult<Option<i64>> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rs.version_no FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
            [DOMAIN.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "version_no").ok()))
}

/// A work order that exists, has no plant rule and has not started (draft, planning, ready): when a
/// rule of the published list matches its equipment and work type now, that rule is recorded on it
/// (`requires_permit = 1`, `permit_rule_code`, `permit_rule_permit_type`), with the protection of a
/// rule flag (owner decision D2 (a), audit findings AU-04 B1/B2). It only adds: a rule already
/// recorded is never replaced or removed, and a work order already started is left as it is (its
/// start was judged where it started). A clearing it carried (a safety request's, decided without
/// this rule) is withdrawn: the rule's stricter clearing applies.
///
/// The same function on every PC, from the same work order and the same list, gives the same rule
/// (the list's order never depends on a local id): called on a local change (draft edit, rule list
/// changed here: written like the change, and sent with it) and on a received work order or rule
/// list (in the apply's transaction: each PC derives it, like the safety-request origin,
/// `permit_requirement::raise_work_orders_of_safety_request`). Recorded in the work order's history
/// (`permit_rule_applied`, with the cause and the list's version) and audit trail. Returns the rule
/// recorded.
pub async fn stamp_matching_rule(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    actor_id: Option<i64>,
    cause: &str,
) -> AppResult<Option<String>> {
    let rules = PublishedRules::load(conn).await?;
    stamp_matching_rule_with(conn, &rules, wo_id, actor_id, cause).await
}

/// [`stamp_matching_rule`] with the list already read.
async fn stamp_matching_rule_with(
    conn: &impl ConnectionTrait,
    rules: &PublishedRules,
    wo_id: i64,
    actor_id: Option<i64>,
    cause: &str,
) -> AppResult<Option<String>> {
    if rules.is_empty() {
        return Ok(None);
    }
    let not_started = crate::wo::workflow::actions::permit_requirement::NOT_STARTED_STATUS_CODES;
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.equipment_id, wo.type_id, wo.requires_permit, wo.permit_rule_code, st.code AS status, \
                    wo.permit_rule_cleared_by_id, wo.permit_rule_cleared_at, wo.permit_rule_clear_reason \
               FROM work_orders wo JOIN work_order_statuses st ON st.id = wo.status_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let held: Option<String> = row.try_get("", "permit_rule_code")?;
    let status: String = row.try_get("", "status")?;
    if held.is_some() || !not_started.contains(&status.as_str()) {
        return Ok(None);
    }
    let equipment: Option<i64> = row.try_get("", "equipment_id")?;
    let type_id: i64 = row.try_get("", "type_id")?;
    let outcome = evaluate_with(conn, rules, equipment, type_id, false).await?;
    let (Some(rule), Some(permit_type)) = (outcome.rule_code.clone(), outcome.permit_type_code.clone()) else {
        return Ok(None);
    };
    let previous_requires: i64 = row.try_get("", "requires_permit")?;
    let cleared_by: Option<i64> = row.try_get("", "permit_rule_cleared_by_id")?;
    let cleared_at: Option<String> = row.try_get("", "permit_rule_cleared_at")?;
    let reason: Option<String> = row.try_get("", "permit_rule_clear_reason")?;
    let written = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_permit = 1, permit_rule_code = ?, permit_rule_permit_type = ?, \
                    permit_rule_cleared_by_id = NULL, permit_rule_cleared_at = NULL, \
                    permit_rule_clear_reason = NULL, row_version = row_version + 1 \
              WHERE id = ? AND permit_rule_code IS NULL",
            [rule.clone().into(), permit_type.clone().into(), wo_id.into()],
        ))
        .await?;
    if written.rows_affected() == 0 {
        return Ok(None);
    }
    let payload = serde_json::json!({
        "requires_permit": true,
        "origin": "rule",
        "cause": cause,
        "rule_code": rule,
        "permit_type_code": permit_type,
        "rule_set_version": published_rule_set_version(conn).await?,
        "actor_sync_id": crate::wo::workflow::actions::permit_requirement::user_sync_id(conn, actor_id).await?,
        "previous_requires_permit": previous_requires != 0,
        "withdrawn_cleared_by_id": cleared_by,
        "withdrawn_cleared_by_sync_id":
            crate::wo::workflow::actions::permit_requirement::user_sync_id(conn, cleared_by).await?,
        "withdrawn_cleared_at": cleared_at,
        "withdrawn_clear_reason": reason,
    })
    .to_string();
    crate::wo::workflow::events::emit_action_event(
        conn,
        wo_id,
        "permit_rule_applied",
        actor_id,
        Some(&status),
        Some(&status),
        Some(&payload),
    )
    .await?;
    crate::wo::audit::insert_wo_change_event(
        conn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "permit_rule_applied".into(),
            actor_id,
            summary: Some(format!("Permis exigé : la règle {rule} de l'usine s'applique à cet OT")),
            details_json: Some(payload),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    record_rule_problems(conn, wo_id, actor_id, &outcome).await?;
    Ok(Some(rule))
}

/// [`stamp_matching_rule`] on every work order of this PC that has no plant rule and has not
/// started: when the plant's rule list changes here or is received. Returns the work orders flagged.
pub async fn stamp_open_work_orders(
    conn: &impl ConnectionTrait,
    actor_id: Option<i64>,
    cause: &str,
) -> AppResult<Vec<i64>> {
    // The list read once for every work order; no rule: nothing to flag.
    let rules = PublishedRules::load(conn).await?;
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let ids = open_unruled_work_orders(conn).await?;
    let mut flagged = Vec::new();
    for id in ids {
        if stamp_matching_rule_with(conn, &rules, id, actor_id, cause)
            .await?
            .is_some()
        {
            flagged.push(id);
        }
    }
    Ok(flagged)
}

/// The work orders not started yet (draft, planning, ready) that have no plant rule, by id.
async fn open_unruled_work_orders(conn: &impl ConnectionTrait) -> AppResult<Vec<i64>> {
    let not_started = crate::wo::workflow::actions::permit_requirement::NOT_STARTED_STATUS_CODES;
    let marks = vec!["?"; not_started.len()].join(",");
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id FROM work_orders wo JOIN work_order_statuses st ON st.id = wo.status_id \
                  WHERE wo.permit_rule_code IS NULL AND st.code IN ({marks}) ORDER BY wo.id"
            ),
            not_started.iter().map(|c| sea_orm::Value::from(*c)).collect::<Vec<_>>(),
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(Into::into))
        .collect()
}

/// The durable catch-up of [`stamp_open_work_orders`] (audit finding AU-04 R2-S2), like
/// `permit::ended_hold::sweep`: at start-up and after each synchronisation pass, every work order
/// not started yet, without a rule, that a rule of the list here matches now gets it (a stamp a
/// failed or older run missed, an equipment whose class changed since). Each PC derives the same
/// from the same work order and list, so it is written with capture suppressed and never sent, like
/// a stamp made while applying received rows. The work orders due are found by reads only; each one
/// a rule matches is then stamped in its own short transaction, so the write lock is held only for
/// that work order, and one that fails is logged and retried by the next run without holding back
/// the others (audit finding AU-04 R3-N3). Idempotent. Returns the work orders flagged.
pub async fn catch_up(db: &sea_orm::DatabaseConnection) -> usize {
    use sea_orm::TransactionTrait;
    let due = async {
        let rules = PublishedRules::load(db).await?;
        if rules.is_empty() {
            return AppResult::Ok((rules, Vec::new()));
        }
        let ids = open_unruled_work_orders(db).await?;
        Ok((rules, ids))
    };
    let (rules, ids) = match due.await {
        Ok(found) => found,
        Err(err) => {
            tracing::error!(
                target: "maintafox::sync",
                error = %err,
                "permit rules catch-up not read; retried at the next run"
            );
            return 0;
        }
    };
    let mut flagged = 0;
    for wo_id in ids {
        let one = async {
            // Read first: a work order no rule matches takes no write lock.
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT equipment_id, type_id FROM work_orders WHERE id = ? AND permit_rule_code IS NULL",
                    [wo_id.into()],
                ))
                .await?;
            let Some(row) = row else {
                return AppResult::Ok(false);
            };
            let equipment: Option<i64> = row.try_get("", "equipment_id")?;
            let type_id: i64 = row.try_get("", "type_id")?;
            if evaluate_with(db, &rules, equipment, type_id, false)
                .await?
                .rule_code
                .is_none()
            {
                return Ok(false);
            }
            let txn = db.begin().await?;
            crate::sync::capture::begin_suppression(&txn).await?;
            let stamped = stamp_matching_rule_with(&txn, &rules, wo_id, None, stamp_causes::CATCH_UP).await?;
            crate::sync::capture::end_suppression(&txn).await?;
            txn.commit().await?;
            Ok(stamped.is_some())
        };
        match one.await {
            Ok(true) => flagged += 1,
            Ok(false) => {}
            Err(err) => tracing::error!(
                target: "maintafox::sync",
                work_order_id = wo_id,
                error = %err,
                "permit rule not applied to this work order by the catch-up; retried at the next run"
            ),
        }
    }
    flagged
}

/// Whether the reference set `set_id` belongs to the plant's rule list.
pub async fn is_rules_set(conn: &impl ConnectionTrait, set_id: i64) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE rs.id = ? AND UPPER(TRIM(d.code)) = ?",
            [set_id.into(), DOMAIN.into()],
        ))
        .await?
        .is_some())
}

/// After a change of the plant's rule list on this PC (a rule added, changed or reactivated, a set
/// published, an import applied; `reference::values`, `reference::sets`, `reference::publish`,
/// `reference::imports`): the work orders not started yet that a rule now matches are flagged, in
/// the transaction of the change (both are kept, or neither: audit finding AU-04 R2-S2), written as
/// local changes (sent to the other PCs, which derive the same). Nothing for another list.
pub async fn after_rule_list_change(conn: &impl ConnectionTrait, set_id: i64, actor_id: Option<i64>) -> AppResult<()> {
    if !is_rules_set(conn, set_id).await? {
        return Ok(());
    }
    stamp_open_work_orders(conn, actor_id, stamp_causes::RULES_CHANGED).await?;
    Ok(())
}

/// A work order received started (past ready, or cancelled after its start) without the plant rule
/// that concerns it here, recorded once as a safety finding in its history
/// (`permit_rule_missing_on_arrival`) and audit trail, with the permit-issue right holders of its
/// site told (`settings::received_notice::notify_work_order_without_rule`): its work was done
/// without the rule's protection (whether its permit was required there, and of which type, is in
/// the finding). Two cases:
/// - it has no rule here, and a rule of the list here matches it (its equipment and work type): it
///   is applied as received (its history is kept, never refused);
/// - it holds a rule here and the received row carried none (`received_without_held_rule`: whether
///   that row required the permit): the row is applied, the rule, its permit type, the requirement
///   and a clearing held here are kept (`permit::sync_check::keep_held_rule_trace`, audit finding
///   AU-04 R2-B1), and the finding names the rule held here.
///
/// Returns the rule.
pub async fn finding_for_started_without_rule(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    received_without_held_rule: Option<bool>,
    received_named_rule: bool,
) -> AppResult<Option<String>> {
    let not_started = crate::wo::workflow::actions::permit_requirement::NOT_STARTED_STATUS_CODES;
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.code, wo.entity_sync_id, wo.equipment_id, wo.type_id, wo.requires_permit, wo.permit_rule_code, \
                    wo.permit_rule_permit_type, wo.actual_start, st.code AS status, \
                    (SELECT COUNT(*) FROM wo_action_events e WHERE e.wo_id = wo.id \
                      AND e.action_code = 'permit_rule_missing_on_arrival') AS found \
               FROM work_orders wo JOIN work_order_statuses st ON st.id = wo.status_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let held: Option<String> = row.try_get("", "permit_rule_code")?;
    let status: String = row.try_get("", "status")?;
    let actual_start: Option<String> = row.try_get("", "actual_start")?;
    let found: i64 = row.try_get("", "found")?;
    let started = !not_started.contains(&status.as_str()) && (status != "cancelled" || actual_start.is_some());
    if !started || found > 0 {
        return Ok(None);
    }
    let mut received_rule: Option<(String, Option<String>)> = None;
    let (rule, permit_type, requires, held_here) = match (held, received_without_held_rule) {
        (Some(rule), Some(received_requires)) => {
            let permit_type: Option<String> = row.try_get("", "permit_rule_permit_type")?;
            (rule, permit_type.unwrap_or_default(), received_requires, true)
        }
        // Received started naming a rule (none held here): a finding when the list here asks for
        // another rule or permit type (audit finding AU-04 R4-B1); the row is applied as received.
        (Some(named), None) if received_named_rule => {
            let named_type: Option<String> = row.try_get("", "permit_rule_permit_type")?;
            let equipment: Option<i64> = row.try_get("", "equipment_id")?;
            let type_id: i64 = row.try_get("", "type_id")?;
            let outcome = evaluate(conn, equipment, type_id, false).await?;
            let (Some(rule), Some(permit_type)) = (outcome.rule_code, outcome.permit_type_code) else {
                return Ok(None);
            };
            if rule == named && Some(&permit_type) == named_type.as_ref() {
                return Ok(None);
            }
            received_rule = Some((named, named_type));
            let requires: i64 = row.try_get("", "requires_permit")?;
            (rule, permit_type, requires != 0, false)
        }
        (Some(_), None) => return Ok(None),
        (None, _) => {
            let equipment: Option<i64> = row.try_get("", "equipment_id")?;
            let type_id: i64 = row.try_get("", "type_id")?;
            let outcome = evaluate(conn, equipment, type_id, false).await?;
            let (Some(rule), Some(permit_type)) = (outcome.rule_code, outcome.permit_type_code) else {
                return Ok(None);
            };
            let requires: i64 = row.try_get("", "requires_permit")?;
            (rule, permit_type, requires != 0, false)
        }
    };
    let code: String = row.try_get("", "code")?;
    let wo_sync: String = row.try_get("", "entity_sync_id")?;
    let payload = serde_json::json!({
        "rule_code": rule,
        "permit_type_code": permit_type,
        "rule_set_version": published_rule_set_version(conn).await?,
        "status": status,
        "requires_permit": requires,
        "rule_held_here": held_here,
        "received_rule_code": received_rule.as_ref().map(|r| r.0.clone()),
        "received_permit_type_code": received_rule.as_ref().and_then(|r| r.1.clone()),
    })
    .to_string();
    crate::wo::workflow::events::emit_action_event(
        conn,
        wo_id,
        "permit_rule_missing_on_arrival",
        None,
        Some(&status),
        Some(&status),
        Some(&payload),
    )
    .await?;
    crate::wo::audit::insert_wo_change_event(
        conn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "permit_rule_missing_on_arrival".into(),
            actor_id: None,
            summary: Some(format!(
                "Constat sécurité : OT reçu déjà démarré sans la règle {rule} que l'usine applique ici \
                 (permis {permit_type})"
            )),
            details_json: Some(payload),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    crate::settings::received_notice::notify_work_order_without_rule(
        conn,
        wo_id,
        &wo_sync,
        &code,
        &rule,
        &permit_type,
        requires,
    )
    .await?;
    Ok(Some(rule))
}

/// The plant rule held here on a work order before a received row was written over it, with the
/// clearing it carried (`permit::sync_check::keep_held_rule_trace`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldTrace {
    pub code: String,
    pub permit_type: Option<String>,
    pub cleared_by_sync_id: Option<String>,
    pub cleared_at: Option<String>,
    pub clear_reason: Option<String>,
}

/// A work order not started yet, received naming a rule (or a permit type) other than the one held
/// here (`held`), or naming a rule while none is held here (`held` = `None`: a work order new here,
/// or not stamped yet): two PCs that derived the rule from two versions of the list or from
/// different equipment data, or a row that names a weaker rule or type (audit findings AU-04 R2-S1,
/// R3-B1, R4-B1). Applied, never refused; then this PC's own evaluation decides, never the received
/// row: the first rule of the list here that matches the work order here, with the list's permit
/// type; with none, the rule held here with its permit type; with none held either, the received
/// rule as sent (it only adds a requirement where nothing here asks for one). A received rule or
/// permit type that does not match here, or is unknown or retired here, never replaces a rule this
/// PC derives or holds. Every PC with the same list and the same equipment data keeps the same rule,
/// with no conflict; a requirement is never removed (`requires_permit` stays 1). Written with capture
/// suppressed (each PC derives it; this runs in the apply): when the row written is not the rule
/// kept, the rule kept is written and a clearing the row carried (decided for another rule) is
/// withdrawn. Recorded (`permit_rule_applied`, cause `rules_converged`, the rule and permit type
/// before (the held ones, else the received ones), after and received, and the clearings withdrawn)
/// in the work order's history and audit trail whenever the rule or type changes from that "before",
/// or a clearing (held here, or carried by the row) is withdrawn (audit finding AU-04 R4-S1).
/// Returns the rule kept when something was recorded.
pub async fn converge_received_rule(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    held: Option<&HeldTrace>,
) -> AppResult<Option<String>> {
    let not_started = crate::wo::workflow::actions::permit_requirement::NOT_STARTED_STATUS_CODES;
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.equipment_id, wo.type_id, wo.requires_permit, wo.permit_rule_code, wo.permit_rule_permit_type, \
                    st.code AS status, wo.permit_rule_cleared_by_id, wo.permit_rule_cleared_at, \
                    wo.permit_rule_clear_reason \
               FROM work_orders wo JOIN work_order_statuses st ON st.id = wo.status_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let status: String = row.try_get("", "status")?;
    let received_code: Option<String> = row.try_get("", "permit_rule_code")?;
    let received_type: Option<String> = row.try_get("", "permit_rule_permit_type")?;
    let (Some(received_code), true) = (received_code, not_started.contains(&status.as_str())) else {
        return Ok(None);
    };
    let rules = PublishedRules::load(conn).await?;
    let equipment: Option<i64> = row.try_get("", "equipment_id")?;
    let type_id: i64 = row.try_get("", "type_id")?;
    let first_match = evaluate_with(conn, &rules, equipment, type_id, false).await?;
    let received = (received_code.clone(), received_type.clone());
    let before = held.map_or_else(|| received.clone(), |h| (h.code.clone(), h.permit_type.clone()));
    let kept = match (first_match.rule_code, first_match.permit_type_code) {
        (Some(code), Some(permit_type)) => (code, Some(permit_type)),
        _ => before.clone(),
    };
    let rule_changed = kept != before;
    let row_differs = kept != received;
    let row_cleared_by: Option<i64> = row.try_get("", "permit_rule_cleared_by_id")?;
    let row_cleared_at: Option<String> = row.try_get("", "permit_rule_cleared_at")?;
    let row_reason: Option<String> = row.try_get("", "permit_rule_clear_reason")?;
    use crate::wo::workflow::actions::permit_requirement::user_sync_id;
    // A clearing held here that the row did not keep, and one the row carried for another rule.
    let held_withdrawn = held.filter(|h| h.cleared_at.is_some() && (row_differs || row_cleared_at != h.cleared_at));
    let row_withdrawn =
        row_differs && row_cleared_at.is_some() && held.and_then(|h| h.cleared_at.clone()) != row_cleared_at;
    if row_differs {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_permit = 1, permit_rule_code = ?, permit_rule_permit_type = ?, \
                    permit_rule_cleared_by_id = NULL, permit_rule_cleared_at = NULL, \
                    permit_rule_clear_reason = NULL, row_version = row_version + 1 \
              WHERE id = ?",
            [kept.0.clone().into(), kept.1.clone().into(), wo_id.into()],
        ))
        .await?;
    }
    if !rule_changed && held_withdrawn.is_none() && !row_withdrawn {
        return Ok(None);
    }
    let received_requires: i64 = row.try_get("", "requires_permit")?;
    let (row_by_sync, row_at, row_why) = if row_withdrawn {
        (user_sync_id(conn, row_cleared_by).await?, row_cleared_at, row_reason)
    } else {
        (None, None, None)
    };
    let payload = serde_json::json!({
        "requires_permit": true,
        "origin": "rule",
        "cause": stamp_causes::RULES_CONVERGED,
        "rule_code": kept.0,
        "permit_type_code": kept.1,
        "previous_rule_code": before.0,
        "previous_permit_type_code": before.1,
        "held_rule_code": held.map(|h| h.code.clone()),
        "received_rule_code": received.0,
        "received_permit_type_code": received.1,
        "rule_set_version": published_rule_set_version(conn).await?,
        "received_requires_permit": received_requires != 0,
        "withdrawn_cleared_by_sync_id": held_withdrawn.and_then(|h| h.cleared_by_sync_id.clone()),
        "withdrawn_cleared_at": held_withdrawn.and_then(|h| h.cleared_at.clone()),
        "withdrawn_clear_reason": held_withdrawn.and_then(|h| h.clear_reason.clone()),
        "received_withdrawn_cleared_by_sync_id": row_by_sync,
        "received_withdrawn_cleared_at": row_at,
        "received_withdrawn_clear_reason": row_why,
    })
    .to_string();
    crate::wo::workflow::events::emit_action_event(
        conn,
        wo_id,
        "permit_rule_applied",
        None,
        Some(&status),
        Some(&status),
        Some(&payload),
    )
    .await?;
    let mut summary = format!(
        "Permis exigé : la règle {} (permis {}) de l'usine s'applique à cet OT (avant : {} permis {} ; reçu : {} permis {})",
        kept.0,
        kept.1.as_deref().unwrap_or("?"),
        before.0,
        before.1.as_deref().unwrap_or("?"),
        received.0,
        received.1.as_deref().unwrap_or("?"),
    );
    if held_withdrawn.is_some() || row_withdrawn {
        summary.push_str(" ; levée de l'exigence retirée");
    }
    crate::wo::audit::insert_wo_change_event(
        conn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "permit_rule_applied".into(),
            actor_id: None,
            summary: Some(summary),
            details_json: Some(payload),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    Ok(Some(kept.0))
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
