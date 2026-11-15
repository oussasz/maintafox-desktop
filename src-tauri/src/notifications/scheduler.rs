//! Notification sweep, every minute on each device:
//!
//! 1. snoozed notifications whose time came back become unread again;
//! 2. condition watchers raise and resolve their notifications (`notifications::watchers`);
//! 3. notifications whose condition cleared are resolved (`notifications::conditions`);
//! 4. unacknowledged notifications escalate (below);
//! 5. notifications older than the retention period expire.
//!
//! **Escalation.** A category whose rule asks for an acknowledgement and names an escalation
//! policy escalates while nobody notified about the event acknowledged it and its condition still
//! holds. Step N of the policy fires `wait_minutes` after the first notification of the event
//! (the delays are cumulative from the alert, so "30, 60, 120" means after half an hour, one hour,
//! two hours). It notifies the step's target — the supervisor of the people already notified, the
//! record's planner, its team leads, the responsibles of its site / org node, the holders of a
//! permission, or the members of a role — who were not notified yet, with the reason
//! `escalation` and the level. Every step is logged in the activity feed (also a step that found
//! nobody, which still moves the event to the next level). The delays and targets are the plant's
//! (Données de référence → Notifications); the shipped defaults are documented in migration 209.
//!
//! Each stage runs in its own transaction: a stage that fails is rolled back and logged, the next
//! stages still run (one bad row never stops the notifications of a device).
//!
//! Each device runs this sweep over its own copy of the notifications: un-snoozing, watchers,
//! resolution, escalation and expiry are not sent (capture suppressed). Escalated notifications
//! get the same id on every device (`emitter::insert_notification_for`), so a recipient's
//! acknowledgement reaches all of them.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use tokio::time::{interval, Duration};

use crate::activity::emitter::{emit_activity_event, ActivityEventInput};
use crate::notifications::emitter::{self, Reason};
use crate::notifications::{conditions, delivery, router, watchers, Result, SqlitePool};
use crate::sync::capture::{begin_suppression, end_suppression};

/// Retention of a notification before it expires, when the plant did not set
/// `notifications.expiry_days` (tenant setting, Données de référence → Notifications).
pub const DEFAULT_EXPIRY_DAYS: i64 = 180;
pub const EXPIRY_SETTING_KEY: &str = "notifications.expiry_days";
/// Bounds of the retention setting.
pub const EXPIRY_DAYS_RANGE: (i64, i64) = (7, 3650);

pub async fn start_notification_scheduler(pool: SqlitePool) {
    let mut ticker = interval(Duration::from_secs(60));

    loop {
        ticker.tick().await;
        if let Err(err) = run_scheduler_tick(&pool).await {
            tracing::error!(error = %err, "notifications::scheduler tick failed");
        }
    }
}

/// One stage of the sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Unsnooze,
    Watch(&'static str),
    Resolve,
    Escalate,
    Expire,
}

/// The stages of one sweep, in order.
pub(crate) fn stages() -> Vec<Stage> {
    let mut out = vec![Stage::Unsnooze];
    out.extend(watchers::WATCHERS.iter().map(|w| Stage::Watch(w)));
    out.extend([Stage::Resolve, Stage::Escalate, Stage::Expire]);
    out
}

#[cfg(test)]
pub async fn run_scheduler_tick_for_test(pool: &SqlitePool) -> Result<()> {
    run_scheduler_tick(pool).await
}

/// One sweep: every stage in its own transaction with capture suppressed. A failing stage is
/// rolled back and logged; the next ones still run.
async fn run_scheduler_tick(pool: &SqlitePool) -> Result<()> {
    let fr = crate::pm::wo_content::tenant_text_locale(pool)
        .await
        .map(|l| l.to_ascii_lowercase().starts_with("fr"))
        .unwrap_or(true);
    let now = crate::clock::now();
    for stage in stages() {
        let txn = pool.begin().await?;
        begin_suppression(&txn).await?;
        match run_stage(&txn, stage, now, fr).await {
            Ok(()) => {
                end_suppression(&txn).await?;
                txn.commit().await?;
            }
            Err(err) => {
                tracing::error!(?stage, error = %err, "notifications::scheduler stage failed");
                txn.rollback().await?;
            }
        }
    }
    Ok(())
}

/// The sweep on one connection (tests): a failing stage is logged and the next ones run.
#[cfg(test)]
pub(crate) async fn run_scheduler_tick_in(pool: &impl ConnectionTrait, now: DateTime<Utc>, fr: bool) -> Result<()> {
    for stage in stages() {
        if let Err(err) = run_stage(pool, stage, now, fr).await {
            tracing::error!(?stage, error = %err, "notifications::scheduler stage failed");
        }
    }
    Ok(())
}

