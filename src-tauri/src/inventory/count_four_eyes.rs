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
//! person to decide, the safe direction (CMMS Advisor Q11). A received version whose checked fields
//! are those held here (only its posting or another unchecked field changed) is accepted as already
//! judged: it is not judged again against this PC's tolerance or cost of today.
//!
//! "Above the tolerance" is judged again on arrival, from the line's own quantities (its variance
//! must be its counted less its system quantity, or the record is refused) and this PC's tolerance;
//! the received flag can only make it stricter. The unit cost is the one the line was entered with
//! (`entry_unit_cost`, kept with the variance's value `entry_variance_value`, migration 259) when
//! that value is the variance × that cost and the cost lies within [`ENTRY_COST_DRIFT`] (±10 %) of
//! this PC's average cost; otherwise this PC's cost (CMMS Advisor Q10). Thresholds differing
//! between PCs can still make a legitimate approval a conflict here: the safe direction.
//!
//! First join (CMMS Advisor Q9): a line new here received with the snapshot of a PC's first join is
//! the plant's history: applied as it is, with its movements, without a four-eyes judgement; its
//! self-approval flag is kept and shown with the other self-approved lines. Only a first join: once
//! this PC completed a full pull, or while a version of the line is held here (open conflict,
//! waiting), a snapshot row is judged as any other ([`first_join_history`]); a movement is judged
//! with its line in the line's own context (snapshot or exchange), not the movement's.
//!
//! Auto-approbation on read (CMMS Advisor Q13, Q14): a line whose approver is its counter and whose
//! variance is above the tolerance is shown and listed as an auto-approbation on every PC and in
//! every read model ([`SELF_APPROVED_ON_READ_SQL`]), whether or not it was flagged where it was
//! approved; within the tolerance it is not one (no second person was needed). Nothing stored is
//! rewritten but "above the tolerance" (`is_critical`): it is always this PC's judgement — made at
//! entry here, or for a received line the received flag OR this PC's recompute (with the Q10 cost
//! check), stored on apply ([`store_judged_critique`]), never the peer's flag alone.
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
//!
//! When the conflict opens, the count-approval holders are told once per count
//! ([`after_count_conflict_recorded`]).
//!
//! A received adjustment is released only by a line version naming it that is its own (the line's
//! count, article, location and variance). One that is not — a forged or corrupted record — is
//! never applied: the line naming it is a `COUNT_APPROVAL_RECORD_INVALID` conflict for a person,
//! whatever their order of arrival, and the adjustment stays held with it (a reversal of it is
//! refused with it, so it never moves the stock on its own) until that conflict is settled: a
//! later version of the line judged clean here, or "take theirs" once the adjustment is the line's
//! own. A reversal, even of a count whose adjustment is applied here, is applied with the line
//! version naming it, and only if it undoes that line's adjustment and, where the count's
//! adjustment is applied here, that count is not reversed here yet and the line held here names no
//! other reversal (otherwise a `COUNT_APPROVAL_RECORD_INVALID` conflict on that reversal: a count is
//! reversed once per posting). Only the count's own reversal (its seeded id) that no version of its
//! line names here is matched against the ledger instead: it must undo an adjustment of that count
//! held here, not reversed yet, or it is a `COUNT_APPROVAL_RECORD_INVALID` conflict on that
//! reversal. A count movement already applied here is never moved again by a later version of it
//! received from the server that changes its quantity (a `COUNT_APPROVAL_RECORD_INVALID` conflict
//! on that movement): only this PC's own posting, never synchronised, takes the version posted
//! first on another PC (`inventory::count_sync`).
//!
//! Known limit (AU-100): a `COUNT_ADJUST` under another id, of a count whose adjustment is already
//! applied here, is not checked against a line: it is applied as received, with any quantity, and
//! the count is listed as posted twice, to be checked by a person with a new count (Stock ›
//! Contrôles; the same count posted on two PCs under ids that differ, a legacy case). No line and
//! no four-eyes check stand behind that second adjustment.

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

/// CMMS Advisor Q13, Q14: whether a count line (`cl`) is shown as an auto-approbation, derived on
/// read: flagged `self_approved`, or a variance above the tolerance (`is_critical`, this PC's
/// judgement, [`store_judged_critique`]) approved by its own counter (both recorded). A variance
/// within the tolerance needs no second person (a count from the list records the person counting
/// as its reviewer): never one, nor a line without variance.
pub const SELF_APPROVED_ON_READ_SQL: &str = "(CASE WHEN cl.self_approved = 1
      OR (ABS(cl.variance_qty) > 1e-9 AND cl.is_critical = 1
          AND cl.approved_by_id IS NOT NULL AND cl.approved_by_id = cl.counted_by_id)
      THEN 1 ELSE 0 END)";

/// Quantities closer than this are no variance.
const QTY_TOLERANCE: f64 = 1e-9;

/// A received line's quantities agree when its variance and its counted minus system quantity are
/// closer than this (a variance is always written as counted − system).
const QUANTITY_AGREEMENT: f64 = 1e-6;

/// CMMS Advisor Q10 (decision register 2026-10-24): the unit cost a received count line was judged
/// with at entry (`entry_unit_cost`) is used again here only when it lies within ±10 % of this PC's
/// average cost of the article at the location (average costs differ a little between PCs, as each
/// averages the movements it holds); further off, it is not trusted and this PC's cost judges.
pub const ENTRY_COST_DRIFT: f64 = 0.10;

/// A received line's stored variance value agrees with its quantity × stored unit cost when they
/// differ by less than this share of the value (or than [`QUANTITY_AGREEMENT`], for a small one).
const VALUE_AGREEMENT: f64 = 1e-6;

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
    /// What the line keeps of its entry for the PCs that receive it (CMMS Advisor Q10): the unit
    /// cost read at the location (`entry_unit_cost`) and the variance's value at that cost
    /// (`entry_variance_value`, variance quantity × unit cost); both blank without a unit cost.
    pub fn entry_values(&self, variance_qty: f64) -> (Option<f64>, Option<f64>) {
        (self.unit_cost, self.unit_cost.map(|cost| variance_qty * cost))
    }

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
/// - a version whose checked fields (quantities, counter, approval, second approval, reversal,
///   flags) are those held here passes without a new judgement: the line was judged when it arrived
///   or was entered, and an edit of the tolerance or a move of the cost since does not judge it
///   again (a held "above" with a received "within" passed the lowering check first);
/// - the record held here is kept: its counter is never cleared, and is changed, like its "above the
///   tolerance" flag is lowered, only with a new entry (another counted or system quantity);
/// - its quantities agree (variance = counted − system), or the record contradicts itself;
/// - a line new here received with the snapshot of a first join is the plant's history (CMMS
///   Advisor Q9): applied as it is, no four-eyes judgement, its `self_approved` flag kept and shown
///   with the other self-approved lines for a manager to review — only before this PC completed a
///   full pull and while nothing of that line is held here ([`first_join_history`]);
/// - the count adjustment it names as posted, when it waits here, is its own, or the record
///   contradicts itself ([`forged_adjustment_named`]): a visible conflict, the adjustment held.
pub(crate) async fn inbound_count_line_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    if let Some(verdict) = forged_adjustment_named(conn, entity_sync_id, received).await? {
        return Ok(Some(verdict));
    }
    judge_count_line(conn, entity_sync_id, received).await
}

