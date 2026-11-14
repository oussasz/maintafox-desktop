//! Every permit type a work order requires (CMMS Advisor 2026-11-04, ux-fix L4-10b): the plant's
//! rules can match a work order with several permit types (for example LOTO for a criticality-A
//! machine and hot work for welding on it). The work order keeps the whole SET of the types the
//! rules gave it, in `work_orders.permit_required_types_json` (migration 279); its start and its
//! resume need a live permit of every type of the set that is not cleared (`permit::wo_gate`).
//!
//! The column holds a JSON array, sorted by permit type, one element per type:
//!
//! ```json
//! [{"permit_type_code":"loto","rule_code":"CRITICITE_A","cleared_by_sync_id":null,"cleared_at":null,"clear_reason":null}]
//! ```
//!
//! - `rule_code`: the first rule of the list (its order) that gave the type;
//! - `cleared_*`: a type lifted on this work order, one type at a time, with the permit-issue right
//!   at its site and a reason (`wo::workflow::actions::permit_requirement::clear_permit_rule_flag`).
//!   The person is named by their sync id, the same on every PC.
//!
//! When it was added is in the work order's history (`permit_rule_applied`), not here: the column
//! travels whole between PCs and must read the same on each.
//!
//! The set only grows: a type a rule gave is never dropped automatically (on creation, a re-check, a
//! received row, a resume, the catch-up). The legacy columns `permit_rule_code` /
//! `permit_rule_permit_type` keep the PRIMARY rule (an energy isolation first, else the list's
//! order) exactly as before, for display and for PCs not on this version; the set always holds the
//! primary type. A NULL column (a row written before migration 279, or by a PC without it) reads as
//! the primary rule alone, from the legacy columns. The legacy clearing of the primary rule and the
//! clearing of its element must both be there for the primary type to count as cleared (a clearing
//! withdrawn in the legacy columns withdraws it here too: fail-closed).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppResult};

/// The column of the set.
pub const COLUMN: &str = "permit_required_types_json";

/// Stable code: the stored set does not read (a corrupt or forged row): refused, never guessed.
pub const CODE_SET_UNREADABLE: &str = "WO_PERMIT_REQUIRED_TYPES_UNREADABLE";

/// Who lifted one permit type on the work order, when and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearing {
    pub by_sync_id: Option<String>,
    pub at: String,
    pub reason: Option<String>,
}

/// One permit type the work order requires, the rule that gave it, and its clearing, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredPermit {
    pub permit_type_code: String,
    pub rule_code: String,
    pub clearing: Option<Clearing>,
}

/// One element as stored and sent (every key always written, `null` when empty).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Element {
    permit_type_code: String,
    rule_code: String,
    #[serde(default)]
    cleared_by_sync_id: Option<String>,
    #[serde(default)]
    cleared_at: Option<String>,
    #[serde(default)]
    clear_reason: Option<String>,
}

fn non_blank(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The primary rule as the legacy columns hold it (the clearing's person by sync id).
#[derive(Debug, Clone, Copy, Default)]
pub struct Legacy<'a> {
    pub rule_code: Option<&'a str>,
    pub permit_type: Option<&'a str>,
    pub cleared_by_sync_id: Option<&'a str>,
    pub cleared_at: Option<&'a str>,
    pub clear_reason: Option<&'a str>,
}

/// The permit types a work order requires, sorted by type, one element per type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequiredSet {
    items: Vec<RequiredPermit>,
    /// The set was read from the column (not only from the legacy columns).
    stored: bool,
    /// The primary type (`permit_rule_permit_type`), when the work order has one.
    primary: Option<String>,
}

