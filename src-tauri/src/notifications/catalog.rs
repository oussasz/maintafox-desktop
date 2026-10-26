//! Fixed vocabulary of the notification system (SYSTEM: maintained by Maintafox).
//!
//! - **Modules** a category belongs to, with the permission a reader needs to see its
//!   notifications. The inbox checks it every time it is read (`notifications::inbox`), so a
//!   person who loses `ot.view` stops seeing work-order notifications at once, whatever was
//!   created for them before.
//! - **Severities** of the notifications that are not about work: `info`, `warning`, `critical`
//!   (`error`, written by older versions, reads as `critical`). Notifications about work also carry
//!   the level of the shared urgency scale (`reference::urgency_scale`).
//! - **Audiences**: who a rule sends a category to (`notification_rules.audiences_json`). The list
//!   of audiences of each rule is the plant's (COMPANY, Données de référence → Notifications); the
//!   audience codes themselves are fixed here.
//! - **Escalation targets** of an escalation step.

use crate::errors::{AppError, AppResult};

/// `(module code, view permission)`. `system` notifications (daily digest) need none.
pub const MODULES: &[(&str, Option<&str>)] = &[
    ("di", Some(crate::rbac::permissions::DI_VIEW)),
    ("ot", Some(crate::rbac::permissions::OT_VIEW)),
    ("pm", Some(crate::rbac::permissions::PM_VIEW)),
    ("ins", Some(crate::rbac::permissions::INS_VIEW)),
    ("inv", Some(crate::rbac::permissions::INV_VIEW)),
    ("ptw", Some(crate::rbac::permissions::PTW_VIEW)),
    ("per", Some(crate::rbac::permissions::PER_VIEW)),
    ("plan", Some(crate::rbac::permissions::PLAN_VIEW)),
    ("fin", Some(crate::rbac::permissions::FIN_VIEW)),
    ("sync", Some(crate::rbac::permissions::SYNC_VIEW)),
    ("system", None),
];

/// View permission of a module; `None` for an unknown module (its notifications are not shown).
pub fn module_view_permission(module: &str) -> Option<Option<&'static str>> {
    MODULES.iter().find(|(m, _)| *m == module).map(|(_, p)| *p)
}

/// Severities of the notifications (SYSTEM list).
pub const SEVERITIES: &[&str] = &["info", "warning", "critical"];

/// Severity as shown: the stored `error` of older versions reads as `critical`.
pub fn normalize_severity(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "critical" | "error" => "critical",
        "warning" => "warning",
        _ => "info",
    }
}

/// Audience codes of a rule. `permission:<code>` is written with the permission code.
pub const AUDIENCES: &[&str] = &[
    // The people the record names.
    "assignee",
    "requester",
    "planner",
    "reviewer",
    "submitter",
    "watcher",
    // Their organisation.
    "team",
    "team_lead",
    "supervisor",
    "entity_manager",
];

/// Prefix of a permission audience (`permission:inv.manage`).
pub const PERMISSION_AUDIENCE_PREFIX: &str = "permission:";

/// One parsed audience of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audience {
    Named(&'static str),
    Permission(String),
}

impl Audience {
    pub fn code(&self) -> String {
        match self {
            Self::Named(code) => (*code).to_string(),
            Self::Permission(p) => format!("{PERMISSION_AUDIENCE_PREFIX}{p}"),
        }
    }
}

/// Parses one audience code; `None` when it is not one of [`AUDIENCES`] or a permission audience.
pub fn parse_audience(raw: &str) -> Option<Audience> {
    let raw = raw.trim();
    if let Some(p) = raw.strip_prefix(PERMISSION_AUDIENCE_PREFIX) {
        let p = p.trim();
        let known = crate::rbac::permissions::all_names().any(|n| n == p);
        return known.then(|| Audience::Permission(p.to_string()));
    }
    AUDIENCES.iter().find(|a| **a == raw).map(|a| Audience::Named(a))
}