/// A received count line naming as its posted adjustment a count adjustment not applied here but
/// waiting in the inbox that is not the line's own ([`adjustment_mismatch`], against the line as it
/// is judged: the received row completed by the copy held here) contradicts itself: a
/// `COUNT_APPROVAL_RECORD_INVALID` conflict for a person (review of d6562b88, §5), never a silent
/// wait. The adjustment stays held ([`inbound_count_movement_hold`]: the line version naming it is
/// in conflict, or set aside by a person's decision), so the stock does not move; it is settled
/// where the count is applied. A movement of another kind named as posted is not held, so not
/// judged here; one not arrived yet is judged when it arrives (the line, waiting for it, is judged
/// again then).
async fn forged_adjustment_named(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    let local = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await?;
    let line = crate::permit::sync_check::overlay(local.as_ref(), received);
    let Some(posted) = field(&line, "posted_transaction_sync_id") else {
        return Ok(None);
    };
    if crate::permit::sync_check::local_id(conn, "inventory_transactions", posted)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let Some((_, movement)) = waiting_movement(conn, posted).await? else {
        return Ok(None);
    };
    if field(&movement, "movement_type") != Some("COUNT_ADJUST")
        || field(&movement, "source_type") != Some("COUNT_SESSION")
    {
        return Ok(None);
    }
    match adjustment_mismatch(&movement, &line) {
        Some(why) => conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!(
                "count line {entity_sync_id} names movement {posted} as its posted adjustment, but {why}: that \
                 movement is held here and the stock does not move"
            ),
        ),
        None => Ok(None),
    }
}

/// The judgement of a received count line ([`inbound_count_line_check`]) without the check of the
/// adjustment it names ([`forged_adjustment_named`]): [`inbound_count_movement_hold`] makes that
/// check itself, against the movement it holds.
async fn judge_count_line(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    let local = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await?;
    // CMMS Advisor Q9: the snapshot of a first join brings the plant's history, judged where it
    // was made (as `wo::closeout_sync` takes a work order new here from a snapshot) — only a first
    // join ([`first_join_history`]): a snapshot asked for again later (the server refused the
    // checkpoint, after a restore) brings back the lines held here too, judged as any other.
    if local.is_none()
        && crate::sync::materialize::applying_snapshot_row()
        && first_join_history(conn, entity_sync_id).await?
    {
        return Ok(None);
    }
    let line = crate::permit::sync_check::overlay(local.as_ref(), received);
    let approver = field(&line, "approved_by_sync_id");
    let counter = field(&line, "counted_by_sync_id");
    let self_approved = flag(&line, "self_approved");
    if let Some(l) = &local {
        let same_quantities = same_quantities(l, &line);
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
        // Lowered: the flag held here (this PC's judgement) is cleared by the received version
        // without a new entry. Not when this PC raised it itself from the "within" of the last
        // version applied here (from any PC: the inbox does not record a sender) and the line has
        // not changed here since ([`critique_raised_here`]): the sender's own flag again, accepted;
        // the flag held stays raised ([`store_judged_critique`]). A flag entered here, or received
        // as "above", is never lowered so.
        if same_quantities
            && flag(l, "is_critical")
            && !flag(&line, "is_critical")
            && !critique_raised_here(conn, entity_sync_id).await?
        {
            return conflict(
                SYNC_APPROVAL_RECORD_CHANGED,
                format!(
                    "count line {entity_sync_id}: its variance, above the four-eyes tolerance here, was marked within \
                     it without a new entry of the counted quantity"
                ),
            );
        }
        // A held "above" over a received "within" passed the lowering check above.
        if already_judged(l, &line) {
            return Ok(None);
        }
    }
    // "Above the tolerance" is judged here again, from this PC's tolerance and the line's own
    // quantities: a received flag can make a line stricter, never more lenient.
    let above = flag(&line, "is_critical") || above_here(conn, &line).await?;
    // The variance is what "above the tolerance" is judged on: it must be the counted quantity less
    // the system quantity (a variance of 0 with another counted quantity would hide a variance).
    if let Some(why) = quantities_disagree(&line) {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!("count line {entity_sync_id}: {why}"),
        );
    }
    // A count reversed (its reversal movement is applied before the line, a reference it needs, or
    // with its held adjustment, [`count_movement_group`]): nothing it moved is kept, four-eyes has
    // nothing to hold — once the movement it names is shown to be the reversal of this count.
    if let Some(reversal) = field(&line, "reversed_transaction_sync_id") {
        return reversal_check(conn, entity_sync_id, &line, reversal).await;
    }
    let by_counter = approver.is_some() && counter.is_some() && approver == counter;
    let second = field(&line, "second_approved_by_sync_id");
    // A second approval is given to any variance approved by its counter (CMMS Advisor Q15): needed
    // above the tolerance, optional within it where it was posted (above it here, it settles it).
    let has_variance = line
        .get("variance_qty")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|v| v.abs() > QTY_TOLERANCE);
    if second.is_some() && !(by_counter && has_variance) {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!(
                "count line {entity_sync_id} carries a second approval although it is not a variance approved by \
                 its counter"
            ),
        );
    }
    if let Some(second) = second {
        return second_approval_check(conn, entity_sync_id, &line, second, above).await;
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

/// Whether a count line's quantities (counted, system, variance) are the same in both versions.
fn same_quantities(
    held: &serde_json::Map<String, serde_json::Value>,
    line: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let quantity =
        |p: &serde_json::Map<String, serde_json::Value>, key: &str| p.get(key).and_then(serde_json::Value::as_f64);
    ["counted_qty", "system_qty", "variance_qty"]
        .iter()
        .all(|k| match (quantity(held, k), quantity(line, k)) {
            (Some(held), Some(got)) => (held - got).abs() <= QTY_TOLERANCE,
            (held, got) => held.is_none() && got.is_none(),
        })
}

/// Whether a received version (`line`: the received row completed by the one held here) is one this
/// PC already judged (review of b4f8b2fd, B1): its checked fields (quantities, the count, article
/// and location it belongs to, counter, approval, whether approval is required, second approval,
/// reversal, self-approval flag) are those held here, and its "above the tolerance" flag is not
/// above the one held here. A version of the line under another count, article or location (whose
/// cost judges it) is judged again (review of 5ead90e8, S1). The flag held is compared with the received
/// one, never with a judgement of now: an edit of the tolerance or a move of the cost since does
/// not judge the line again ([`inbound_count_line_check`] accepts it, [`store_judged_critique`]
/// keeps the flag held). A received "above" over a held "within" is judged.
fn already_judged(
    held: &serde_json::Map<String, serde_json::Value>,
    line: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    same_quantities(held, line)
        && [
            "session_sync_id",
            "article_sync_id",
            "location_sync_id",
            "approved_by_sync_id",
            "approved_at",
            "counted_by_sync_id",
            "second_approved_by_sync_id",
            "second_approved_at",
            "reversed_transaction_sync_id",
        ]
        .iter()
        .all(|k| field(held, k) == field(line, k))
        && ["self_approved", "approval_required"]
            .iter()
            .all(|k| flag(held, k) == flag(line, k))
        && (flag(held, "is_critical") || !flag(line, "is_critical"))
}

/// What a received count line's write must know of the line held here, read before the write
/// (`sync::apply_hooks::held_before_write`): the "above the tolerance" flag held, when the
/// received version is one this PC already judged ([`already_judged`]); `None` otherwise (new
/// here, or changed: judged again after the write).
pub(crate) async fn held_judgement(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<bool>> {
    let Some(local) = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await? else {
        return Ok(None);
    };
    let line = crate::permit::sync_check::overlay(Some(&local), received);
    Ok(already_judged(&local, &line).then(|| flag(&local, "is_critical")))
}

/// Whether a count line new here, received with a snapshot, is taken as the plant's history (CMMS
/// Advisor Q9, review of 536cb792): only while this PC has not completed its first full pull
/// (`sync::plant_join::complete_pull_done`) AND nothing of that line is held here — no open sync
/// conflict and no version waiting or refused from an exchange. A snapshot asked for again (a
/// re-bootstrap after the server refused the checkpoint) brings back a line held here in conflict,
/// with its held adjustment: it is judged again, never applied unjudged.
async fn first_join_history(conn: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<bool> {
    if crate::sync::plant_join::complete_pull_done(conn).await? {
        return Ok(false);
    }
    let held = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS h WHERE EXISTS (
                     SELECT 1 FROM sync_conflicts WHERE entity_type = ? AND entity_sync_id = ?
                        AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed'))
                 OR EXISTS (
                     SELECT 1 FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ?
                        AND apply_status IN ('deferred', 'conflict')
                        AND server_batch_id NOT LIKE '{}%')",
                crate::sync::materialize::SNAPSHOT_BATCH_PREFIX
            ),
            [LINES.into(), entity_sync_id.into(), LINES.into(), entity_sync_id.into()],
        ))
        .await?
        .is_some();
    Ok(!held)
}

/// After a received count line was written (`sync::apply_hooks::after_upsert`, capture
/// suppressed): its "above the tolerance" flag is this PC's judgement (CMMS Advisor Q14) — the
/// received flag OR [`above_here`] (the line's quantities, this PC's tolerance, the Q10 cost
/// check) — so every read of it, the derived auto-approbation first, rests on that judgement, never
/// on the peer's flag alone. This hook never lowers the flag, and a received "within" over a flag
/// this PC raised itself ([`critique_raised_here`]) without a new entry leaves it raised: it falls
/// only with a new entry judged within (or a local entry within).
///
/// A raise is marked on the line (`critique_raised_version`, device-local, never sent: migration
/// 259, `sync::row_spec`) with the row version it was made at; any other outcome clears the mark.
///
/// `held`: the flag held here before the write when the received version was one this PC already
/// judged ([`held_judgement`]). That version is not judged again with today's tolerance or cost:
/// the flag held is kept as it was judged when the line arrived or was entered — a flag this PC
/// raised over a received "within" stays raised (and marked), a "within" is not raised.
pub(crate) async fn store_judged_critique(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    held: Option<bool>,
) -> AppResult<()> {
    let Some(line) = crate::permit::sync_check::local_wire(conn, LINES, entity_sync_id).await? else {
        return Ok(());
    };
    let counted = |key: &str| line.get(key).and_then(serde_json::Value::as_f64).is_some();
    let raise = match held {
        // Already judged: kept, never judged again (a received "within" over a held "above").
        Some(held) => held && !flag(&line, "is_critical"),
        // A line not counted yet has no variance to judge.
        None => {
            counted("counted_qty")
                && counted("system_qty")
                && !flag(&line, "is_critical")
                && above_here(conn, &line).await?
        }
    };
    let sql = if raise {
        "UPDATE inventory_count_lines SET is_critical = 1, critique_raised_version = row_version
         WHERE entity_sync_id = ?"
    } else {
        "UPDATE inventory_count_lines SET critique_raised_version = NULL
         WHERE entity_sync_id = ? AND critique_raised_version IS NOT NULL"
    };
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [entity_sync_id.into()],
    ))
    .await?;
    Ok(())
}

