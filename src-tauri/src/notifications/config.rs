//! The plant's notification settings (COMPANY), edited in Données de référence → Notifications
//! (permission `ref.manage`):
//!
//! - per category: whether it is raised at all, who receives it (audiences), whether it must be
//!   acknowledged, which escalation policy applies, and the advance warning of "due soon"
//!   categories;
//! - the escalation steps: delay after the alert and target of each level;
//! - how long notifications are kept before they expire.
//!
//! The categories themselves, their module and severity are fixed by Maintafox (SYSTEM): none can
//! be added, removed or recoded here. Every change is a normal synchronised write.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::notifications::{catalog, scheduler};

/// Categories with an advance warning (`lead_time_minutes`).
pub const LEAD_TIME_CATEGORIES: &[&str] = &["ptw_expiring"];
/// Bounds of a delay or lead time, in minutes (one week).
pub const MAX_MINUTES: i64 = 10_080;

fn gate(msg: String) -> AppError {
    AppError::ValidationFailed(vec![format!("GATE_NOTIFICATION_CONFIG_INVALID: {msg}")])
}

fn decode(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("notification config decode '{field}': {err}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleConfig {
    pub rule_id: i64,
    pub category_code: String,
    pub module_code: String,
    pub default_severity: String,
    pub is_user_configurable: bool,
    /// Recipients are named elsewhere (budget alert configuration, digest): no audiences here.
    pub fixed_routing: bool,
    pub is_active: bool,
    pub audiences: Vec<String>,
    pub requires_ack: bool,
    pub escalation_policy_id: Option<i64>,
    pub uses_lead_time: bool,
    pub lead_time_minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EscalationStep {
    pub id: i64,
    pub level_no: i64,
    pub wait_minutes: i64,
    pub target: String,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EscalationPolicy {
    pub id: i64,
    pub name: String,
    pub steps: Vec<EscalationStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetentionSetting {
    pub expiry_days: i64,
    /// `true` while the plant has not set it (the shipped default applies).
    pub is_default: bool,
    pub default_days: i64,
    pub min_days: i64,
    pub max_days: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationConfig {
    pub rules: Vec<RuleConfig>,
    pub policies: Vec<EscalationPolicy>,
    pub retention: RetentionSetting,
    /// Audience codes a rule may use (plus `permission:<code>`).
    pub audiences: Vec<String>,
    pub escalation_targets: Vec<String>,
}

pub async fn load(conn: &impl ConnectionTrait) -> AppResult<NotificationConfig> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT r.id, c.code, COALESCE(c.module_code, '') AS module_code, c.default_severity, \
                    c.is_user_configurable, r.is_active, r.audiences_json, r.routing_mode, r.requires_ack, \
                    r.escalation_policy_id, r.lead_time_minutes \
             FROM notification_categories c JOIN notification_rules r ON r.category_code = c.code \
             WHERE c.is_offered = 1 ORDER BY c.module_code, c.code"
                .to_string(),
        ))
        .await?;
    let mut rules = Vec::with_capacity(rows.len());
    for r in &rows {
        let code: String = r.try_get("", "code").map_err(|e| decode("code", e))?;
        let audiences_json: Option<String> = r.try_get("", "audiences_json").map_err(|e| decode("audiences", e))?;
        let routing_mode: String = r.try_get("", "routing_mode").map_err(|e| decode("routing_mode", e))?;
        let severity: String = r.try_get("", "default_severity").map_err(|e| decode("severity", e))?;
        rules.push(RuleConfig {
            rule_id: r.try_get("", "id").map_err(|e| decode("id", e))?,
            module_code: r.try_get("", "module_code").map_err(|e| decode("module_code", e))?,
            default_severity: catalog::normalize_severity(&severity).to_string(),
            is_user_configurable: r
                .try_get::<i64>("", "is_user_configurable")
                .map_err(|e| decode("cfg", e))?
                == 1,
            fixed_routing: catalog::FIXED_ROUTING_CATEGORIES.contains(&code.as_str()),
            is_active: r.try_get::<i64>("", "is_active").map_err(|e| decode("is_active", e))? == 1,
            audiences: catalog::rule_audiences(audiences_json.as_deref(), &routing_mode)
                .iter()
                .map(catalog::Audience::code)
                .collect(),
            requires_ack: r
                .try_get::<i64>("", "requires_ack")
                .map_err(|e| decode("requires_ack", e))?
                == 1,
            escalation_policy_id: r
                .try_get("", "escalation_policy_id")
                .map_err(|e| decode("escalation_policy_id", e))?,
            uses_lead_time: LEAD_TIME_CATEGORIES.contains(&code.as_str()),
            lead_time_minutes: r.try_get("", "lead_time_minutes").map_err(|e| decode("lead", e))?,
            category_code: code,
        });
    }
    let stored = scheduler::stored_expiry_days(conn).await?;
    Ok(NotificationConfig {
        rules,
        policies: policies(conn).await?,
        retention: RetentionSetting {
            expiry_days: stored.unwrap_or(scheduler::DEFAULT_EXPIRY_DAYS),
            is_default: stored.is_none(),
            default_days: scheduler::DEFAULT_EXPIRY_DAYS,
            min_days: scheduler::EXPIRY_DAYS_RANGE.0,
            max_days: scheduler::EXPIRY_DAYS_RANGE.1,
        },
        audiences: catalog::AUDIENCES.iter().map(|a| (*a).to_string()).collect(),
        escalation_targets: catalog::ESCALATION_TARGETS.iter().map(|t| (*t).to_string()).collect(),
    })
}

async fn policies(conn: &impl ConnectionTrait) -> AppResult<Vec<EscalationPolicy>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.id AS pid, p.name, s.id AS sid, s.level_no, s.wait_minutes, s.route_to_type, s.route_to_reference \
             FROM notification_escalation_policies p \
             LEFT JOIN notification_escalation_steps s ON s.escalation_policy_id = p.id \
             ORDER BY p.name, p.id, s.level_no"
                .to_string(),
        ))
        .await?;
    let mut out: Vec<EscalationPolicy> = Vec::new();
    for r in &rows {
        let pid: i64 = r.try_get("", "pid").map_err(|e| decode("pid", e))?;
        if out.last().is_none_or(|p| p.id != pid) {
            out.push(EscalationPolicy {
                id: pid,
                name: r.try_get("", "name").map_err(|e| decode("name", e))?,
                steps: Vec::new(),
            });
        }
        if let Some(sid) = r.try_get::<Option<i64>>("", "sid").map_err(|e| decode("sid", e))? {
            if let Some(policy) = out.last_mut() {
                policy.steps.push(EscalationStep {
                    id: sid,
                    level_no: r.try_get("", "level_no").map_err(|e| decode("level_no", e))?,
                    wait_minutes: r.try_get("", "wait_minutes").map_err(|e| decode("wait", e))?,
                    target: r.try_get("", "route_to_type").map_err(|e| decode("target", e))?,
                    reference: r
                        .try_get("", "route_to_reference")
                        .map_err(|e| decode("reference", e))?,
                });
            }
        }
    }
    Ok(out)
}