/// Reads a stored or received set: an array of objects, each naming a permit type and a rule, no
/// type twice. Anything else does not read (`Err`, why).
pub fn parse(json: &str) -> Result<Vec<RequiredPermit>, String> {
    let elements: Vec<Element> =
        serde_json::from_str(json).map_err(|e| format!("the required permit types do not read: {e}"))?;
    let mut items: Vec<RequiredPermit> = Vec::with_capacity(elements.len());
    for e in elements {
        let Some(permit_type_code) = non_blank(Some(&e.permit_type_code)) else {
            return Err("a required permit type names no permit type".into());
        };
        let Some(rule_code) = non_blank(Some(&e.rule_code)) else {
            return Err(format!("required permit type {permit_type_code} names no rule"));
        };
        if items.iter().any(|i| i.permit_type_code == permit_type_code) {
            return Err(format!("permit type {permit_type_code} is listed twice"));
        }
        let clearing = non_blank(e.cleared_at.as_deref()).map(|at| Clearing {
            by_sync_id: non_blank(e.cleared_by_sync_id.as_deref()),
            at,
            reason: non_blank(e.clear_reason.as_deref()),
        });
        items.push(RequiredPermit {
            permit_type_code,
            rule_code,
            clearing,
        });
    }
    items.sort_by(|a, b| a.permit_type_code.cmp(&b.permit_type_code));
    Ok(items)
}

impl RequiredSet {
    /// The set of a work order from its column (`json`, NULL or blank: none stored) and its legacy
    /// columns: the primary type is always in it; when the legacy columns hold no clearing of the
    /// primary rule, its element counts as not cleared.
    pub fn from_row(json: Option<&str>, legacy: Legacy<'_>) -> Result<Self, String> {
        let stored_json = json.map(str::trim).filter(|j| !j.is_empty());
        let mut set = RequiredSet {
            items: match stored_json {
                Some(j) => parse(j)?,
                None => Vec::new(),
            },
            stored: stored_json.is_some(),
            primary: None,
        };
        let primary = non_blank(legacy.permit_type).zip(non_blank(legacy.rule_code));
        if let Some((permit_type, rule)) = primary {
            let legacy_clearing = non_blank(legacy.cleared_at).map(|at| Clearing {
                by_sync_id: non_blank(legacy.cleared_by_sync_id),
                at,
                reason: non_blank(legacy.clear_reason),
            });
            match set.items.iter_mut().find(|i| i.permit_type_code == permit_type) {
                Some(item) => {
                    if legacy_clearing.is_none() {
                        item.clearing = None;
                    }
                }
                None => set.items.push(RequiredPermit {
                    permit_type_code: permit_type.clone(),
                    rule_code: rule,
                    clearing: legacy_clearing,
                }),
            }
            set.primary = Some(permit_type);
            set.sort();
        }
        Ok(set)
    }

    /// A set made of rule matches (`(permit type, rule)`, the first rule per type), none cleared.
    pub fn from_matches(matches: &[(String, String)]) -> Self {
        let mut set = RequiredSet::default();
        for (permit_type, rule) in matches {
            set.add(permit_type, rule);
        }
        set
    }

    fn sort(&mut self) {
        self.items.sort_by(|a, b| a.permit_type_code.cmp(&b.permit_type_code));
    }

    pub fn items(&self) -> &[RequiredPermit] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The set was read from its column (a NULL column reads from the legacy columns only).
    pub fn stored(&self) -> bool {
        self.stored
    }

    /// The primary type (the legacy `permit_rule_permit_type`).
    pub fn primary(&self) -> Option<&str> {
        self.primary.as_deref()
    }

    pub fn get(&self, permit_type: &str) -> Option<&RequiredPermit> {
        self.items.iter().find(|i| i.permit_type_code == permit_type)
    }

    pub fn contains(&self, permit_type: &str) -> bool {
        self.get(permit_type).is_some()
    }