/// Whether the "above the tolerance" flag held here on a count line is one this PC raised itself
/// from a received "within" ([`store_judged_critique`]) and the line has not changed here since:
/// its mark names the line's current row version. Every change of the line bumps `row_version` (an
/// entry or an approval here, another received version applied), so a flag entered here, or
/// received as "above", never carries a matching mark.
async fn critique_raised_here(conn: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS raised FROM inventory_count_lines
             WHERE entity_sync_id = ? AND is_critical = 1 AND critique_raised_version = row_version",
            [entity_sync_id.into()],
        ))
        .await?
        .is_some())
}

/// Why a received line's variance is not its counted less its system quantity (`None`: they
/// agree, or one of them is not given — then judged strictly, [`above_here`]).
fn quantities_disagree(line: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let number = |key: &str| line.get(key).and_then(serde_json::Value::as_f64);
    let (Some(counted), Some(system), Some(variance)) =
        (number("counted_qty"), number("system_qty"), number("variance_qty"))
    else {
        return None;
    };
    ((variance - (counted - system)).abs() > QUANTITY_AGREEMENT).then(|| {
        format!("its variance {variance} is not its counted quantity {counted} less its system quantity {system}")
    })
}

/// The unit cost a received line is judged with here (CMMS Advisor Q10): the cost stored on the line
/// at entry when it is consistent — its stored variance value is the variance × that cost — and
/// within [`ENTRY_COST_DRIFT`] of this PC's average cost; otherwise this PC's cost (the strict side:
/// a cost not trusted never judges). No cost here: none (the percent alone judges, as at entry).
fn judged_unit_cost(
    line: &serde_json::Map<String, serde_json::Value>,
    variance: f64,
    cost_here: Option<f64>,
) -> Option<f64> {
    let here = cost_here?;
    let number = |key: &str| line.get(key).and_then(serde_json::Value::as_f64);
    let (Some(stored_cost), Some(stored_value)) = (number("entry_unit_cost"), number("entry_variance_value")) else {
        return Some(here);
    };
    let expected = variance * stored_cost;
    let consistent = (stored_value - expected).abs() <= QUANTITY_AGREEMENT.max(VALUE_AGREEMENT * expected.abs());
    let close = (stored_cost - here).abs() <= ENTRY_COST_DRIFT * here.abs();
    Some(if consistent && close { stored_cost } else { here })
}

