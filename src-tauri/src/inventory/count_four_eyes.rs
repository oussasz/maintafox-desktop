//! Four-eyes on stock count variances above the plant's tolerance (D11, CMMS Advisor decision
//! register 2026-10-22/23, migration 259).
//!
//! Tolerance. One plant-wide pair of optional settings (Settings > Inventory), read when a count is
//! entered, on every count path (count-session line, count from the stock-to-reconcile list):
//! - [`SETTING_VALUE_THRESHOLD`]: |variance| × the location's unit cost (average cost, plant
//!   currency);
//! - [`SETTING_PERCENT_THRESHOLD`]: |variance| / system quantity × 100.
//!
//! A variance is above the tolerance ("critique", `is_critical` on the line) when ANY threshold that
//! is set is exceeded — "exceeds" is strictly greater (`>`): a variance exactly at a threshold is
//! within it. A blank threshold (never set: no row) is ignored, never read as zero. Both blank:
//! strict, every variance is above. An article without a unit cost at the location — no average
//! cost, or a cost of exactly zero (or less): a value threshold cannot judge a variance worth
//! nothing — is judged on the percent only, and is strict when the percent is blank. A system
//! quantity of zero or less is always above (no percent can be computed). No variance is never
//! above.
//!
//! Approval. Every variance is approved with a note of at least 8 characters, as before. Within the
//! tolerance the counter may approve it. Above it, the approver is someone other than the counter
//! (`counted_by_id`, the signed-in person who entered the counted quantity), unless the plant's site
//! opted out of four-eyes (the same per-site rule as permits and reliability decisions,
//! `reliability::study::decisions::SELF_APPROVAL_SETTING`): stores have no site yet, so an
//! organisation with exactly ONE planning site follows that site's rule and any other organisation
//! applies four-eyes ([`store_rule`]). Under the opt-out the counter's approval is flagged
//! `self_approved` ("auto-approbation"), shown on screen and in the audit. A variance whose counter
//! was not recorded (entered before this version) cannot show the separation — whether or not it is
//! flagged above the tolerance: its flag was set under the retired rule — and is entered again.
//!
//! Synchronisation. A count line received from another PC is judged again here
//! ([`inbound_count_line_check`], as `permit::sync_check` judges a permit's approval): approved by its
//! counter above the tolerance and not flagged — or flagged where this PC's store rule is four-eyes —
//! is a sync conflict for a person to decide; while a change of the site's opt-out is still in the
//! inbox, or while no planning site is applied here yet, the line waits. What is held here is kept
//! (as `permit::sync_check` keeps a permit's author and approval): a received row cannot clear the
//! counter of a line held here, nor give it another counter or lower its "above the tolerance" flag
//! without a new entry (another counted or system quantity) — `COUNT_APPROVAL_RECORD_CHANGED`. A
//! re-entry by another person of the same quantity on another PC is therefore a conflict for a
//! person to decide, the safe direction.
//!
//! Held with its stock movement (CMMS Advisor Q8). Where this PC judges a received count line
//! four-eyes (its current site rule; under the opt-out here it is accepted on arrival, flagged), the
//! line is a `COUNT_SELF_APPROVAL` conflict AND the count's stock movements wait with it
//! ([`inbound_count_movement_hold`]): its `COUNT_ADJUST` and a `COUNT_REVERSAL` of it are not applied,
//! so the stock here does not move. The conflict is settled where the count is applied, never by
//! "keep mine" / "take theirs" here (refused, `sync::queries`):
//! - a second approval by someone other than the counter, holding the count-approval right
//!   (`controls::second_approve_count_line`): when that version arrives the line passes, its
//!   movement is applied with it and the conflict closes ([`after_count_line_applied`]);
//! - or the count's reversal: the held adjustment and its reversal are applied together (net zero),
//!   the conflict closes and the counter is told to count again.
//! When the conflict opens, the count-approval holders are told once per count
//! ([`after_count_conflict_recorded`]).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};
use crate::reliability::study::decisions::SelfApprovalRule;
use crate::settings::site::InboundVerdict;

/// Plant setting (optional decimal, plant currency): a variance whose value is above it needs a
/// second person.
pub const SETTING_VALUE_THRESHOLD: &str = "inventory.count_four_eyes_value_threshold";
/// Plant setting (optional decimal, percent of the system quantity): a variance above it needs a
/// second person.
pub const SETTING_PERCENT_THRESHOLD: &str = "inventory.count_four_eyes_percent_threshold";