    /// The types still required (not cleared), sorted.
    pub fn uncleared_types(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.clearing.is_none())
            .map(|i| i.permit_type_code.clone())
            .collect()
    }

    /// Adds `permit_type` (given by `rule`) when the set does not hold it; never touches another
    /// type, nor the rule or clearing of a type it holds. Returns whether it was added.
    pub fn add(&mut self, permit_type: &str, rule: &str) -> bool {
        if self.contains(permit_type) {
            return false;
        }
        self.items.push(RequiredPermit {
            permit_type_code: permit_type.to_string(),
            rule_code: rule.to_string(),
            clearing: None,
        });
        self.sort();
        true
    }

    /// Records the clearing of `permit_type` (in the set). Returns whether it was there.
    pub fn clear(&mut self, permit_type: &str, clearing: Clearing) -> bool {
        match self.items.iter_mut().find(|i| i.permit_type_code == permit_type) {
            Some(item) => {
                item.clearing = Some(clearing);
                true
            }
            None => false,
        }
    }

    /// Withdraws every clearing (the permit is required again). Returns the clearings withdrawn.
    pub fn withdraw_clearings(&mut self) -> Vec<RequiredPermit> {
        let withdrawn: Vec<RequiredPermit> = self.items.iter().filter(|i| i.clearing.is_some()).cloned().collect();
        for item in &mut self.items {
            item.clearing = None;
        }
        withdrawn
    }

    /// The canonical JSON of the set (sorted by type, every key written). `None` when empty: the
    /// column stays NULL, read as the legacy columns.
    pub fn to_json(&self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let elements: Vec<Element> = self
            .items
            .iter()
            .map(|i| Element {
                permit_type_code: i.permit_type_code.clone(),
                rule_code: i.rule_code.clone(),
                cleared_by_sync_id: i.clearing.as_ref().and_then(|c| c.by_sync_id.clone()),
                cleared_at: i.clearing.as_ref().map(|c| c.at.clone()),
                clear_reason: i.clearing.as_ref().and_then(|c| c.reason.clone()),
            })
            .collect();
        Some(serde_json::to_string(&elements).unwrap_or_else(|_| "[]".into()))
    }
}

/// The set two PCs end on when a row is received over the set held here (before it is written,
/// `permit::sync_check::keep_held_rule_trace`): the union of both, so a type added on either PC is
/// kept and two PCs that added different types converge on both. For a type both hold, the rule is
/// the smaller rule code (the same on every PC); the received clearing wins (it passed the check on
/// arrival); a clearing held here that the row does not carry is kept, unless the row is a re-tick
/// ([`is_received_retick`]): then every clearing held here is withdrawn, and returned (second
/// element) for the caller to record. A received row never drops a type.
pub fn merge(
    held: &RequiredSet,
    received: &RequiredSet,
    received_requires: bool,
) -> (RequiredSet, Vec<RequiredPermit>) {
    let retick = is_received_retick(held, received, received_requires);
    let mut withdrawn = Vec::new();
    let mut merged = RequiredSet {
        items: Vec::new(),
        stored: true,
        primary: received.primary.clone().or_else(|| held.primary.clone()),
    };
    for item in held.items.iter().chain(received.items.iter()) {
        if merged.contains(&item.permit_type_code) {
            continue;
        }
        let (h, r) = (held.get(&item.permit_type_code), received.get(&item.permit_type_code));
        let one = match (h, r) {
            (Some(h), Some(r)) => RequiredPermit {
                permit_type_code: h.permit_type_code.clone(),
                rule_code: h.rule_code.clone().min(r.rule_code.clone()),
                clearing: match (&h.clearing, &r.clearing) {
                    (_, Some(c)) => Some(c.clone()),
                    (Some(_), None) if retick => {
                        withdrawn.push(h.clone());
                        None
                    }
                    (held_clearing, None) => held_clearing.clone(),
                },
            },
            (Some(only), None) | (None, Some(only)) => only.clone(),
            (None, None) => continue,
        };
        merged.items.push(one);
    }
    merged.sort();
    (merged, withdrawn)
}

/// Whether a received row is an unambiguous re-tick of the permit requirement (the permit required
/// again, every clearing withdrawn), as `permit_requirement::withdraw_clearing_on_retick` makes it:
/// the row requires the permit, its set lifts no type at all, and every type held here is lifted
/// (a re-tick is only made on a work order whose permit was no longer required). A PC on this
/// version sends `requires_permit = 1` whenever one type is still required, so that flag alone says
/// nothing: a row that still lifts a type, or that arrives over a type still required here, comes
/// from a PC that has not received a lifting made here yet (security review L4-10b), and the
/// liftings held here are kept. A row naming a type not held here is no re-tick either: the sender
/// requires the permit for that type (its flag is explained by it), and it may not have received
/// the liftings made here.
pub fn is_received_retick(held: &RequiredSet, received: &RequiredSet, received_requires: bool) -> bool {
    received_requires
        && !received.is_empty()
        && received
            .items
            .iter()
            .all(|i| i.clearing.is_none() && held.contains(&i.permit_type_code))
        && held.items.iter().any(|i| i.clearing.is_some())
        && held.uncleared_types().is_empty()
}