/// Whether a received line's variance is above this PC's four-eyes tolerance, judged on its own
/// quantities (its counted less its system quantity) and the unit cost [`judged_unit_cost`] gives
/// (`evaluate`, as at entry). Quantities that cannot be read are above (strict).
async fn above_here(conn: &impl ConnectionTrait, line: &serde_json::Map<String, serde_json::Value>) -> AppResult<bool> {
    let number = |key: &str| line.get(key).and_then(serde_json::Value::as_f64);
    let (Some(counted), Some(system)) = (number("counted_qty"), number("system_qty")) else {
        return Ok(true);
    };
    let variance = counted - system;
    let local = |table: &'static str, key: &'static str| async move {
        match field(line, key) {
            Some(sync_id) => crate::permit::sync_check::local_id(conn, table, sync_id).await,
            None => Ok(None),
        }
    };
    let unit_cost = match (
        local("articles", "article_sync_id").await?,
        local("stock_locations", "location_sync_id").await?,
    ) {
        (Some(article), Some(location)) => {
            crate::inventory::valuation::location_average_cost(conn, article, location).await?
        }
        _ => None,
    };
    let unit_cost = judged_unit_cost(line, variance, unit_cost);
    Ok(CountTolerance::read(conn).await?.is_above(variance, system, unit_cost))
}

/// The four-eyes rule of the count screens: the plant's tolerance and whether the stores' site
/// opted out of four-eyes (the counter may then approve, flagged "auto-approbation").
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CountToleranceView {
    pub value_threshold: Option<f64>,
    pub percent_threshold: Option<f64>,
    pub self_approval_allowed: bool,
}

impl CountToleranceView {
    pub async fn read<C: ConnectionTrait>(db: &C) -> AppResult<Self> {
        let tolerance = CountTolerance::read(db).await?;
        Ok(Self {
            value_threshold: tolerance.value_threshold,
            percent_threshold: tolerance.percent_threshold,
            self_approval_allowed: store_rule(db).await?.rule == SelfApprovalRule::Allowed,
        })
    }
}

/// A received second approval of a self-approved line: by someone other than the counter, dated,
/// and — when it settles a variance above the tolerance here (`above`) — by a person holding the
/// count-approval right here (`inv.manage`, plant-wide as stores have no site). Within the tolerance
/// here no second person is needed: the optional second approval (CMMS Advisor Q15) is only checked
/// to be well formed.
async fn second_approval_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    line: &serde_json::Map<String, serde_json::Value>,
    second: &str,
    above: bool,
) -> AppResult<Option<InboundVerdict>> {
    if Some(second) == field(line, "counted_by_sync_id") || field(line, "second_approved_at").is_none() {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!("count line {entity_sync_id}: its second approval is by its counter, or undated"),
        );
    }
    if !above {
        return Ok(None);
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

/// A received version of a count line naming a movement.
struct NamingLine {
    sync_id: String,
    payload: serde_json::Map<String, serde_json::Value>,
    /// The inbox batch it came with: the line is judged in its own context (snapshot or exchange).
    server_batch_id: String,
    /// Its inbox status: `pending`, `deferred`, or `conflict` (refused for a person).
    apply_status: String,
}

/// The latest received version (waiting, or refused for a person) of the count line naming the
/// movement `movement_sync_id` under `key`.
async fn waiting_line_naming(
    conn: &impl ConnectionTrait,
    key: &str,
    movement_sync_id: &str,
) -> AppResult<Option<NamingLine>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT entity_sync_id, payload_json, server_batch_id, apply_status FROM sync_inbox
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
    let payload: String = row.try_get("", "payload_json")?;
    match serde_json::from_str::<serde_json::Value>(&payload)? {
        serde_json::Value::Object(map) => Ok(Some(NamingLine {
            sync_id: row.try_get("", "entity_sync_id")?,
            payload: map,
            server_batch_id: row.try_get("", "server_batch_id")?,
            apply_status: row.try_get("", "apply_status")?,
        })),
        _ => Ok(None),
    }
}

/// A received version of the count line naming the movement under `key` that a person's decision
/// on its conflict set aside ("keep mine", "dismiss": not applied, no longer waiting), with the
/// code of that line's latest conflict: `(line sync id, conflict code)`.
async fn set_aside_line_naming(
    conn: &impl ConnectionTrait,
    key: &str,
    movement_sync_id: &str,
) -> AppResult<Option<(String, Option<String>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT i.entity_sync_id,
                        (SELECT c.conflict_type FROM sync_conflicts c
                          WHERE c.entity_type = i.entity_type AND c.entity_sync_id = i.entity_sync_id
                          ORDER BY c.id DESC LIMIT 1) AS code
                 FROM sync_inbox i
                 WHERE i.entity_type = ? AND i.apply_status IN ('superseded', 'rejected')
                   AND i.operation NOT IN ('delete', 'archive')
                   AND json_extract(i.payload_json, '$.{key}') = ?
                 ORDER BY i.row_version DESC, i.id DESC LIMIT 1"
            ),
            [LINES.into(), movement_sync_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some((row.try_get("", "entity_sync_id")?, row.try_get("", "code")?)))
}

/// Whether count line `line_sync_id` has an open `COUNT_APPROVAL_RECORD_INVALID` conflict.
async fn invalid_conflict_open(conn: &impl ConnectionTrait, line_sync_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS o FROM sync_conflicts WHERE conflict_type = ? AND entity_type = ? AND entity_sync_id = ?
               AND status NOT IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed') LIMIT 1",
            [SYNC_APPROVAL_RECORD_INVALID.into(), LINES.into(), line_sync_id.into()],
        ))
        .await?
        .is_some())
}

/// "Take theirs" on a received count line (`sync::conflicts::take_remote_version`), before that
/// version is written: the count movement it names as posted, held with it in the inbox
/// ([`inbound_count_movement_hold`]), is applied first — the line refers to it — through every
/// check, the line version taken counted as waiting (no longer refused): the movement passes only
/// if it is that line's own and the line passes four-eyes here; its reversal comes with it
/// ([`count_movement_group`]). Otherwise nothing is taken (the caller rolls back): the stock
/// never moves by a movement that is not the line's.
pub(crate) async fn apply_held_movement_of_taken_line(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    linked_inbox_id: Option<i64>,
    payload_json: &str,
) -> AppResult<()> {
    use crate::sync::materialize::ApplyOutcome;
    if entity_type != LINES {
        return Ok(());
    }
    let Ok(serde_json::Value::Object(line)) = serde_json::from_str::<serde_json::Value>(payload_json) else {
        return Ok(());
    };
    let Some(posted) = field(&line, "posted_transaction_sync_id") else {
        return Ok(());
    };
    if crate::permit::sync_check::local_id(conn, "inventory_transactions", posted)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let Some((movement_inbox_id, _)) = waiting_movement(conn, posted).await? else {
        return Ok(());
    };
    if let Some(inbox_id) = linked_inbox_id {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_inbox SET apply_status = 'pending' WHERE id = ? AND apply_status = 'conflict'",
            [inbox_id.into()],
        ))
        .await?;
    }
    let Some(item) = crate::sync::materialize::load_inbox_row(conn, movement_inbox_id).await? else {
        return Ok(());
    };
    match Box::pin(crate::sync::materialize::apply_inbox_row_in(conn, &item)).await? {
        ApplyOutcome::Applied | ApplyOutcome::Unchanged => Ok(()),
        ApplyOutcome::Deferred { message, .. } | ApplyOutcome::Conflict { message, .. } => {
            Err(AppError::ValidationFailed(vec![format!(
                "GATE_SYNC_SERVER_VERSION_NOT_APPLIED: The server version cannot be applied yet: the count movement \
                 {posted} it names is held: {message}"
            )]))
        }
    }
}