/// Refusal: the counter approves a variance above the tolerance where four-eyes applies.
pub const REFUSED_SELF_APPROVAL: &str = "GATE_INV_COUNT_SELF_APPROVAL";
/// Refusal: a variance above the tolerance whose counter was not recorded (entered before the
/// update): the separation cannot be shown, the line is entered again.
pub const REFUSED_COUNTER_UNKNOWN: &str = "GATE_INV_COUNT_COUNTER_UNKNOWN";

/// Sync conflict: a received count line approved by its counter above the tolerance, not flagged as
/// a self-approval, or flagged where four-eyes applies on this PC.
pub const SYNC_SELF_APPROVAL: &str = "COUNT_SELF_APPROVAL";
/// Sync conflict: a received count line whose approval record contradicts itself.
pub const SYNC_APPROVAL_RECORD_INVALID: &str = "COUNT_APPROVAL_RECORD_INVALID";
/// Sync conflict: a received count line clears or changes the counter held here, or lowers its
/// "above the tolerance" flag, without a new entry; or changes the second approval held here.
pub const SYNC_APPROVAL_RECORD_CHANGED: &str = "COUNT_APPROVAL_RECORD_CHANGED";

/// Notification category: a count held back here for a second approval (migration 259), to the
/// count-approval holders, once per count.
pub const SECOND_APPROVAL_CATEGORY: &str = "stock_count_second_approval";

const LINES: &str = "inventory_count_lines";

/// Quantities closer than this are no variance.
const QTY_TOLERANCE: f64 = 1e-9;

/// The plant's four-eyes tolerance (`None`: blank).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CountTolerance {
    pub value_threshold: Option<f64>,
    pub percent_threshold: Option<f64>,
}

impl CountTolerance {
    /// The current plant settings.
    pub async fn read<C: ConnectionTrait>(db: &C) -> AppResult<Self> {
        Ok(Self {
            value_threshold: crate::settings::catalog::optional_decimal_setting(db, SETTING_VALUE_THRESHOLD).await?,
            percent_threshold: crate::settings::catalog::optional_decimal_setting(db, SETTING_PERCENT_THRESHOLD)
                .await?,
        })
    }

    /// Whether a variance is above the tolerance (see the module documentation). `unit_cost`: the
    /// location's unit cost, `None` when the article has none there; a cost of zero (or less) is no
    /// cost: the percent alone judges.
    pub fn is_above(&self, variance_qty: f64, system_qty: f64, unit_cost: Option<f64>) -> bool {
        let variance = variance_qty.abs();
        if variance <= QTY_TOLERANCE {
            return false;
        }
        if system_qty <= 0.0 {
            return true;
        }
        let by_value = match (self.value_threshold, unit_cost) {
            (Some(threshold), Some(cost)) if cost > 0.0 => Some(variance * cost > threshold),
            _ => None,
        };
        let by_percent = self
            .percent_threshold
            .map(|threshold| variance / system_qty * 100.0 > threshold);
        match (by_value, by_percent) {
            (None, None) => true,
            (value, percent) => value.unwrap_or(false) || percent.unwrap_or(false),
        }
    }
}

/// What the tolerance said of one count, kept in its audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToleranceVerdict {
    pub tolerance: CountTolerance,
    pub unit_cost: Option<f64>,
    pub above_tolerance: bool,
}

impl ToleranceVerdict {
    pub fn audit_json(&self) -> serde_json::Value {
        serde_json::json!({
            "value_threshold": self.tolerance.value_threshold,
            "percent_threshold": self.tolerance.percent_threshold,
            "unit_cost": self.unit_cost,
            "above_tolerance": self.above_tolerance,
            "rule": "above when any threshold set is exceeded (>); both blank: every variance",
        })
    }
}

/// The tolerance applied to a count of an article at a location, in the caller's transaction.
pub async fn evaluate<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
    variance_qty: f64,
    system_qty: f64,
) -> AppResult<ToleranceVerdict> {
    let tolerance = CountTolerance::read(conn).await?;
    let unit_cost = crate::inventory::valuation::location_average_cost(conn, article_id, location_id).await?;
    Ok(ToleranceVerdict {
        tolerance,
        unit_cost,
        above_tolerance: tolerance.is_above(variance_qty, system_qty, unit_cost),
    })
}