/// The refusal of a stored set that does not read.
pub fn unreadable(wo_id: i64, why: &str) -> crate::errors::AppError {
    org_validation_failed(
        CODE_SET_UNREADABLE,
        format!("Work order {wo_id}: {why}. The permits it requires cannot be judged; have it checked."),
        crate::errors::issue_params(&[("wo_id", wo_id.to_string()), ("detail", why.to_string())]),
    )
}

/// The SELECT columns (on `wo`, joined with `ua` the legacy clearer) that [`from_query_row`] reads.
pub const SELECT_COLUMNS: &str = "wo.permit_required_types_json AS rs_json, wo.permit_rule_code AS rs_rule, \
     wo.permit_rule_permit_type AS rs_type, ua_rs.entity_sync_id AS rs_cleared_by_sync, \
     wo.permit_rule_cleared_at AS rs_cleared_at, wo.permit_rule_clear_reason AS rs_clear_reason";

/// The join [`SELECT_COLUMNS`] needs (on `wo`).
pub const SELECT_JOIN: &str = "LEFT JOIN user_accounts ua_rs ON ua_rs.id = wo.permit_rule_cleared_by_id";

/// The set from a row read with [`SELECT_COLUMNS`] / [`SELECT_JOIN`].
pub fn from_query_row(row: &sea_orm::QueryResult, wo_id: i64) -> AppResult<RequiredSet> {
    let json: Option<String> = row.try_get("", "rs_json")?;
    let rule: Option<String> = row.try_get("", "rs_rule")?;
    let permit_type: Option<String> = row.try_get("", "rs_type")?;
    let by: Option<String> = row.try_get("", "rs_cleared_by_sync")?;
    let at: Option<String> = row.try_get("", "rs_cleared_at")?;
    let reason: Option<String> = row.try_get("", "rs_clear_reason")?;
    RequiredSet::from_row(
        json.as_deref(),
        Legacy {
            rule_code: rule.as_deref(),
            permit_type: permit_type.as_deref(),
            cleared_by_sync_id: by.as_deref(),
            cleared_at: at.as_deref(),
            clear_reason: reason.as_deref(),
        },
    )
    .map_err(|why| unreadable(wo_id, &why))
}