/// A field that is present (possibly `null`) deserialises to `Some(value)`; an absent one stays `None`.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRuleInput {
    pub rule_id: i64,
    pub is_active: Option<bool>,
    pub audiences: Option<Vec<String>>,
    pub requires_ack: Option<bool>,
    /// `Some(None)` clears the escalation.
    #[serde(default, deserialize_with = "present")]
    pub escalation_policy_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    pub lead_time_minutes: Option<Option<i64>>,
}

pub async fn update_rule(conn: &impl ConnectionTrait, input: &UpdateRuleInput) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT category_code FROM notification_rules WHERE id = ?",
            [input.rule_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_rule".into(),
            id: input.rule_id.to_string(),
        })?;
    let category: String = row.try_get("", "category_code").map_err(|e| decode("category", e))?;
    let mut sets: Vec<&str> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(active) = input.is_active {
        sets.push("is_active = ?");
        values.push(i64::from(active).into());
    }
    if let Some(audiences) = &input.audiences {
        if catalog::FIXED_ROUTING_CATEGORIES.contains(&category.as_str()) {
            return Err(gate(format!("the recipients of '{category}' are not chosen by a rule")));
        }
        let list = catalog::validate_audiences(audiences)?;
        sets.push("audiences_json = ?");
        values.push(serde_json::to_string(&list).map_err(|e| decode("audiences", e))?.into());
    }
    if let Some(ack) = input.requires_ack {
        sets.push("requires_ack = ?");
        values.push(i64::from(ack).into());
    }
    if let Some(policy) = input.escalation_policy_id {
        if let Some(pid) = policy {
            let exists = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS x FROM notification_escalation_policies WHERE id = ?",
                    [pid.into()],
                ))
                .await?
                .is_some();
            if !exists {
                return Err(gate(format!("escalation policy {pid} does not exist")));
            }
        }
        sets.push("escalation_policy_id = ?");
        values.push(policy.into());
    }
    if let Some(lead) = input.lead_time_minutes {
        if !LEAD_TIME_CATEGORIES.contains(&category.as_str()) {
            return Err(gate(format!("'{category}' has no advance warning")));
        }
        if let Some(m) = lead {
            if !(1..=MAX_MINUTES).contains(&m) {
                return Err(gate(format!("the advance warning must be 1 to {MAX_MINUTES} minutes")));
            }
        }
        sets.push("lead_time_minutes = ?");
        values.push(lead.into());
    }
    if sets.is_empty() {
        return Ok(());
    }
    values.push(input.rule_id.into());
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("UPDATE notification_rules SET {} WHERE id = ?", sets.join(", ")),
        values,
    ))
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct EscalationStepInput {
    pub wait_minutes: i64,
    pub target: String,
    pub reference: Option<String>,
}