/// The four-eyes rule of the stores, and the site it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreRule {
    pub rule: SelfApprovalRule,
    /// The organisation's only planning site, when it has exactly one.
    pub site_id: Option<i64>,
    /// No planning site at all here (none applied yet on a PC still receiving the organisation).
    pub no_site: bool,
}

/// Stores have no site yet: an organisation with exactly one planning site (live node whose type
/// can own work, outside a draft structure, as `settings::site` lists them) follows that site's
/// opt-out; any other organisation applies four-eyes.
pub async fn store_rule<C: ConnectionTrait>(conn: &C) -> AppResult<StoreRule> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT n.id FROM org_nodes n
                 JOIN org_node_types t ON t.id = n.node_type_id AND t.can_own_work = 1
                 WHERE n.deleted_at IS NULL AND {}
                 LIMIT 2",
                crate::settings::site::NOT_A_DRAFT_NODE_SQL
            ),
        ))
        .await?;
    if rows.len() != 1 {
        return Ok(StoreRule {
            rule: SelfApprovalRule::SiteUnresolved,
            site_id: None,
            no_site: rows.is_empty(),
        });
    }
    let site: i64 = rows[0].try_get("", "id")?;
    let allowed =
        crate::settings::site::site_setting(conn, site, crate::reliability::study::decisions::SELF_APPROVAL_SETTING)
            .await?
            == Some(true);
    Ok(StoreRule {
        rule: if allowed {
            SelfApprovalRule::Allowed
        } else {
            SelfApprovalRule::FourEyes
        },
        site_id: Some(site),
        no_site: false,
    })
}

/// The four-eyes check of an approval of a counted variance by `approver_id`. Returns whether it is
/// a self-approval ("auto-approbation", allowed only under the store's opt-out); refused where
/// four-eyes applies, or when the counter of a variance was not recorded (whatever its flag: a line
/// entered before the update carries the flag of the retired rule, so its separation cannot be
/// shown).
pub async fn check_approval<C: ConnectionTrait>(
    conn: &C,
    has_variance: bool,
    above_tolerance: bool,
    counted_by_id: Option<i64>,
    approver_id: i64,
) -> AppResult<bool> {
    if !has_variance {
        return Ok(false);
    }
    let Some(counter) = counted_by_id else {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_COUNTER_UNKNOWN}: the person who counted this variance was not recorded (entered before the \
             update): enter the counted quantity again."
        )]));
    };
    if !above_tolerance || counter != approver_id {
        return Ok(false);
    }
    if store_rule(conn).await?.rule == SelfApprovalRule::Allowed {
        return Ok(true);
    }
    Err(AppError::ValidationFailed(vec![format!(
        "{REFUSED_SELF_APPROVAL}: you counted this variance, which is above the four-eyes tolerance: another \
         person approves it."
    )]))
}

fn field<'a>(p: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    crate::permit::sync_check::field(p, key)
}