/// Why a received reversal of a count whose adjustment is applied here, which no version of its
/// line names here, is not the reversal of one of that count's adjustments held here (`None`: it
/// is): its quantity undoes one of them (within [`QUANTITY_AGREEMENT`]) and that count has fewer
/// reversals here than adjustments (a count is reversed once per posting: a second reversal of one
/// posting would move the stock again). Only for the count's own reversal (its seeded id): see
/// [`inbound_count_movement_hold`].
async fn reversal_of_adjustment_here(
    conn: &impl ConnectionTrait,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<String>> {
    let Some(undone) = received.get("quantity").and_then(serde_json::Value::as_f64) else {
        return Ok(Some("its quantity is not given".to_string()));
    };
    let Some(CountMovementsHere {
        adjusted,
        reversed,
        undone: matching,
    }) = count_movements_here(conn, received, undone).await?
    else {
        return Ok(Some("its count is not held here".to_string()));
    };
    Ok(if matching == 0 {
        Some(format!(
            "its quantity ({undone}) undoes no adjustment of this count held here"
        ))
    } else if reversed >= adjusted {
        Some(ALREADY_REVERSED_HERE.to_string())
    } else {
        None
    })
}

/// Why a received count movement is refused: its count has as many reversals here as adjustments.
const ALREADY_REVERSED_HERE: &str = "this count is already reversed here";

/// The stock movements held here of a received count movement's count (its session, article and
/// location, any sync id).
struct CountMovementsHere {
    /// Its `COUNT_ADJUST` movements.
    adjusted: i64,
    /// Its `COUNT_REVERSAL` movements.
    reversed: i64,
    /// Its `COUNT_ADJUST` movements that a quantity `undone` undoes (within [`QUANTITY_AGREEMENT`]).
    undone: i64,
}

/// The [`CountMovementsHere`] of `received`'s count (`None`: its session, article or location is
/// not here).
async fn count_movements_here(
    conn: &impl ConnectionTrait,
    received: &serde_json::Map<String, serde_json::Value>,
    undone: f64,
) -> AppResult<Option<CountMovementsHere>> {
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
        return Ok(None);
    };
    let of_count = "FROM inventory_transactions WHERE source_type = 'COUNT_SESSION' AND source_id = ?
                      AND article_id = ? AND location_id = ?";
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT (SELECT COUNT(*) {of_count} AND movement_type = 'COUNT_ADJUST') AS adjusted,
                        (SELECT COUNT(*) {of_count} AND movement_type = 'COUNT_REVERSAL') AS reversed,
                        (SELECT COUNT(*) {of_count} AND movement_type = 'COUNT_ADJUST'
                            AND ABS(quantity + ?) <= ?) AS undone"
            ),
            [
                session.into(),
                article.into(),
                location.into(),
                session.into(),
                article.into(),
                location.into(),
                session.into(),
                article.into(),
                location.into(),
                undone.into(),
                QUANTITY_AGREEMENT.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("count movements: no row".to_string()))?;
    Ok(Some(CountMovementsHere {
        adjusted: row.try_get("", "adjusted")?,
        reversed: row.try_get("", "reversed")?,
        undone: row.try_get("", "undone")?,
    }))
}

/// Why a received reversal, released by the line version naming it and undoing that line's
/// adjustment, would still reverse its count a second time here (`None`: it would not; review of
/// ab58cd53, B2). A count is reversed once per posting: it is refused when its count already has
/// as many reversals here as adjustments, or when the version of its line held here names another
/// reversal. Otherwise a reversal under a new id, named by a forged version of the line, would move
/// the stock again once the real one is applied.
async fn second_reversal_here(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
    line_here: Option<&serde_json::Map<String, serde_json::Value>>,
) -> AppResult<Option<String>> {
    let undone = received
        .get("quantity")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default();
    if let Some(here) = count_movements_here(conn, received, undone).await? {
        if here.reversed >= here.adjusted {
            return Ok(Some(ALREADY_REVERSED_HERE.to_string()));
        }
    }
    Ok(line_here
        .and_then(|line| field(line, "reversed_transaction_sync_id"))
        .filter(|other| *other != entity_sync_id)
        .map(|other| format!("its count line here names another reversal ({other})")))
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

/// The latest version of a movement waiting in the inbox: `(inbox id, payload)`.
async fn waiting_movement(
    conn: &impl ConnectionTrait,
    movement_sync_id: &str,
) -> AppResult<Option<(i64, serde_json::Map<String, serde_json::Value>)>> {
    waiting_version(conn, "inventory_transactions", movement_sync_id).await
}

/// The latest version of a row of `entity_type` waiting in the inbox: `(inbox id, payload)`.
async fn waiting_version(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    sync_id: &str,
) -> AppResult<Option<(i64, serde_json::Map<String, serde_json::Value>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, payload_json FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ?
               AND apply_status IN ('pending', 'deferred') AND operation NOT IN ('delete', 'archive')
             ORDER BY row_version DESC, id DESC LIMIT 1",
            [entity_type.into(), sync_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: i64 = row.try_get("", "id")?;
    let payload: String = row.try_get("", "payload_json")?;
    match serde_json::from_str::<serde_json::Value>(&payload)? {
        serde_json::Value::Object(map) => Ok(Some((id, map))),
        _ => Ok(None),
    }
}

/// The movement here, or its latest version waiting in the inbox (`None`: not arrived yet).
async fn movement_here_or_waiting(
    conn: &impl ConnectionTrait,
    movement_sync_id: &str,
) -> AppResult<Option<serde_json::Map<String, serde_json::Value>>> {
    Ok(
        match crate::permit::sync_check::local_wire(conn, "inventory_transactions", movement_sync_id).await? {
            Some(here) => Some(here),
            None => waiting_movement(conn, movement_sync_id)
                .await?
                .map(|(_, waiting)| waiting),
        },
    )
}

/// The movement a received count line names as its reversal (`reversed_transaction_sync_id`) must
/// be the reversal of that count: a `COUNT_REVERSAL` of a count session, of the line's session,
/// article and location, undoing exactly the adjustment the line names as posted (its quantity is
/// the opposite of the adjustment's) — here, or waiting in the inbox to be applied with it.
/// Otherwise the record contradicts itself (a line naming any other movement, or a token reversal,
/// would release its held adjustment without a real reversal); a reversal named without a posted
/// adjustment too (a reversal only ever undoes one). Not arrived yet, the line waits for it.
async fn reversal_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    line: &serde_json::Map<String, serde_json::Value>,
    reversal: &str,
) -> AppResult<Option<InboundVerdict>> {
    let Some(posted) = field(line, "posted_transaction_sync_id") else {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!("count line {entity_sync_id} names movement {reversal} as its reversal but no posted adjustment"),
        );
    };
    let Some(movement) = movement_here_or_waiting(conn, reversal).await? else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "count line {entity_sync_id}: its reversal {reversal} has not arrived yet"
        ))));
    };
    if !reverses_count(&movement, line) {
        return conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!(
                "count line {entity_sync_id} names movement {reversal} as its reversal, but it is not the reversal \
                 of this count (session, article and location)"
            ),
        );
    }
    let Some(adjustment) = movement_here_or_waiting(conn, posted).await? else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "count line {entity_sync_id}: the adjustment {posted} its reversal undoes has not arrived yet"
        ))));
    };
    match undo_mismatch(&movement, &adjustment) {
        None => Ok(None),
        Some(why) => conflict(
            SYNC_APPROVAL_RECORD_INVALID,
            format!(
                "count line {entity_sync_id}: its reversal {reversal} does not undo its posted adjustment \
                 {posted}: {why}"
            ),
        ),
    }
}