pub(crate) async fn run_stage(pool: &impl ConnectionTrait, stage: Stage, now: DateTime<Utc>, fr: bool) -> Result<()> {
    match stage {
        Stage::Unsnooze => {
            pool.execute(Statement::from_string(
                DbBackend::Sqlite,
                "UPDATE notifications
                 SET delivery_state = 'delivered', read_at = NULL, snoozed_until = NULL
                 WHERE delivery_state = 'snoozed'
                   AND snoozed_until IS NOT NULL
                   AND snoozed_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now')"
                    .to_string(),
            ))
            .await?;
        }
        Stage::Watch(watcher) => {
            let r = watchers::run_watcher(pool, watcher, now, fr).await?;
            if r.raised + r.already_true + r.resolved > 0 {
                tracing::info!(
                    watcher,
                    raised = r.raised,
                    already_true = r.already_true,
                    resolved = r.resolved,
                    "notifications::watchers round"
                );
            }
        }
        Stage::Resolve => {
            conditions::resolve_cleared(pool).await?;
        }
        Stage::Escalate => {
            run_escalation_sweep(pool, now).await?;
        }
        Stage::Expire => {
            let expiry_days = expiry_days(pool).await?;
            let expiry_modifier = format!("-{expiry_days} days");
            pool.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                concat!(
                    "UPDATE notifications
                 SET delivery_state = 'expired'
                 WHERE delivery_state IN ('pending', 'delivered', 'read', 'escalated')
                   AND created_at < strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', ?))
                   -- A safety notice stays until acknowledged (`settings::received_notice`).
                   AND NOT EXISTS (SELECT 1 FROM notification_events e
                                   WHERE e.id = notifications.notification_event_id
                                     AND e.category_code IN ",
                    crate::safety_notice_categories_sql!(),
                    ")"
                ),
                [expiry_modifier.into()],
            ))
            .await?;
        }
    }
    Ok(())
}

/// One event waiting for an acknowledgement.
struct Pending {
    event_id: i64,
    category: String,
    source_module: String,
    source_record_id: Option<String>,
    payload_json: Option<String>,
    policy_id: i64,
    first_at: String,
    level: i64,
}

pub(crate) async fn run_escalation_sweep(pool: &impl ConnectionTrait, now: DateTime<Utc>) -> Result<u64> {
    let rows = pool
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT e.id AS event_id, e.category_code, e.source_module, e.source_record_id, e.payload_json,
                    r.escalation_policy_id, MIN(n.created_at) AS first_at, MAX(n.escalation_level) AS level
             FROM notification_events e
             JOIN notification_rules r ON r.category_code = e.category_code
             JOIN notifications n ON n.notification_event_id = e.id
             WHERE r.is_active = 1 AND r.requires_ack = 1 AND r.escalation_policy_id IS NOT NULL
               AND e.resolved_at IS NULL
             GROUP BY e.id
             HAVING SUM(CASE WHEN n.acknowledged_at IS NOT NULL THEN 1 ELSE 0 END) = 0
                AND SUM(CASE WHEN n.delivery_state IN ('expired', 'closed') THEN 0 ELSE 1 END) > 0"
                .to_string(),
        ))
        .await?;
    let mut pending = Vec::with_capacity(rows.len());
    for r in &rows {
        pending.push(Pending {
            event_id: r.try_get("", "event_id")?,
            category: r.try_get("", "category_code")?,
            source_module: r.try_get("", "source_module")?,
            source_record_id: r.try_get("", "source_record_id")?,
            payload_json: r.try_get("", "payload_json")?,
            policy_id: r.try_get("", "escalation_policy_id")?,
            first_at: r.try_get("", "first_at")?,
            level: r.try_get("", "level")?,
        });
    }

    let mut escalated = 0;
    for p in pending {
        let Some(step) = pool
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT level_no, wait_minutes, route_to_type, route_to_reference FROM notification_escalation_steps
                 WHERE escalation_policy_id = ? AND level_no = ?",
                [p.policy_id.into(), (p.level + 1).into()],
            ))
            .await?
        else {
            continue;
        };
        let level: i64 = step.try_get("", "level_no")?;
        let wait: i64 = step.try_get("", "wait_minutes")?;
        let target: String = step.try_get("", "route_to_type")?;
        let reference: Option<String> = step.try_get("", "route_to_reference")?;
        let Ok(first) = DateTime::parse_from_rfc3339(&p.first_at) else {
            continue;
        };
        if now.signed_duration_since(first.with_timezone(&Utc)).num_minutes() < wait {
            continue;
        }
        // One event that cannot escalate (bad row, missing record) does not hold the others back.
        match escalate(pool, &p, level, wait, &target, reference.as_deref()).await {
            Ok(()) => escalated += 1,
            Err(err) => tracing::error!(event_id = p.event_id, level, error = %err, "notifications: escalation failed"),
        }
    }
    Ok(escalated)
}