fn flag(p: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    match p.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

fn conflict(code: &'static str, message: String) -> AppResult<Option<InboundVerdict>> {
    Ok(Some(InboundVerdict::Conflict { code, message }))
}

/// Four-eyes on a count line received by synchronisation (see the module documentation), judged on
/// the received row completed by the copy held here:
/// - a line approved by its counter (both recorded, the same person) above the tolerance is accepted
///   only flagged `self_approved` and where this PC's store rule is the opt-out of the only site;
///   a change of that site's opt-out still in the inbox makes it wait (either way);
/// - `self_approved` is set only on such a line (never without an approver, by a second person, or
///   within the tolerance);
/// - a line whose counter or approver is not recorded (entered on a PC before the update) passes;
/// - an approval held here as received passes (judged when it arrived);
/// - the record held here is kept: its counter is never cleared, and is changed, like its "above the
///   tolerance" flag is lowered, only with a new entry (another counted or system quantity).
pub(crate) async fn inbound_count_line_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    let local = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await?;
    let line = crate::permit::sync_check::overlay(local.as_ref(), received);
    let approver = field(&line, "approved_by_sync_id");
    let counter = field(&line, "counted_by_sync_id");
    let self_approved = flag(&line, "self_approved");
    let above = flag(&line, "is_critical");
    if let Some(l) = &local {
        let quantity =
            |p: &serde_json::Map<String, serde_json::Value>, key: &str| p.get(key).and_then(serde_json::Value::as_f64);
        let same_quantities =
            ["counted_qty", "system_qty", "variance_qty"]
                .iter()
                .all(|k| match (quantity(l, k), quantity(&line, k)) {
                    (Some(held), Some(got)) => (held - got).abs() <= QTY_TOLERANCE,
                    (held, got) => held.is_none() && got.is_none(),
                });
        let held_counter = field(l, "counted_by_sync_id");
        if held_counter.is_some() && counter.is_none() {
            return conflict(
                SYNC_APPROVAL_RECORD_CHANGED,
                format!("count line {entity_sync_id}: the person who counted it, recorded here, cannot be removed"),
            );
        }
        if same_quantities && held_counter.is_some() && held_counter != counter {
            return conflict(
                SYNC_APPROVAL_RECORD_CHANGED,
                format!(
                    "count line {entity_sync_id}: the person who counted it, recorded here, changed without a new \
                     entry of the counted quantity"
                ),
            );
        }
        let held_second = field(l, "second_approved_by_sync_id");
        if held_second.is_some()
            && (field(&line, "second_approved_by_sync_id") != held_second
                || field(&line, "second_approved_at") != field(l, "second_approved_at"))
        {
            return conflict(
                SYNC_APPROVAL_RECORD_CHANGED,
                format!(
                    "count line {entity_sync_id}: its second approval, recorded here, cannot be removed or changed"
                ),
            );
        }
        if same_quantities && flag(l, "is_critical") && !above {
            return conflict(
                SYNC_APPROVAL_RECORD_CHANGED,
                format!(
                    "count line {entity_sync_id}: its variance, above the four-eyes tolerance here, was marked within \
                     it without a new entry of the counted quantity"
                ),
            );
        }
        let same = same_quantities
            && [
                "approved_by_sync_id",
                "approved_at",
                "counted_by_sync_id",
                "second_approved_by_sync_id",
                "second_approved_at",
                "reversed_transaction_sync_id",
            ]
            .iter()
            .all(|k| field(l, k) == field(&line, k))
            && flag(l, "self_approved") == self_approved
            && flag(l, "is_critical") == above;
        if same {
            return Ok(None);
        }
    }
    // A count reversed (its reversal movement is applied before the line, a reference it needs):
    // nothing it moved is kept, four-eyes has nothing to hold.
    if field(&line, "reversed_transaction_sync_id").is_some() {
        return Ok(None);
    }
    let by_counter = approver.is_some() && counter.is_some() && approver == counter;
    let second = field(&line, "second_approved_by_sync_id");
    if second.is_some() && !(by_counter && above) {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!(
                "count line {entity_sync_id} carries a second approval although it was not approved by its counter \
                 above the four-eyes tolerance"
            ),
        );
    }
    if let (Some(second), true) = (second, by_counter && above) {
        return second_approval_check(conn, entity_sync_id, &line, second).await;
    }
    if !(by_counter && above) {
        if self_approved {
            return conflict(
                SYNC_APPROVAL_RECORD_INVALID,
                format!(
                    "count line {entity_sync_id} is flagged as a self-approval but was not approved by its counter \
                     above the four-eyes tolerance"
                ),
            );
        }
        return Ok(None);
    }
    let approved_at = field(&line, "approved_at").unwrap_or("?");
    if !self_approved {
        return conflict(
            SYNC_SELF_APPROVAL,
            format!(
                "count line {entity_sync_id} was approved on {approved_at} by the person who counted it, above the \
                 four-eyes tolerance, without being recorded as a self-approval"
            ),
        );
    }
    let store = store_rule(conn).await?;
    match (store.rule, store.site_id) {
        (SelfApprovalRule::Allowed, Some(site)) => {
            if crate::permit::sync_check::site_rule_change_waiting(conn, site, false).await? {
                return Ok(Some(InboundVerdict::Defer(format!(
                    "count line {entity_sync_id}: self-approved; the site's return to four-eyes is still arriving"
                ))));
            }
            Ok(None)
        }
        (SelfApprovalRule::FourEyes, Some(site)) => {
            if crate::permit::sync_check::site_rule_change_waiting(conn, site, true).await? {
                return Ok(Some(InboundVerdict::Defer(format!(
                    "count line {entity_sync_id}: self-approved; the site's four-eyes opt-out is still arriving"
                ))));
            }
            conflict(
                SYNC_SELF_APPROVAL,
                format!(
                    "count line {entity_sync_id} was self-approved on {approved_at}; the site's setting on this PC \
                     now: four-eyes (self-approval not allowed)"
                ),
            )
        }
        // No planning site here yet: the organisation is still arriving (a self-approval is only
        // ever recorded where there is exactly one site) — the line waits for it.
        _ if store.no_site => Ok(Some(InboundVerdict::Defer(format!(
            "count line {entity_sync_id}: self-approved; no site is applied here yet"
        )))),
        _ => conflict(
            SYNC_SELF_APPROVAL,
            format!(
                "count line {entity_sync_id} was self-approved on {approved_at}; this organisation does not have \
                 exactly one site, so four-eyes applies to the stores"
            ),
        ),
    }
}