async fn validate_step(conn: &impl ConnectionTrait, input: &EscalationStepInput) -> AppResult<Option<String>> {
    if !(1..=MAX_MINUTES).contains(&input.wait_minutes) {
        return Err(gate(format!("an escalation delay must be 1 to {MAX_MINUTES} minutes")));
    }
    if !catalog::ESCALATION_TARGETS.contains(&input.target.as_str()) {
        return Err(gate(format!("unknown escalation target '{}'", input.target)));
    }
    let reference = input.reference.as_deref().map(str::trim).filter(|r| !r.is_empty());
    match input.target.as_str() {
        "permission" => {
            let p = reference.ok_or_else(|| gate("a permission target needs a permission".into()))?;
            if !crate::rbac::permissions::all_names().any(|n| n == p) {
                return Err(gate(format!("unknown permission '{p}'")));
            }
            Ok(Some(p.to_string()))
        }
        "role" => {
            let name = reference.ok_or_else(|| gate("a role target needs a role".into()))?;
            let exists = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS x FROM roles WHERE LOWER(TRIM(name)) = LOWER(?) AND deleted_at IS NULL",
                    [name.into()],
                ))
                .await?
                .is_some();
            if !exists {
                return Err(gate(format!("no role named '{name}'")));
            }
            Ok(Some(name.to_string()))
        }
        _ => Ok(None),
    }
}

/// Delays of the other steps of the policy must stay strictly increasing with the level.
async fn check_order(conn: &impl ConnectionTrait, policy_id: i64, level: i64, wait: i64) -> AppResult<()> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT level_no, wait_minutes FROM notification_escalation_steps WHERE escalation_policy_id = ? AND level_no <> ?",
            [policy_id.into(), level.into()],
        ))
        .await?;
    for r in &rows {
        let l: i64 = r.try_get("", "level_no").map_err(|e| decode("level_no", e))?;
        let w: i64 = r.try_get("", "wait_minutes").map_err(|e| decode("wait", e))?;
        if (l < level && w >= wait) || (l > level && w <= wait) {
            return Err(gate(format!(
                "level {level} must escalate later than level {} and earlier than the next levels",
                level - 1
            )));
        }
    }
    Ok(())
}

pub async fn update_step(conn: &impl ConnectionTrait, step_id: i64, input: &EscalationStepInput) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT escalation_policy_id, level_no FROM notification_escalation_steps WHERE id = ?",
            [step_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_escalation_step".into(),
            id: step_id.to_string(),
        })?;
    let policy: i64 = row
        .try_get("", "escalation_policy_id")
        .map_err(|e| decode("policy", e))?;
    let level: i64 = row.try_get("", "level_no").map_err(|e| decode("level", e))?;
    let reference = validate_step(conn, input).await?;
    check_order(conn, policy, level, input.wait_minutes).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notification_escalation_steps SET wait_minutes = ?, route_to_type = ?, route_to_reference = ? WHERE id = ?",
        [
            input.wait_minutes.into(),
            input.target.clone().into(),
            reference.into(),
            step_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Adds the next level to a policy.
pub async fn add_step(conn: &impl ConnectionTrait, policy_id: i64, input: &EscalationStepInput) -> AppResult<()> {
    let policy = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.name, COALESCE((SELECT MAX(level_no) FROM notification_escalation_steps s \
                                      WHERE s.escalation_policy_id = p.id), 0) AS last \
             FROM notification_escalation_policies p WHERE p.id = ?",
            [policy_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_escalation_policy".into(),
            id: policy_id.to_string(),
        })?;
    let name: String = policy.try_get("", "name").map_err(|e| decode("name", e))?;
    let level: i64 = policy.try_get::<i64>("", "last").map_err(|e| decode("last", e))? + 1;
    let reference = validate_step(conn, input).await?;
    check_order(conn, policy_id, level, input.wait_minutes).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_escalation_steps \
         (entity_sync_id, escalation_policy_id, level_no, wait_minutes, route_to_type, route_to_reference) \
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            // Same key as the seeded steps (`sync::seeded_identity`: policy name + level).
            crate::sync::identity::seeded_entity_sync_id("notification_escalation_steps", &format!("{name}:{level}"))
                .into(),
            policy_id.into(),
            level.into(),
            input.wait_minutes.into(),
            input.target.clone().into(),
            reference.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Removes the last level of its policy (levels stay contiguous).
pub async fn remove_step(conn: &impl ConnectionTrait, step_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT s.level_no, (SELECT MAX(level_no) FROM notification_escalation_steps x \
                                 WHERE x.escalation_policy_id = s.escalation_policy_id) AS last \
             FROM notification_escalation_steps s WHERE s.id = ?",
            [step_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_escalation_step".into(),
            id: step_id.to_string(),
        })?;
    let level: i64 = row.try_get("", "level_no").map_err(|e| decode("level", e))?;
    let last: i64 = row.try_get("", "last").map_err(|e| decode("last", e))?;
    if level != last {
        return Err(gate("only the last level of a policy can be removed".into()));
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM notification_escalation_steps WHERE id = ?",
        [step_id.into()],
    ))
    .await?;
    Ok(())
}

/// Validates a retention in days.
pub fn validate_expiry_days(days: i64) -> AppResult<()> {
    let (min, max) = scheduler::EXPIRY_DAYS_RANGE;
    if (min..=max).contains(&days) {
        Ok(())
    } else {
        Err(gate(format!("notifications are kept {min} to {max} days")))
    }
}