/// The set of a work order here (`None`: no such work order). A stored set that does not read is
/// refused ([`CODE_SET_UNREADABLE`]).
pub async fn read(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<RequiredSet>> {
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {SELECT_COLUMNS} FROM work_orders wo {SELECT_JOIN} WHERE wo.id = ?"),
            [wo_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    from_query_row(&row, wo_id).map(Some)
}

/// Writes the set of a work order (NULL when empty), with the row's version moved. `expected`: the
/// version the caller read (`None`: unchecked). Returns whether it was written.
pub async fn write(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    set: &RequiredSet,
    expected: Option<i64>,
) -> AppResult<bool> {
    let json = set.to_json();
    let written = match expected {
        Some(version) => {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET permit_required_types_json = ?, row_version = row_version + 1 \
                  WHERE id = ? AND row_version = ?",
                [json.into(), wo_id.into(), version.into()],
            ))
            .await?
        }
        None => {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET permit_required_types_json = ?, row_version = row_version + 1 WHERE id = ?",
                [json.into(), wo_id.into()],
            ))
            .await?
        }
    };
    Ok(written.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clearing(at: &str) -> Clearing {
        Clearing {
            by_sync_id: Some("user-1".into()),
            at: at.into(),
            reason: Some("raison suffisante".into()),
        }
    }

    #[test]
    fn the_stored_form_is_canonical() {
        let mut set = RequiredSet::from_matches(&[
            ("loto".into(), "CRITICITE_A".into()),
            ("hot_work".into(), "SOUDURE".into()),
            ("loto".into(), "AUTRE".into()),
        ]);
        assert_eq!(
            set.to_json().expect("json"),
            r#"[{"permit_type_code":"hot_work","rule_code":"SOUDURE","cleared_by_sync_id":null,"cleared_at":null,"clear_reason":null},{"permit_type_code":"loto","rule_code":"CRITICITE_A","cleared_by_sync_id":null,"cleared_at":null,"clear_reason":null}]"#
        );
        assert!(set.clear("hot_work", clearing("2026-11-04T08:00:00Z")));
        let again = parse(&set.to_json().expect("json")).expect("reads");
        assert_eq!(again, set.items().to_vec());
        assert_eq!(set.uncleared_types(), vec!["loto".to_string()]);
        assert!(RequiredSet::default().to_json().is_none(), "an empty set stays NULL");
    }

    #[test]
    fn a_set_that_does_not_read_is_refused() {
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"permit_type_code":"loto"}"#).is_err(), "not an array");
        assert!(parse(r#"[{"permit_type_code":"","rule_code":"R"}]"#).is_err());
        assert!(parse(r#"[{"permit_type_code":"loto","rule_code":""}]"#).is_err());
        assert!(
            parse(r#"[{"permit_type_code":"loto","rule_code":"A"},{"permit_type_code":"loto","rule_code":"B"}]"#)
                .is_err(),
            "a type twice"
        );
        assert_eq!(parse("[]").expect("empty").len(), 0);
    }

    #[test]
    fn a_null_column_reads_as_the_primary_rule() {
        let legacy = Legacy {
            rule_code: Some("CRITICITE_A"),
            permit_type: Some("loto"),
            cleared_by_sync_id: Some("user-1"),
            cleared_at: Some("2026-11-04T08:00:00Z"),
            clear_reason: Some("raison suffisante"),
        };
        let set = RequiredSet::from_row(None, legacy).expect("reads");
        assert!(!set.stored());
        assert_eq!(set.items().len(), 1);
        assert_eq!(set.primary(), Some("loto"));
        assert_eq!(
            set.get("loto").expect("primary").clearing,
            Some(clearing("2026-11-04T08:00:00Z"))
        );
        assert!(set.uncleared_types().is_empty());
        assert!(RequiredSet::from_row(None, Legacy::default()).expect("none").is_empty());
    }

    #[test]
    fn the_primary_type_is_always_in_the_set_and_its_legacy_withdrawal_wins() {
        let mut stored = RequiredSet::from_matches(&[("hot_work".into(), "SOUDURE".into())]);
        stored.add("loto", "CRITICITE_A");
        stored.clear("loto", clearing("2026-11-04T08:00:00Z"));
        let json = stored.to_json().expect("json");
        // The legacy columns hold no clearing of the primary rule: it is required.
        let legacy = Legacy {
            rule_code: Some("CRITICITE_A"),
            permit_type: Some("loto"),
            ..Legacy::default()
        };
        let set = RequiredSet::from_row(Some(&json), legacy).expect("reads");
        assert_eq!(set.uncleared_types(), vec!["hot_work".to_string(), "loto".to_string()]);
        // A primary type missing from the stored set is added from the legacy columns.
        let only_hot = RequiredSet::from_matches(&[("hot_work".into(), "SOUDURE".into())])
            .to_json()
            .expect("json");
        let set = RequiredSet::from_row(Some(&only_hot), legacy).expect("reads");
        assert!(set.contains("loto") && set.contains("hot_work"));
        assert!(RequiredSet::from_row(Some("nope"), legacy).is_err());
    }

    #[test]
    fn two_sets_merge_on_their_union() {
        let mut held = RequiredSet::from_matches(&[("loto".into(), "CRITICITE_A".into())]);
        held.clear("loto", clearing("2026-11-04T08:00:00Z"));
        let received =
            RequiredSet::from_matches(&[("hot_work".into(), "SOUDURE".into()), ("loto".into(), "AUTRE".into())]);
        // The row does not require the permit: the clearing held here is kept.
        let (merged, withdrawn) = merge(&held, &received, false);
        assert!(withdrawn.is_empty());
        assert_eq!(merged.items().len(), 2);
        assert_eq!(
            merged.get("loto").expect("loto").rule_code,
            "AUTRE",
            "the smaller rule code"
        );
        assert!(merged.get("loto").expect("loto").clearing.is_some());
        assert_eq!(merged.uncleared_types(), vec!["hot_work".to_string()]);
        // Requiring the permit for hot work, which the row still requires, is no re-tick of loto.
        let (merged, withdrawn) = merge(&held, &received, true);
        assert!(withdrawn.is_empty());
        assert_eq!(merged.uncleared_types(), vec!["hot_work".to_string()]);
        // A received clearing wins; the same merge either way round gives the same set.
        let mut cleared = received.clone();
        cleared.clear("hot_work", clearing("2026-11-04T09:00:00Z"));
        let (a, _) = merge(&held, &cleared, false);
        assert!(a.get("hot_work").expect("hot").clearing.is_some());
        assert_eq!(
            merge(&received, &held, false).0.to_json(),
            merge(&held, &received, false).0.to_json(),
            "both PCs end on the same set"
        );
    }

    /// PC A lifted hot work while LOTO stays required; PC B, before receiving that lifting, sends an
    /// unrelated edit (requires_permit = 1, both types not lifted). A keeps its lifting, B adopts it:
    /// the states never swap (security review L4-10b).
    #[test]
    fn a_row_that_has_not_seen_a_lifting_never_withdraws_it() {
        let mut on_a = RequiredSet::from_matches(&[
            ("hot_work".into(), "SOUDURE".into()),
            ("loto".into(), "CRITICITE_A".into()),
        ]);
        on_a.clear("hot_work", clearing("2026-11-04T08:00:00Z"));
        let on_b = RequiredSet::from_matches(&[
            ("hot_work".into(), "SOUDURE".into()),
            ("loto".into(), "CRITICITE_A".into()),
        ]);
        assert!(!is_received_retick(&on_a, &on_b, true));
        let (a_after, withdrawn) = merge(&on_a, &on_b, true);
        assert!(withdrawn.is_empty(), "A keeps its lifting");
        assert_eq!(a_after.uncleared_types(), vec!["loto".to_string()]);
        let (b_after, withdrawn) = merge(&on_b, &on_a, true);
        assert!(withdrawn.is_empty());
        assert_eq!(b_after.uncleared_types(), vec!["loto".to_string()], "B adopts it");
        assert_eq!(a_after.to_json(), b_after.to_json());
        // A row that still lifts another type has not seen every lifting made here either.
        let mut all_lifted = on_a.clone();
        all_lifted.clear("loto", clearing("2026-11-04T09:00:00Z"));
        assert!(!is_received_retick(&all_lifted, &on_a, true));
    }

    /// A re-tick (every type lifted here, the row requiring the permit with no type lifted) still
    /// withdraws every clearing, returned for the caller to record; a row that does not require the
    /// permit never does.
    #[test]
    fn an_unambiguous_re_tick_withdraws_every_clearing() {
        let mut held = RequiredSet::from_matches(&[
            ("hot_work".into(), "SOUDURE".into()),
            ("loto".into(), "CRITICITE_A".into()),
        ]);
        held.clear("hot_work", clearing("2026-11-04T08:00:00Z"));
        held.clear("loto", clearing("2026-11-04T08:30:00Z"));
        let received = RequiredSet::from_matches(&[
            ("hot_work".into(), "SOUDURE".into()),
            ("loto".into(), "CRITICITE_A".into()),
        ]);
        assert!(is_received_retick(&held, &received, true));
        let (merged, withdrawn) = merge(&held, &received, true);
        assert_eq!(merged.uncleared_types().len(), 2);
        let types: Vec<&str> = withdrawn.iter().map(|i| i.permit_type_code.as_str()).collect();
        assert_eq!(types, vec!["hot_work", "loto"]);
        assert!(!is_received_retick(&held, &received, false));
        let (merged, withdrawn) = merge(&held, &received, false);
        assert!(withdrawn.is_empty() && merged.uncleared_types().is_empty());
    }
}