/// A received second approval of a self-approved line: by someone other than the counter, dated,
/// and holding the count-approval right here (`inv.manage`, plant-wide as stores have no site).
async fn second_approval_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    line: &serde_json::Map<String, serde_json::Value>,
    second: &str,
) -> AppResult<Option<InboundVerdict>> {
    if Some(second) == field(line, "counted_by_sync_id") || field(line, "second_approved_at").is_none() {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!("count line {entity_sync_id}: its second approval is by its counter, or undated"),
        );
    }
    let Some(user) = crate::permit::sync_check::local_id(conn, "user_accounts", second).await? else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "count line {entity_sync_id}: its second approver {second} is not applied yet"
        ))));
    };
    if !crate::settings::site::holds_tenant_right(conn, user, crate::rbac::permissions::INV_MANAGE).await? {
        return conflict(
            SYNC_SELF_APPROVAL,
            format!(
                "count line {entity_sync_id}: its second approver does not hold the count-approval right here \
                 (inv.manage)"
            ),
        );
    }
    Ok(None)
}

/// Message of the recount notice of a count reversed where it was applied, held back here
/// (`notifications:messages.<key>`).
pub const RECOUNT_AFTER_REVERSAL_MESSAGE: &str = "stock_count_recount_reversed";

/// Movement types of a count that move the stock.
const COUNT_MOVEMENTS: [&str; 2] = ["COUNT_ADJUST", "COUNT_REVERSAL"];

/// The latest received version (waiting, or refused for a person) of the count line naming the
/// movement `movement_sync_id` under `key`.
async fn waiting_line_naming(
    conn: &impl ConnectionTrait,
    key: &str,
    movement_sync_id: &str,
) -> AppResult<Option<(String, serde_json::Map<String, serde_json::Value>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT entity_sync_id, payload_json FROM sync_inbox
                 WHERE entity_type = ? AND apply_status IN ('pending', 'deferred', 'conflict')
                   AND operation NOT IN ('delete', 'archive')
                   AND json_extract(payload_json, '$.{key}') = ?
                 ORDER BY row_version DESC, id DESC LIMIT 1"
            ),
            [LINES.into(), movement_sync_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let sync_id: String = row.try_get("", "entity_sync_id")?;
    let payload: String = row.try_get("", "payload_json")?;
    match serde_json::from_str::<serde_json::Value>(&payload)? {
        serde_json::Value::Object(map) => Ok(Some((sync_id, map))),
        _ => Ok(None),
    }
}

/// Whether this PC holds a `COUNT_ADJUST` of the received movement's count session, article and
/// location (any sync id).
async fn count_adjusted_here(
    conn: &impl ConnectionTrait,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<bool> {
    held_here(
        conn,
        received,
        "SELECT 1 AS a FROM inventory_transactions
         WHERE movement_type = 'COUNT_ADJUST' AND source_type = 'COUNT_SESSION' AND source_id = ?
           AND article_id = ? AND location_id = ? LIMIT 1",
    )
    .await
}

/// Whether this PC holds the count line of the received movement's count session, article and
/// location.
async fn count_line_held_here(
    conn: &impl ConnectionTrait,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<bool> {
    held_here(
        conn,
        received,
        "SELECT 1 AS a FROM inventory_count_lines WHERE session_id = ? AND article_id = ? AND location_id = ? LIMIT 1",
    )
    .await
}

/// Whether `sql` (bound to the local session, article and location of a received count movement)
/// finds a row here; `false` when one of them is not here.
async fn held_here(
    conn: &impl ConnectionTrait,
    received: &serde_json::Map<String, serde_json::Value>,
    sql: &'static str,
) -> AppResult<bool> {
    let local = |table: &'static str, key: &'static str| async move {
        match field(received, key) {
            Some(sync_id) => crate::permit::sync_check::local_id(conn, table, sync_id).await,
            None => Ok(None),
        }
    };
    let (Some(session), Some(article), Some(location)) = (
        local("inventory_count_sessions", "source_sync_id").await?,
        local("articles", "article_sync_id").await?,
        local("stock_locations", "location_sync_id").await?,
    ) else {
        return Ok(false);
    };
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [session.into(), article.into(), location.into()],
        ))
        .await?
        .is_some())
}