/// Users of an escalation target for the event.
async fn escalation_targets(
    pool: &impl ConnectionTrait,
    p: &Pending,
    target: &str,
    reference: Option<&str>,
    notified: &[i64],
) -> Result<Vec<i64>> {
    let mut payload = p
        .payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("source_module".into(), p.source_module.clone().into());
        obj.insert(
            "source_record_id".into(),
            p.source_record_id.clone().map_or(serde_json::Value::Null, Into::into),
        );
    }
    let ctx = router::load_context(pool, &payload).await?;
    Ok(match target {
        "supervisor" => {
            let mut users: Vec<i64> = ctx.assignees.clone();
            users.extend_from_slice(notified);
            router::supervisor_user_ids(pool, &ctx.person_ids, &users).await?
        }
        "planner" => ctx.planners.clone(),
        "team_lead" => match ctx.team_id {
            Some(team) => router::team_member_user_ids(pool, team, true).await?,
            None => Vec::new(),
        },
        "entity_manager" => match ctx.org_node_id {
            Some(node) => router::entity_manager_user_ids(pool, node).await?,
            None => Vec::new(),
        },
        "permission" => match reference {
            Some(code) => router::permission_holder_user_ids(pool, code, ctx.org_node_id).await?,
            None => Vec::new(),
        },
        "role" => match reference {
            Some(name) => router::role_user_ids(pool, name).await?,
            None => Vec::new(),
        },
        "user" => reference.and_then(|r| r.parse::<i64>().ok()).into_iter().collect(),
        other => {
            tracing::warn!(target = other, "notifications::scheduler unknown escalation target");
            Vec::new()
        }
    })
}

async fn escalate(
    pool: &impl ConnectionTrait,
    p: &Pending,
    level: i64,
    wait: i64,
    target: &str,
    reference: Option<&str>,
) -> Result<()> {
    let originals = pool
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT recipient_user_id, title, body, action_url FROM notifications
             WHERE notification_event_id = ? ORDER BY escalation_level, id",
            [p.event_id.into()],
        ))
        .await?;
    let notified: Vec<i64> = originals
        .iter()
        .filter_map(|r| r.try_get::<Option<i64>>("", "recipient_user_id").ok().flatten())
        .collect();
    let Some(first) = originals.first() else {
        return Ok(());
    };
    let title: String = first.try_get("", "title")?;
    let body: Option<String> = first.try_get("", "body")?;
    let action_url: Option<String> = first.try_get("", "action_url")?;

    let mut users = escalation_targets(pool, p, target, reference, &notified).await?;
    users.retain(|u| !notified.contains(u));
    users.sort_unstable();
    users.dedup();

    let reason_ref = format!("{level}:{wait}");
    let mut created = 0;
    for user_id in &users {
        let Some(new_id) = emitter::insert_notification_for(
            pool,
            p.event_id,
            *user_id,
            &title,
            body.as_deref(),
            action_url.as_deref(),
            level,
            Some(Reason {
                code: "escalation",
                reference: Some(&reason_ref),
            }),
        )
        .await?
        else {
            continue;
        };
        created += 1;
        if let Err(err) = delivery::schedule_deliveries(pool, new_id).await {
            tracing::error!(notification_id = new_id, error = %err, "notifications::scheduler escalation delivery failed");
        }
    }

    // The event reaches this level even when the step found nobody: the next step can fire.
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications SET escalation_level = ? WHERE notification_event_id = ? AND escalation_level < ?",
        [level.into(), p.event_id.into(), level.into()],
    ))
    .await?;

    if let Err(err) = emit_activity_event(
        pool,
        ActivityEventInput {
            event_class: "system".to_string(),
            event_code: "notification.escalated".to_string(),
            source_module: "notifications".to_string(),
            source_record_type: Some("notification_event".to_string()),
            source_record_id: Some(p.event_id.to_string()),
            entity_scope_id: None,
            actor_id: None,
            severity: if created == 0 { "warning" } else { "info" }.to_string(),
            summary_json: Some(serde_json::json!({
                "category_code": p.category,
                "level": level,
                "wait_minutes": wait,
                "target": target,
                "target_reference": reference,
                "recipients": created,
            })),
            correlation_id: Some(format!("notification-event-{}", p.event_id)),
            visibility_scope: "global".to_string(),
        },
    )
    .await
    {
        tracing::error!(event_id = p.event_id, level, error = %err, "notifications: escalation not logged");
    }
    if created == 0 {
        tracing::warn!(
            event_id = p.event_id,
            level,
            target,
            "notifications: escalation step found nobody to notify"
        );
    }
    Ok(())
}

/// Retention in days: the tenant setting, else [`DEFAULT_EXPIRY_DAYS`].
pub async fn expiry_days(pool: &impl ConnectionTrait) -> Result<i64> {
    Ok(stored_expiry_days(pool).await?.unwrap_or(DEFAULT_EXPIRY_DAYS))
}

/// The retention set by the plant, if any.
pub async fn stored_expiry_days(pool: &impl ConnectionTrait) -> Result<Option<i64>> {
    let row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json
             FROM app_settings
             WHERE setting_key = ?
               AND setting_scope = 'tenant'
             LIMIT 1",
            [EXPIRY_SETTING_KEY.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let raw_value = row.try_get::<String>("", "setting_value_json")?;
    let parsed: Option<serde_json::Value> = serde_json::from_str(&raw_value).ok();
    let days = parsed.and_then(|v| v.as_i64().or_else(|| v.get("days").and_then(serde_json::Value::as_i64)));
    if days.is_none() {
        tracing::warn!(raw_value, "notifications: unreadable retention setting, default used");
    }
    Ok(days.map(|d| d.clamp(EXPIRY_DAYS_RANGE.0, EXPIRY_DAYS_RANGE.1)))
}