/// Whether `movement` is a reversal of `line`'s count: a `COUNT_REVERSAL` of a count session, of
/// the line's session, article and location.
fn reverses_count(
    movement: &serde_json::Map<String, serde_json::Value>,
    line: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let same = |movement_key: &str, line_key: &str| {
        field(line, line_key).is_some() && field(movement, movement_key) == field(line, line_key)
    };
    field(movement, "movement_type") == Some("COUNT_REVERSAL")
        && field(movement, "source_type") == Some("COUNT_SESSION")
        && same("source_sync_id", "session_sync_id")
        && same("article_sync_id", "article_sync_id")
        && same("location_sync_id", "location_sync_id")
}

/// Why a reversal does not undo `adjustment` (`None`: its quantity is the opposite of the
/// adjustment's, within [`QUANTITY_AGREEMENT`]).
fn undo_mismatch(
    reversal: &serde_json::Map<String, serde_json::Value>,
    adjustment: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let quantity =
        |m: &serde_json::Map<String, serde_json::Value>| m.get("quantity").and_then(serde_json::Value::as_f64);
    match (quantity(reversal), quantity(adjustment)) {
        (Some(undone), Some(adjusted)) if (undone + adjusted).abs() <= QUANTITY_AGREEMENT => None,
        (undone, adjusted) => Some(format!(
            "its quantity ({}) is not the opposite of the adjustment's ({})",
            undone.map_or_else(|| "?".to_string(), |q| q.to_string()),
            adjusted.map_or_else(|| "?".to_string(), |q| q.to_string()),
        )),
    }
}

/// A received count adjustment whose line arrives reversed is applied together with its reversal,
/// in one transaction (as a permit and its parts, `permit::sync_check::apply_group`): the reversal
/// waiting in the inbox is applied right after it. If the reversal cannot be applied, nothing of the
/// two is kept: when it waits for another row, the adjustment waits too (both are tried again);
/// when it is refused, the adjustment becomes a `SYNC_GROUP_REFUSED` conflict for a person ("take
/// theirs" runs the group again with every check) and the reversal stays waiting. The stock never
/// moves by the adjustment alone.
pub(crate) async fn count_movement_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::permit::sync_check::Group>> {
    if field(received, "movement_type") != Some("COUNT_ADJUST")
        || field(received, "source_type") != Some("COUNT_SESSION")
    {
        return Ok(None);
    }
    let Some(NamingLine { payload: line, .. }) =
        waiting_line_naming(conn, "posted_transaction_sync_id", entity_sync_id).await?
    else {
        return Ok(None);
    };
    let Some(reversal) = field(&line, "reversed_transaction_sync_id") else {
        return Ok(None);
    };
    let Some((inbox_id, _)) = waiting_movement(conn, reversal).await? else {
        return Ok(None);
    };
    Ok(Some(crate::permit::sync_check::Group::new(
        entity_sync_id.to_string(),
        None,
        vec![crate::permit::sync_check::GroupRow::new(
            "inventory_transactions",
            inbox_id,
        )],
        true,
        None,
    )))
}

/// A received version of a count movement already held here (review of 07d895a9, B1). The same
/// count posted on two PCs makes the PC whose posting lost take the version the server received
/// first, and move its stock by the difference (`inventory::count_sync::adopt_first_posted`): only
/// for that PC's own posting, never synchronised (no version of it received or acknowledged here).
/// A movement synchronised here (received from another PC, or this PC's posting the server
/// acknowledged) was judged when it was applied (with its line, or posted here): a later version
/// changing its quantity — a forged or corrupted record, the server accepting a new version of
/// any row — would move the stock with no line or four-eyes behind it, on every PC including the
/// one where the count was posted. It is a `COUNT_APPROVAL_RECORD_INVALID` conflict for a person
/// on that movement, not applied: "take theirs" judges it again (refused), "keep mine" sends the
/// movement as held here again. Its quantity is compared with the one held here, which is its
/// line's variance on every PC but the one whose divergent posting lost (there the line held is
/// that PC's own, and a version matching it would still move the stock). A version differing in
/// another field only is taken as before (audited, the stock does not move).
async fn received_version_of_movement_here(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    const MOVEMENTS: &str = "inventory_transactions";
    let Some(held) =
        crate::inventory::count_sync::seeded_count_local(conn, MOVEMENTS, entity_sync_id, received).await?
    else {
        return Ok(None);
    };
    if crate::inventory::count_sync::adoption_delta(&held, received).is_none()
        || crate::sync::queries::synced_row_version(conn, MOVEMENTS, entity_sync_id)
            .await?
            .is_none()
    {
        return Ok(None);
    }
    let quantity = |m: &serde_json::Map<String, serde_json::Value>| {
        m.get("quantity")
            .and_then(serde_json::Value::as_f64)
            .map_or_else(|| "?".to_string(), |q| q.to_string())
    };
    let line = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.entity_sync_id AS line FROM inventory_count_lines cl
             JOIN inventory_transactions t ON t.id IN (cl.posted_transaction_id, cl.reversed_transaction_id)
             WHERE t.entity_sync_id = ? LIMIT 1",
            [entity_sync_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "line"))
        .transpose()?
        .flatten();
    let of_line = line.map_or_else(String::new, |line| format!(" (count line {line})"));
    conflict(
        SYNC_APPROVAL_RECORD_INVALID,
        format!(
            "count movement {entity_sync_id}{of_line} is applied here with the quantity {}; a later version \
             received with the quantity {} would move the stock by the difference: it is not applied and the stock \
             does not move",
            quantity(&held),
            quantity(received),
        ),
    )
}