/// Whether a movement is here or waiting in the inbox.
async fn movement_here_or_waiting(conn: &impl ConnectionTrait, movement_sync_id: &str) -> AppResult<bool> {
    if crate::permit::sync_check::local_id(conn, "inventory_transactions", movement_sync_id)
        .await?
        .is_some()
    {
        return Ok(true);
    }
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'inventory_transactions' AND entity_sync_id = ?
               AND apply_status IN ('pending', 'deferred') LIMIT 1",
            [movement_sync_id.into()],
        ))
        .await?
        .is_some())
}

/// A received stock movement of a count (`COUNT_ADJUST`, `COUNT_REVERSAL` of a count session) is
/// held with its count line (CMMS Advisor Q8, see the module documentation): it waits while the
/// latest received version of its line is refused or waits here; a reversal waits for the
/// adjustment it undoes; an adjustment whose line arrives reversed is applied with its reversal
/// (net zero). A movement already here (a replay, the same posting received twice) is not held; a
/// movement whose line has not arrived yet waits for it.
pub(crate) async fn inbound_count_movement_hold(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    let Some(kind) = field(received, "movement_type").filter(|k| COUNT_MOVEMENTS.contains(k)) else {
        return Ok(None);
    };
    if field(received, "source_type") != Some("COUNT_SESSION")
        || crate::permit::sync_check::local_id(conn, "inventory_transactions", entity_sync_id)
            .await?
            .is_some()
    {
        return Ok(None);
    }
    // The count's adjustment is already applied here (under this id or another: the same count
    // posted on two PCs, `inventory::count_sync`): its line was judged when it arrived; the
    // movements of that count go on as before.
    if count_adjusted_here(conn, received).await? {
        return Ok(None);
    }
    let adjust = kind == "COUNT_ADJUST";
    let key = if adjust {
        "posted_transaction_sync_id"
    } else {
        "reversed_transaction_sync_id"
    };
    let Some((line_sync_id, line)) = waiting_line_naming(conn, key, entity_sync_id).await? else {
        // No received version names it, but the count's line is held here (judged when it arrived,
        // or entered and approved on this PC): the movement is applied as before.
        if count_line_held_here(conn, received).await? {
            return Ok(None);
        }
        return Ok(Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id}: its count line has not arrived yet (held with it, four-eyes)"
        ))));
    };
    if !adjust {
        // A reversal undoes the adjustment named by its line: applied once that one is here.
        let held = match field(&line, "posted_transaction_sync_id") {
            Some(posted) => crate::permit::sync_check::local_id(conn, "inventory_transactions", posted)
                .await?
                .is_none(),
            None => false,
        };
        return Ok(held.then(|| {
            InboundVerdict::Defer(format!(
                "count reversal {entity_sync_id}: the adjustment it undoes is held here with count line {line_sync_id}"
            ))
        }));
    }
    if let Some(reversal) = field(&line, "reversed_transaction_sync_id") {
        if movement_here_or_waiting(conn, reversal).await? {
            return Ok(None);
        }
    }
    Ok(match inbound_count_line_check(conn, &line_sync_id, &line).await? {
        None => None,
        Some(InboundVerdict::Defer(why)) => Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id} waits with its count line {line_sync_id}: {why}"
        ))),
        Some(InboundVerdict::Conflict { code, message }) => Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id} is held with its count line {line_sync_id} ({code}: {message}); \
                 settled where the count is applied: a second approval, or its reversal"
        ))),
    })
}