/// Audiences of a rule from its stored list, falling back to the single routing mode of rules
/// written before migration 209. Unknown codes are skipped (and logged).
pub fn rule_audiences(audiences_json: Option<&str>, routing_mode: &str) -> Vec<Audience> {
    let codes: Vec<String> = match audiences_json {
        Some(raw) => serde_json::from_str(raw).unwrap_or_else(|err| {
            tracing::warn!(error = %err, raw, "notifications: unreadable rule audiences");
            Vec::new()
        }),
        None => vec![routing_mode.to_string()],
    };
    codes
        .iter()
        .filter_map(|c| {
            let parsed = parse_audience(c);
            if parsed.is_none() && !matches!(c.as_str(), "manual" | "role") {
                tracing::warn!(audience = %c, "notifications: unknown audience skipped");
            }
            parsed
        })
        .collect()
}

/// Validates and normalises the audience list of a rule (Données de référence editor).
pub fn validate_audiences(raw: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for code in raw {
        let parsed = parse_audience(code).ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "GATE_NOTIFICATION_AUDIENCE_INVALID: unknown audience '{code}'."
            )])
        })?;
        let code = parsed.code();
        if !out.contains(&code) {
            out.push(code);
        }
    }
    Ok(out)
}

/// Target types of an escalation step. `permission` and `role` carry a reference (permission
/// code, role name); the others are resolved from the record and the people already notified.
pub const ESCALATION_TARGETS: &[&str] = &[
    "supervisor",
    "planner",
    "team_lead",
    "entity_manager",
    "permission",
    "role",
];

/// Categories whose recipients are not chosen by a rule: the budget alert configuration names
/// its recipient, the digest goes to the person who asked for it, a safety rule changed on another
/// PC goes to the settings administrators ([`ALWAYS_DELIVERED`]).
pub const FIXED_ROUTING_CATEGORIES: &[&str] = &[
    "budget_control_alert",
    "daily_digest",
    "safety_rule_received",
    "loto_forced_removal",
    "pm_concurrent_versions",
    "permit_suspended_work_held",
    "permit_ended_under_work",
    "wo_permit_rule_missing",
    "insp_anomaly_origin_cancelled",
    "rel_decision_refused_wo_kept",
];

/// Categories that can be neither muted nor re-routed, locally or by synchronisation: always sent
/// to this audience, whatever their rule row says (inactive, other audiences) and whatever the
/// recipients' preferences say. The notice that a safety rule changed on another PC exists to catch
/// a rogue PC, which could otherwise switch it off first (`settings::received_notice`).
///
/// Delivery of such a category ignores preferences on every channel: in-app and desktop toast
/// always, never folded into the daily digest, never emailed (the delivery SQL of `os_toast`,
/// `digest` and `email` names `safety_rule_received`; toast, digest and email each have a test with
/// a preference set after the notice). Its preference rows are also removed before each notice
/// (`settings::received_notice::ensure_notice_rows`, tested).
/// A lock taken off by a forced removal goes to the permit close-right holders of the permit's
/// site (`settings::received_notice::FORCED_REMOVAL_CATEGORY`, routed by the permit's node).
pub const ALWAYS_DELIVERED: &[(&str, &str)] = &[
    ("safety_rule_received", "permission:adm.settings"),
    ("loto_forced_removal", "permission:ptw.close"),
    // Two concurrent versions of a PM plan (`pm::version_clash`): the PM planners.
    ("pm_concurrent_versions", "permission:pm.edit"),
    // A running work order put on hold by the suspension of its permit (owner decision D5): its
    // responsible (the permit's work order routes `assignee` to it); nobody there: the fallback
    // ([`ALWAYS_DELIVERED_FALLBACK`]).
    ("permit_suspended_work_held", "assignee"),
    // A permit ended on another PC before, or while, its work ran here (owner decision D9): the
    // work order's responsible and the permit close-right holders of the permit's site.
    ("permit_ended_under_work", "assignee"),
    ("permit_ended_under_work", "permission:ptw.close"),
    // A work order received started without the plant permit rule that concerns it here (owner
    // decision D2 (a), audit finding AU-04 R2-S3): the permit-issue right holders of its site.
    ("wo_permit_rule_missing", "permission:ptw.issue"),
    // An inspection anomaly cancelled after it was routed (CMMS Advisor decision on AU-72, option B):
    // the responsible of its work order (or of the work order its request was converted to).
    ("insp_anomaly_origin_cancelled", "assignee"),
    // A reliability decision refused at synchronisation whose work order is kept, or whose PM
    // version stays published (AU-76b, CMMS Advisor Q6 and S4): the work order's planner, and as
    // the event's watchers the decision's proposer, the work order's responsible when it has no
    // planner, or the PM planners (`settings::received_notice::announce_refused_decision`).
    ("rel_decision_refused_wo_kept", "planner"),
    ("rel_decision_refused_wo_kept", "watcher"),
];