/// A received stock movement of a count (`COUNT_ADJUST`, `COUNT_REVERSAL` of a count session) is
/// held with its count line (CMMS Advisor Q8, see the module documentation). It is applied only
/// with a received version of its line that NAMES it (posted / reversed movement) and passes the
/// four-eyes check here; otherwise it waits — even when an earlier version of that line is here
/// (entered, approved: it says nothing of this movement, and the version naming it may still be
/// refused). A line here cannot name a movement not applied yet (it refers to it), so the version
/// naming it always comes from the inbox, with it or after it. A reversal waits for the adjustment
/// it undoes; an adjustment whose line arrives reversed is applied with its reversal, in one
/// transaction ([`count_movement_group`], net zero). A movement already here (a replay, the same
/// posting received twice) is not held, unless the version received would move the stock again
/// ([`received_version_of_movement_here`]).
pub(crate) async fn inbound_count_movement_hold(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<InboundVerdict>> {
    let Some(kind) = field(received, "movement_type").filter(|k| COUNT_MOVEMENTS.contains(k)) else {
        return Ok(None);
    };
    if field(received, "source_type") != Some("COUNT_SESSION") {
        return Ok(None);
    }
    if crate::permit::sync_check::local_id(conn, "inventory_transactions", entity_sync_id)
        .await?
        .is_some()
    {
        return received_version_of_movement_here(conn, entity_sync_id, received).await;
    }
    let adjust = kind == "COUNT_ADJUST";
    // The count's adjustment is already applied here (under this id or another: the same count
    // posted on two PCs, `inventory::count_sync`): its line was judged when it arrived; another
    // adjustment of it goes on as before (applied, listed "posted twice"), with no line check: a
    // known limit stated in the module documentation.
    let count_here = count_adjusted_here(conn, received).await?;
    if count_here && adjust {
        return Ok(None);
    }
    let key = if adjust {
        "posted_transaction_sync_id"
    } else {
        "reversed_transaction_sync_id"
    };
    let naming = waiting_line_naming(conn, key, entity_sync_id).await?;
    // A reversal of it is judged with the version of its line naming it, as any (review of
    // 07d895a9, S1). Only the count's own reversal (its seeded id: the same on every PC) that no
    // version of its line names here — waiting, refused or set aside — is matched against the
    // ledger as it is held: that version may never come here (the line of the PC whose divergent
    // posting lost stays in conflict there). It must undo an adjustment of that count held here,
    // not reversed yet; otherwise it is a conflict for a person on that reversal, not a silent wait
    // (review of 07d895a9, S2). A reversal under another id waits for the line version naming it.
    if count_here
        && naming.is_none()
        && crate::inventory::count_sync::is_seeded_count_movement(entity_sync_id, received)
        && set_aside_line_naming(conn, key, entity_sync_id).await?.is_none()
    {
        return match reversal_of_adjustment_here(conn, received).await? {
            None => Ok(None),
            Some(why) => conflict(
                SYNC_APPROVAL_RECORD_INVALID,
                format!(
                    "count reversal {entity_sync_id}, which no count line here names, is not the reversal of a \
                     posting of its count held here: {why}; it is not applied and the stock does not move"
                ),
            ),
        };
    }
    let Some(NamingLine {
        sync_id: line_sync_id,
        payload: line,
        server_batch_id,
        apply_status,
    }) = naming
    else {
        // The version of its line naming it was set aside by a person's decision on that line's
        // conflict ("keep mine", "dismiss"): it waits for that line to be settled, not to arrive.
        if let Some((line_sync_id, code)) = set_aside_line_naming(conn, key, entity_sync_id).await? {
            return Ok(Some(InboundVerdict::Defer(format!(
                "count movement {entity_sync_id} waits for the conflict of its count line {line_sync_id} ({}) to be \
                 settled: the version of that line naming it was set aside here, not applied (held with it, \
                 four-eyes)",
                code.as_deref().unwrap_or("?")
            ))));
        }
        // No received version of its line names it yet (a page of the exchange can end between the
        // movement and the line version naming it): it waits for that version, judged with it.
        return Ok(Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id}: the version of its count line naming it has not arrived yet (held \
             with it, four-eyes)"
        ))));
    };
    // That version refused as a record contradicting itself (review of 2faa97d5, S1): the movement
    // waits for that conflict to be settled — a corrected version of the line from the PC where the
    // count is applied, judged here, or "take theirs" ([`apply_held_movement_of_taken_line`]) — never
    // released while the line stays refused, its conflict saying the stock did not move.
    if apply_status == "conflict" && invalid_conflict_open(conn, &line_sync_id).await? {
        return Ok(Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id} waits for the conflict of its count line {line_sync_id} \
             ({SYNC_APPROVAL_RECORD_INVALID}) to be settled (held with it)"
        ))));
    }
    let line_here = crate::permit::sync_check::local_wire(conn, LINES, &line_sync_id).await?;
    if !adjust {
        // A reversal undoes the adjustment named by its line: applied once that one is here (with
        // it, in its group), and only if it is the reversal of that line's count undoing that
        // adjustment (review of 2faa97d5, N1: an adjustment applied here does not release any
        // reversal of its count). A line naming no adjustment for it to undo, or another reversal,
        // is refused when it is judged (`reversal_check`): the reversal is never applied alone.
        let adjustment = match field(&line, "posted_transaction_sync_id") {
            Some(posted) => crate::permit::sync_check::local_wire(conn, "inventory_transactions", posted).await?,
            None => None,
        };
        let Some(adjustment) = adjustment else {
            return Ok(Some(InboundVerdict::Defer(format!(
                "count reversal {entity_sync_id}: the adjustment it undoes is not applied here (held with count line \
                 {line_sync_id})"
            ))));
        };
        let line = crate::permit::sync_check::overlay(line_here.as_ref(), &line);
        let why = if reverses_count(received, &line) {
            undo_mismatch(received, &adjustment)
        } else {
            Some("it is not the reversal of this count (session, article and location)".to_string())
        };
        if let Some(why) = why {
            return Ok(Some(InboundVerdict::Defer(format!(
                "count reversal {entity_sync_id} is held with its count line {line_sync_id} \
                 ({SYNC_APPROVAL_RECORD_INVALID}: {why})"
            ))));
        }
        // The count's adjustment applied here: its line version is not enough, the count must not
        // be reversed here already (review of ab58cd53, B2) — a visible conflict on that reversal.
        if count_here {
            if let Some(why) = second_reversal_here(conn, entity_sync_id, received, line_here.as_ref()).await? {
                return conflict(
                    SYNC_APPROVAL_RECORD_INVALID,
                    format!(
                        "count reversal {entity_sync_id}, named by a version of count line {line_sync_id}, would \
                         reverse its count a second time: {why}; it is not applied and the stock does not move"
                    ),
                );
            }
        }
        return Ok(None);
    }
    // The adjustment its line releases must be that line's own (review of 5ead90e8, S2), as a
    // reversal must be the reversal of its count (`reversal_check`): otherwise a line judged here
    // would release a movement of another count, article, location or quantity. The line naming it
    // is then itself a visible conflict ([`forged_adjustment_named`]), recorded when that line is
    // judged — on the same pass (movements go before the lines that name them) or when it arrives
    // or is tried again; a line version naming it cannot be applied here before it (it refers to
    // it), so this hold never has to record it.
    if let Some(why) = adjustment_mismatch(received, &crate::permit::sync_check::overlay(line_here.as_ref(), &line)) {
        return Ok(Some(InboundVerdict::Defer(format!(
            "count movement {entity_sync_id} is held with its count line {line_sync_id} \
             ({SYNC_APPROVAL_RECORD_INVALID}: {why})"
        ))));
    }
    // Its line judged here, in the line's own context (a version from a snapshot or an exchange):
    // a line arriving reversed passes once its reversal is shown to be this count's
    // (`reversal_check`), and the adjustment is then applied with it (`count_movement_group`). The
    // adjustment this line names is the one just checked against it.
    let judged = crate::sync::materialize::judged_as_row_of_batch(
        &server_batch_id,
        judge_count_line(conn, &line_sync_id, &line),
    )
    .await?;
    Ok(match judged {
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

/// Why a received count adjustment is not the one of the count line naming it as posted (`None`: it
/// is): the adjustment of a count is a `COUNT_ADJUST` of that line's count session, article and
/// location, of the line's variance (within [`QUANTITY_AGREEMENT`]).
fn adjustment_mismatch(
    movement: &serde_json::Map<String, serde_json::Value>,
    line: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let same = |movement_key: &str, line_key: &str| {
        field(line, line_key).is_some() && field(movement, movement_key) == field(line, line_key)
    };
    if !(same("source_sync_id", "session_sync_id")
        && same("article_sync_id", "article_sync_id")
        && same("location_sync_id", "location_sync_id"))
    {
        return Some("it is not the adjustment of this count (session, article and location)".to_string());
    }
    let number =
        |p: &serde_json::Map<String, serde_json::Value>, key: &str| p.get(key).and_then(serde_json::Value::as_f64);
    match (number(movement, "quantity"), number(line, "variance_qty")) {
        (Some(moved), Some(variance)) if (moved - variance).abs() <= QUANTITY_AGREEMENT => None,
        (moved, variance) => Some(format!(
            "its quantity ({}) is not the count's variance ({})",
            moved.map_or_else(|| "?".to_string(), |q| q.to_string()),
            variance.map_or_else(|| "?".to_string(), |q| q.to_string()),
        )),
    }
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

/// What the sync centre shows of a count held back here (CMMS Advisor Q12): its reference, who
/// counted (and approved) it and when it was posted, from the session and person held here (or the
/// session's version waiting in the inbox); `None` where it is not known here yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CountHoldContext {
    pub session_code: Option<String>,
    pub counter: Option<String>,
    pub posted_at: Option<String>,
}

/// The [`CountHoldContext`] of a `COUNT_SELF_APPROVAL` conflict, from its received line.
pub(crate) async fn count_hold_context(
    conn: &impl ConnectionTrait,
    inbound_payload_json: Option<&str>,
) -> AppResult<Option<CountHoldContext>> {
    let Some(Ok(serde_json::Value::Object(line))) = inbound_payload_json.map(serde_json::from_str::<serde_json::Value>)
    else {
        return Ok(None);
    };
    let session = match field(&line, "session_sync_id") {
        Some(sync_id) => {
            match crate::permit::sync_check::local_wire(conn, "inventory_count_sessions", sync_id).await? {
                Some(here) => Some(here),
                None => waiting_version(conn, "inventory_count_sessions", sync_id)
                    .await?
                    .map(|(_, waiting)| waiting),
            }
        }
        None => None,
    };
    let of_session = |key: &str| session.as_ref().and_then(|s| field(s, key)).map(str::to_string);
    let counter = match field(&line, "counted_by_sync_id") {
        Some(sync_id) => conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(NULLIF(TRIM(display_name), ''), username) AS v FROM user_accounts
                 WHERE entity_sync_id = ?",
                [sync_id.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<Option<String>>("", "v").ok().flatten()),
        None => None,
    };
    Ok(Some(CountHoldContext {
        session_code: of_session("session_code"),
        counter,
        posted_at: of_session("posted_at"),
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
/// for a second approval tells the count-approval holders, once per count — its session's sync id
/// names the notice on every PC, so several lines of one count held back, or the same line refused
/// again (judged again on a later pass, "retry later"), never raise another one.
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
    let count = field(&line, "session_sync_id").unwrap_or(entity_sync_id);
    device_notice(
        conn,
        SECOND_APPROVAL_CATEGORY,
        SECOND_APPROVAL_CATEGORY,
        &format!("count-held|{count}"),
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
/// reversed count tells its counter to count again. Its `COUNT_APPROVAL_RECORD_INVALID` conflicts
/// of older versions close too: a later version passed every check here (with its movement, held
/// while the older one was refused).
pub(crate) async fn after_count_line_applied(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    applied_row_version: i64,
) -> AppResult<()> {
    if entity_type != LINES {
        return Ok(());
    }
    crate::sync::code_clash::close_conflicts_superseded_by(
        conn,
        SYNC_APPROVAL_RECORD_INVALID,
        LINES,
        entity_sync_id,
        applied_row_version,
        "A later version of the count line passed its checks here and was applied.",
    )
    .await?;
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

    /// CMMS Advisor Q10: the cost a line was entered with judges it again only when its stored value
    /// is the variance × that cost and the cost is within 10 % of the cost here.
    #[test]
    fn the_entry_cost_judges_only_when_consistent_and_close_to_the_cost_here() {
        let line = |cost: Option<f64>, value: Option<f64>| {
            let mut p = serde_json::Map::new();
            p.insert("entry_unit_cost".into(), serde_json::json!(cost));
            p.insert("entry_variance_value".into(), serde_json::json!(value));
            p
        };
        let judged = super::judged_unit_cost;
        assert_eq!(judged(&line(Some(10.0), Some(-50.0)), -5.0, Some(10.5)), Some(10.0));
        assert_eq!(
            judged(&line(Some(10.0), Some(-50.0)), -5.0, Some(11.0)),
            Some(10.0),
            "1 of 11: within 10 % of the cost here"
        );
        assert_eq!(
            judged(&line(Some(10.0), Some(-50.0)), -5.0, Some(11.2)),
            Some(11.2),
            "beyond 10 %"
        );
        assert_eq!(
            judged(&line(Some(10.0), Some(-40.0)), -5.0, Some(10.4)),
            Some(10.4),
            "value ≠ 5 × 10"
        );
        assert_eq!(
            judged(&line(Some(9.0), Some(-50.0)), -5.0, Some(10.0)),
            Some(10.0),
            "inconsistent value"
        );
        assert_eq!(
            judged(&line(None, None), -5.0, Some(10.0)),
            Some(10.0),
            "nothing stored"
        );
        assert_eq!(
            judged(&line(Some(10.0), Some(-50.0)), -5.0, None),
            None,
            "no cost here: percent only"
        );
    }

    #[test]
    fn a_system_quantity_of_zero_or_less_is_always_above() {
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(1.0, 0.0, Some(0.01)));
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(6.0, -5.0, Some(0.01)));
    }
}
