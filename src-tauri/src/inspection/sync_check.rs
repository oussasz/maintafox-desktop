//! Inspection anomalies received by synchronisation, re-checked with the rules of this PC (audit
//! finding AU-72 S4).
//!
//! An anomaly is decided here only by routing it to a request or a work order
//! (`inspection::routing`: `triaged`, with that decision and that one link), by deferring it
//! (`deferred`, decision `defer`, no link), or, while it is open and nobody acted on it, cancelled by a
//! corrected reading (`inspection::results::sync_auto_anomaly`: `cancelled`, no decision, no link).
//! Routing and deferring check only that the anomaly has no link yet (`routing::assert_routable`):
//! an anomaly deferred or cancelled here can still be routed (to `triaged`) or deferred, and the
//! received row of such a change is accepted the same way ([`allowed_move`], audit finding AU-72
//! R2-S4). Nothing here closes an anomaly without a decision ("resolved" is never written), reopens
//! a decided one, changes a link once set, or clears "revue permis requise" (it requires the permit
//! on the work order raised from the anomaly, `routing::route_inspection_anomaly_to_wo`). A received
//! row that does any of these is refused as a sync conflict, for a person to decide:
//! - `INSP_ANOMALY_PERMIT_REVIEW_CLEARED`: "revue permis requise" held here, cleared;
//! - `INSP_ANOMALY_LINK_CHANGED`: the request or work order linked here, removed or replaced;
//! - `INSP_ANOMALY_DECISION_INVALID`: a status that does not match its decision and link, "resolved"
//!   or an unknown status, or a status change that is not one of the moves made here: open, deferred
//!   or cancelled to triaged or deferred, open to cancelled (a triaged anomaly never moves; a
//!   deferred or cancelled one is never reopened).

use serde_json::{Map, Value};

use crate::errors::AppResult;
use crate::permit::sync_check::{conflict, field, local_wire, overlay};
use crate::settings::site::InboundVerdict;
use sea_orm::ConnectionTrait;

type Payload = Map<String, Value>;

fn on(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

/// Whether a status, its decision and its links agree, as this PC writes them.
fn consistent(status: &str, decision: Option<&str>, di: bool, wo: bool) -> bool {
    match status {
        "open" | "cancelled" => decision.is_none() && !di && !wo,
        "triaged" => match decision {
            Some("di") => di && !wo,
            Some("wo") => wo && !di,
            _ => false,
        },
        "deferred" => decision == Some("defer") && !di && !wo,
        _ => false,
    }
}

/// Whether a status held here may become `to` by a decision made as this PC makes it: routing or
/// deferring an anomaly without a link (`routing::assert_routable`: open, deferred or cancelled), or
/// the automatic cancel of an open one (`results::sync_auto_anomaly`). The link itself is judged
/// apart (never removed or replaced).
fn allowed_move(from: &str, to: &str) -> bool {
    from == to
        || matches!(
            (from, to),
            ("open" | "deferred" | "cancelled", "triaged" | "deferred") | ("open", "cancelled")
        )
}

/// See the module documentation.
pub(crate) async fn inbound_anomaly_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "inspection_anomalies", entity_sync_id).await?;
    let a = overlay(local.as_ref(), received);
    if let Some(l) = local.as_ref() {
        if on(l, "requires_permit_review") && !on(&a, "requires_permit_review") {
            return conflict(
                "INSP_ANOMALY_PERMIT_REVIEW_CLEARED",
                format!("inspection anomaly {entity_sync_id}: its \"revue permis requise\" cannot be cleared"),
            );
        }
        for link in ["linked_di_sync_id", "linked_work_order_sync_id"] {
            if let Some(held) = field(l, link) {
                if field(&a, link) != Some(held) {
                    return conflict(
                        "INSP_ANOMALY_LINK_CHANGED",
                        format!(
                            "inspection anomaly {entity_sync_id}: the request or work order raised from it cannot \
                             be removed or replaced"
                        ),
                    );
                }
            }
        }
    }
    let status = field(&a, "resolution_status").unwrap_or_default();
    let decision = field(&a, "routing_decision");
    let (di, wo) = (
        field(&a, "linked_di_sync_id").is_some(),
        field(&a, "linked_work_order_sync_id").is_some(),
    );
    let held_status = local.as_ref().and_then(|l| field(l, "resolution_status"));
    let moved_elsewhere = held_status.is_some_and(|held| !allowed_move(held, status));
    if !consistent(status, decision, di, wo) || moved_elsewhere {
        return conflict(
            "INSP_ANOMALY_DECISION_INVALID",
            format!(
                "inspection anomaly {entity_sync_id}: status '{status}' with decision {decision:?} is not a decision \
                 made here (route to a request or a work order, defer), or changes a decided anomaly"
            ),
        );
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{allowed_move, consistent};

    #[test]
    fn a_status_moves_only_as_a_decision_here_moves_it() {
        for from in ["open", "deferred", "cancelled"] {
            assert!(allowed_move(from, "triaged"), "{from} routed");
            assert!(allowed_move(from, "deferred"), "{from} deferred");
        }
        assert!(allowed_move("open", "cancelled"), "automatic cancel");
        assert!(allowed_move("triaged", "triaged"));
        for (from, to) in [
            ("triaged", "open"),
            ("triaged", "deferred"),
            ("triaged", "cancelled"),
            ("deferred", "open"),
            ("deferred", "cancelled"),
            ("cancelled", "open"),
            ("open", "resolved"),
        ] {
            assert!(!allowed_move(from, to), "{from} -> {to}");
        }
    }

    #[test]
    fn a_status_matches_its_decision_and_link() {
        assert!(consistent("open", None, false, false));
        assert!(consistent("cancelled", None, false, false));
        assert!(consistent("triaged", Some("di"), true, false));
        assert!(consistent("triaged", Some("wo"), false, true));
        assert!(consistent("deferred", Some("defer"), false, false));
        assert!(
            !consistent("resolved", None, false, false),
            "never closed without a decision"
        );
        assert!(!consistent("triaged", None, false, false));
        assert!(
            !consistent("triaged", Some("di"), false, true),
            "the link of another decision"
        );
        assert!(!consistent("open", Some("wo"), false, true));
        assert!(!consistent("cancelled", Some("defer"), false, false));
        assert!(!consistent("unknown", None, false, false));
    }
}