/// Display parameters of a count line notice: article, location, count and counter, read from what
/// is here (the line's payload names them by sync id).
async fn notice_params(
    conn: &impl ConnectionTrait,
    line: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let text = |sql: &'static str, sync_id: Option<String>| async move {
        let Some(sync_id) = sync_id else {
            return Ok::<Option<String>, AppError>(None);
        };
        Ok(conn
            .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [sync_id.into()]))
            .await?
            .and_then(|r| r.try_get::<Option<String>>("", "v").ok().flatten()))
    };
    let owned = |key: &str| field(line, key).map(str::to_string);
    let unknown = || "?".to_string();
    Ok(serde_json::json!({
        "code": text("SELECT article_code AS v FROM articles WHERE entity_sync_id = ?", owned("article_sync_id"))
            .await?
            .unwrap_or_else(unknown),
        "title": text("SELECT article_name AS v FROM articles WHERE entity_sync_id = ?", owned("article_sync_id"))
            .await?
            .unwrap_or_default(),
        "location": text(
            "SELECT w.code || '/' || sl.code AS v FROM stock_locations sl JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE sl.entity_sync_id = ?",
            owned("location_sync_id"),
        )
        .await?
        .unwrap_or_else(unknown),
        "session": text(
            "SELECT session_code AS v FROM inventory_count_sessions WHERE entity_sync_id = ?",
            owned("session_sync_id"),
        )
        .await?
        .unwrap_or_else(unknown),
        "counter": text(
            "SELECT COALESCE(NULLIF(TRIM(display_name), ''), username) AS v FROM user_accounts WHERE entity_sync_id = ?",
            owned("counted_by_sync_id"),
        )
        .await?
        .unwrap_or_else(unknown),
    }))
}

/// A device notice (`notifications::emitter::emit_device_event_with`): the same identity on every PC,
/// written with capture suppressed (each PC raises its own), fire-and-log.
#[allow(clippy::too_many_arguments)]
async fn device_notice(
    conn: &impl ConnectionTrait,
    category: &str,
    message_key: &str,
    identity: &str,
    source: Option<(&str, i64)>,
    params: serde_json::Value,
    title: String,
    body: String,
) -> AppResult<()> {
    let input = crate::notifications::emitter::NotificationEventInput {
        source_module: source.map_or("inventory", |(m, _)| m).into(),
        source_record_id: source.map(|(_, id)| id.to_string()),
        event_code: category.into(),
        category_code: category.into(),
        severity: "warning".into(),
        dedupe_key: Some(identity.to_string()),
        payload_json: None,
        title,
        body: Some(body),
        action_url: Some(crate::inventory::negative_stock::LIST_URL.to_string()),
    };
    let message = crate::notifications::emitter::NotificationMessage::new(message_key, params);
    // Inside a received row's apply capture is already suppressed: left as it is.
    let suppressed = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 AS s FROM sync_capture_suppression LIMIT 1".to_string(),
        ))
        .await?
        .is_some();
    if !suppressed {
        crate::sync::capture::begin_suppression(conn).await?;
    }
    let emitted =
        crate::notifications::emitter::emit_device_event_with(conn, input, Some(identity), Some(&message)).await;
    if !suppressed {
        crate::sync::capture::end_suppression(conn).await?;
    }
    if let Err(err) = emitted {
        tracing::warn!(error = %err, identity, "count notice not raised");
    }
    Ok(())
}

/// After a received count line's conflict was recorded (`sync::materialize`): a count held back
/// for a second approval tells the count-approval holders, once per count (its line's sync id names
/// the notice on every PC, a later refusal of the same line refreshes it).
pub(crate) async fn after_count_conflict_recorded(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    code: &str,
    payload_json: &str,
) -> AppResult<()> {
    if entity_type != LINES || code != SYNC_SELF_APPROVAL {
        return Ok(());
    }
    let Ok(serde_json::Value::Object(line)) = serde_json::from_str::<serde_json::Value>(payload_json) else {
        return Ok(());
    };
    let params = notice_params(conn, &line).await?;
    let text = |k: &str| {
        params
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?")
            .to_string()
    };
    device_notice(
        conn,
        SECOND_APPROVAL_CATEGORY,
        SECOND_APPROVAL_CATEGORY,
        &format!("count-held|{entity_sync_id}"),
        None,
        params.clone(),
        format!("Comptage retenu — {} à {}", text("code"), text("location")),
        format!(
            "Le comptage {} de {} (compté et approuvé par {}) est retenu ici : seconde approbation ou annulation du \
             comptage, sur le poste où il est appliqué.",
            text("session"),
            text("code"),
            text("counter")
        ),
    )
    .await
}