/// The audience of an [`ALWAYS_DELIVERED`] category when its fixed audiences reach nobody here: a
/// safety notice must reach someone. A running work order held by its permit's suspension (D5)
/// whose responsible is unset, or not an active user here, goes to the permit close-right holders of
/// the permit's site, the audience of a permit ended under work (D9). A work order received started
/// without its plant permit rule, at a site where nobody holds the permit-issue right here, goes to
/// the settings administrators (who keep the rule list).
pub const ALWAYS_DELIVERED_FALLBACK: &[(&str, &str)] = &[
    ("permit_suspended_work_held", "permission:ptw.close"),
    ("wo_permit_rule_missing", "permission:adm.settings"),
    // No responsible: the work order's planner, or the request's reviewer (its node's responsibles
    // when it has none).
    ("insp_anomaly_origin_cancelled", "planner"),
    ("insp_anomaly_origin_cancelled", "reviewer"),
    // Neither a planner nor a proposer active here: the holders of the right to approve decisions.
    ("rel_decision_refused_wo_kept", "permission:ram.manage"),
];

/// The fallback audiences of an [`ALWAYS_DELIVERED`] category ([`ALWAYS_DELIVERED_FALLBACK`]).
pub fn always_delivered_fallback(category: &str) -> Vec<Audience> {
    ALWAYS_DELIVERED_FALLBACK
        .iter()
        .filter(|(c, _)| *c == category)
        .filter_map(|(_, a)| parse_audience(a))
        .collect()
}

/// The fixed audience of an [`ALWAYS_DELIVERED`] category (its first, when it has several).
pub fn always_delivered_audience(category: &str) -> Option<Audience> {
    always_delivered_audiences(category).into_iter().next()
}

/// Every fixed audience of an [`ALWAYS_DELIVERED`] category (empty: the category is routed by its
/// rule).
pub fn always_delivered_audiences(category: &str) -> Vec<Audience> {
    ALWAYS_DELIVERED
        .iter()
        .filter(|(c, _)| *c == category)
        .filter_map(|(_, a)| parse_audience(a))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audiences_parse_named_and_permission_codes() {
        assert_eq!(parse_audience("planner"), Some(Audience::Named("planner")));
        assert_eq!(
            parse_audience(&format!("permission:{}", crate::rbac::permissions::INV_MANAGE)),
            Some(Audience::Permission(crate::rbac::permissions::INV_MANAGE.into()))
        );
        assert_eq!(parse_audience("permission:no.such"), None);
        assert_eq!(parse_audience("boss"), None);
        assert!(validate_audiences(&["boss".into()]).is_err());
        assert_eq!(
            validate_audiences(&["assignee".into(), "assignee".into(), " planner ".into()]).unwrap(),
            vec!["assignee".to_string(), "planner".to_string()]
        );
    }

    #[test]
    fn legacy_rules_fall_back_to_their_routing_mode() {
        assert_eq!(rule_audiences(None, "reviewer"), vec![Audience::Named("reviewer")]);
        assert!(rule_audiences(None, "manual").is_empty());
        assert_eq!(
            rule_audiences(
                Some(&format!(
                    r#"["team","permission:{}"]"#,
                    crate::rbac::permissions::OT_VIEW
                )),
                "assignee"
            ),
            vec![
                Audience::Named("team"),
                Audience::Permission(crate::rbac::permissions::OT_VIEW.into())
            ]
        );
    }

    #[test]
    fn every_module_permission_is_a_known_permission() {
        for (_, perm) in MODULES {
            if let Some(p) = perm {
                assert!(crate::rbac::permissions::all_names().any(|n| n == *p), "{p}");
            }
        }
        assert_eq!(normalize_severity("error"), "critical");
        assert_eq!(normalize_severity("whatever"), "info");
    }
}