/// After a received count line was applied (`sync::materialize`): its `COUNT_SELF_APPROVAL`
/// conflicts of older versions close (the second approval or the reversal arrived and passed); a
/// reversed count tells its counter to count again.
pub(crate) async fn after_count_line_applied(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    applied_row_version: i64,
) -> AppResult<()> {
    if entity_type != LINES {
        return Ok(());
    }
    let open = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS o FROM sync_conflicts WHERE conflict_type = ? AND entity_type = ? AND entity_sync_id = ?
               AND inbound_row_version < ?
               AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed') LIMIT 1",
            [
                SYNC_SELF_APPROVAL.into(),
                LINES.into(),
                entity_sync_id.into(),
                applied_row_version.into(),
            ],
        ))
        .await?
        .is_some();
    if !open {
        return Ok(());
    }
    let Some(line) = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await? else {
        return Ok(());
    };
    let reversed = field(&line, "reversed_transaction_sync_id").is_some();
    crate::sync::code_clash::close_conflicts_superseded_by(
        conn,
        SYNC_SELF_APPROVAL,
        LINES,
        entity_sync_id,
        applied_row_version,
        if reversed {
            "The count was reversed where it is applied: its held adjustment and its reversal were applied together."
        } else {
            "The count received its second approval where it is applied: it was applied here with its movement."
        },
    )
    .await?;
    if reversed {
        let Some(line_id) = crate::permit::sync_check::local_id(conn, LINES, entity_sync_id).await? else {
            return Ok(());
        };
        let params = notice_params(conn, &line).await?;
        let text = |k: &str| {
            params
                .get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?")
                .to_string()
        };
        device_notice(
            conn,
            crate::inventory::negative_stock::RECOUNT_CATEGORY,
            RECOUNT_AFTER_REVERSAL_MESSAGE,
            &format!("count-recount|{entity_sync_id}"),
            Some(("inventory_count_lines", line_id)),
            serde_json::json!({
                "code": text("code"),
                "title": text("title"),
                "location": text("location"),
                "session": text("session"),
            }),
            format!("Recomptage demandé — {} à {}", text("code"), text("location")),
            format!(
                "Le comptage {} a été annulé faute de seconde approbation. Recomptez l'emplacement.",
                text("session")
            ),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CountTolerance;

    fn tolerance(value: Option<f64>, percent: Option<f64>) -> CountTolerance {
        CountTolerance {
            value_threshold: value,
            percent_threshold: percent,
        }
    }

    #[test]
    fn any_threshold_set_and_exceeded_makes_a_variance_above_the_tolerance() {
        // No variance is never above.
        assert!(!tolerance(None, None).is_above(0.0, 10.0, Some(2.0)));
        // Both blank: strict.
        assert!(tolerance(None, None).is_above(0.5, 10.0, Some(2.0)));
        // Value only: 3 × 2 = 6.
        assert!(tolerance(Some(5.0), None).is_above(-3.0, 10.0, Some(2.0)));
        assert!(
            !tolerance(Some(6.0), None).is_above(-3.0, 10.0, Some(2.0)),
            "exactly at it: within"
        );
        // Percent only: 3 / 10 = 30 %.
        assert!(tolerance(None, Some(29.0)).is_above(3.0, 10.0, None));
        assert!(
            !tolerance(None, Some(30.0)).is_above(3.0, 10.0, None),
            "exactly at it: within"
        );
        // Either exceeded.
        assert!(tolerance(Some(100.0), Some(20.0)).is_above(3.0, 10.0, Some(2.0)));
        assert!(tolerance(Some(5.0), Some(50.0)).is_above(3.0, 10.0, Some(2.0)));
        assert!(!tolerance(Some(10.0), Some(50.0)).is_above(3.0, 10.0, Some(2.0)));
    }

    #[test]
    fn without_unit_cost_only_the_percent_counts_and_blank_percent_is_strict() {
        assert!(!tolerance(Some(1.0), Some(50.0)).is_above(3.0, 10.0, None));
        assert!(tolerance(Some(1_000.0), None).is_above(3.0, 10.0, None));
    }

    #[test]
    fn a_unit_cost_of_exactly_zero_is_no_cost() {
        // Percent alone judges: 30 % is within 50 %, the value threshold is not read as "worth 0".
        assert!(!tolerance(Some(1.0), Some(50.0)).is_above(3.0, 10.0, Some(0.0)));
        assert!(tolerance(Some(1.0), Some(20.0)).is_above(3.0, 10.0, Some(0.0)));
        // Percent blank: strict, although 3 × 0 = 0 would be within any value threshold.
        assert!(tolerance(Some(1_000.0), None).is_above(3.0, 10.0, Some(0.0)));
    }

    #[test]
    fn a_system_quantity_of_zero_or_less_is_always_above() {
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(1.0, 0.0, Some(0.01)));
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(6.0, -5.0, Some(0.01)));
    }
}
