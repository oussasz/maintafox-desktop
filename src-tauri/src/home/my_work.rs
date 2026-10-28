//! "My work": what the signed-in person has to do — their work orders, requests, inspection
//! rounds, PM occurrences and notifications to acknowledge.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde::Serialize;

use super::{decode, one_row, urgency_rank_sql, ITEM_LIMIT};
use crate::analytics::as_of::instant;
use crate::errors::AppResult;
use crate::inspection::insights::{ROUND_OVERDUE_SQL, ROUND_TODO_SQL};
use crate::pm::queries::{PM_DUE_NEXT_7D_SQL, PM_OVERDUE_SQL, PM_PENDING_SQL};
use crate::wo::backlog::OPEN_STATUS_SQL;

/// Open work orders of one urgency level (`None`: no urgency set).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UrgencyCount {
    pub urgency_code: Option<String>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyWorkOrderItem {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub status_code: String,
    pub urgency_code: Option<String>,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub asset_label: Option<String>,
    pub overdue: bool,
}

/// Work orders the person is primary responsible for whose work is still ahead (draft to on hold,
/// `wo::backlog::OPEN_STATUS_SQL`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyWorkOrders {
    pub open: i64,
    /// Which "open OT" rule `open` counts: `wo::backlog::RULE_OPEN_WORK_TO_DO`.
    pub open_rule: String,
    /// Planned end passed (`wo::backlog::WO_OVERDUE_SQL`, the OT module's one overdue rule).
    pub overdue: i64,
    /// Planned window includes today (local day) and not overdue.
    pub today: i64,
    /// Most urgent first on the shared scale; no urgency last.
    pub by_urgency: Vec<UrgencyCount>,
    /// Overdue first, then most urgent, then earliest planned end.
    pub items: Vec<MyWorkOrderItem>,
    /// "Déconsignation à faire" (owner decision D4): mechanically complete work orders with a lock
    /// still fitted under their permits (`permit::wo_gate::deisolation_pending_sql`), on the list of
    /// their permit holders — the work order's responsible and whoever fitted one of those locks —
    /// until the removal of the last lock is verified. Not counted in `open` (work done).
    pub deisolation_pending: i64,
    /// The first of them, earliest completed first.
    pub deisolation_items: Vec<MyWorkOrderItem>,
}

/// The person's own requests (`di::queries::count_di_queues`, queue `mine`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MyRequests {
    /// Open requests submitted by the person.
    pub open: i64,
    /// Of those, returned to them for clarification (waiting for their answer).
    pub awaiting_my_answer: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyRoundItem {
    pub id: i64,
    pub template_code: String,
    pub template_name: String,
    pub status: String,
    pub scheduled_at: Option<String>,
    pub overdue: bool,
}

/// Inspection rounds assigned to the person (`inspection_rounds.assigned_to_id`, a personnel
/// record) and still to do.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyRounds {
    /// `false` when the login is linked to no personnel record: rounds cannot be assigned to it.
    pub personnel_linked: bool,
    /// Still to do and past their planned time (`inspection::insights::ROUND_OVERDUE_SQL`).
    pub overdue: i64,
    /// Planned later today (local day).
    pub today: i64,
    /// Planned in the next 7 days, after today.
    pub next_7_days: i64,
    /// Every round still to do (the list shows the first ones).
    pub todo: i64,
    pub items: Vec<MyRoundItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyPmItem {
    pub id: i64,
    pub plan_code: String,
    pub plan_title: String,
    pub due_at: Option<String>,
    pub work_order_code: String,
    pub overdue: bool,
}

/// Pending PM occurrences whose work order the person is primary responsible for.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyPm {
    /// Past the end of their tolerance window (`pm::queries::PM_OVERDUE_SQL`, the PM page's rule).
    pub overdue: i64,
    /// Due within the next 7 days (`pm::queries::PM_DUE_NEXT_7D_SQL`).
    pub due_next_7_days: i64,
    /// Every pending occurrence (`pm::queries::PM_PENDING_SQL`; the list shows the first ones).
    pub pending: i64,
    pub items: Vec<MyPmItem>,
}

/// The person's inbox (`notifications::inbox::counts`, the bell's own figures).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MyNotifications {
    pub unread: i64,
    pub to_acknowledge: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomeMyWork {
    pub as_of: String,
    pub work_orders: Option<MyWorkOrders>,
    pub requests: Option<MyRequests>,
    pub rounds: Option<MyRounds>,
    pub pm: Option<MyPm>,
    pub notifications: MyNotifications,
}

/// Overdue: the one OT rule (`wo::backlog::WO_OVERDUE_SQL`: work still ahead, planned end past).
const WO_OVERDUE: &str = crate::wo::backlog::WO_OVERDUE_SQL;
/// Today: the planned window includes the local day of `?`.
const WO_TODAY: &str = "(COALESCE(wo.planned_start, wo.planned_end) IS NOT NULL \
     AND date(COALESCE(wo.planned_start, wo.planned_end), 'localtime') <= date(?, 'localtime') \
     AND date(COALESCE(wo.planned_end, wo.planned_start), 'localtime') >= date(?, 'localtime'))";

pub async fn my_work_orders(db: &DatabaseConnection, user_id: i64, now: DateTime<Utc>) -> AppResult<MyWorkOrders> {
    let at = instant(now);
    let from = format!(
        "FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         LEFT JOIN reference_values ul ON ul.id = wo.urgency_id \
         LEFT JOIN equipment e ON e.id = wo.equipment_id \
         WHERE wo.primary_responsible_id = ? AND s.code IN {OPEN_STATUS_SQL}"
    );
    let row = one_row(
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS open, \
                        COALESCE(SUM(CASE WHEN {WO_OVERDUE} THEN 1 ELSE 0 END), 0) AS overdue, \
                        COALESCE(SUM(CASE WHEN {WO_TODAY} AND NOT {WO_OVERDUE} THEN 1 ELSE 0 END), 0) AS today \
                 {from}"
            ),
            vec![
                at.clone().into(),
                at.clone().into(),
                at.clone().into(),
                at.clone().into(),
                user_id.into(),
            ],
        ))
        .await?,
        "my work orders",
    )?;

    let rank = urgency_rank_sql("ul.code");
    let urgency_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT ul.code AS urgency_code, COUNT(*) AS cnt {from} GROUP BY ul.code ORDER BY MIN({rank})"),
            vec![user_id.into()],
        ))
        .await?;
    let by_urgency = urgency_rows
        .iter()
        .map(|r| {
            Ok(UrgencyCount {
                urgency_code: decode(r, "urgency_code")?,
                count: decode(r, "cnt")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let item_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, wo.code, wo.title, s.code AS status_code, ul.code AS urgency_code, \
                        wo.planned_start, wo.planned_end, \
                        COALESCE(e.asset_id_code || ' — ' || e.name, e.name) AS asset_label, \
                        CASE WHEN {WO_OVERDUE} THEN 1 ELSE 0 END AS overdue \
                 {from} \
                 ORDER BY overdue DESC, {rank} ASC, \
                          CASE WHEN wo.planned_end IS NULL THEN 1 ELSE 0 END, wo.planned_end ASC, wo.id ASC \
                 LIMIT {ITEM_LIMIT}"
            ),
            vec![at.into(), user_id.into()],
        ))
        .await?;
    let items = item_rows
        .iter()
        .map(|r| {
            Ok(MyWorkOrderItem {
                id: decode(r, "id")?,
                code: decode(r, "code")?,
                title: decode(r, "title")?,
                status_code: decode(r, "status_code")?,
                urgency_code: decode(r, "urgency_code")?,
                planned_start: decode(r, "planned_start")?,
                planned_end: decode(r, "planned_end")?,
                asset_label: decode(r, "asset_label")?,
                overdue: decode::<i64>(r, "overdue")? == 1,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let (deisolation_pending, deisolation_items) = my_deisolation(db, user_id).await?;
    Ok(MyWorkOrders {
        open_rule: crate::wo::backlog::RULE_OPEN_WORK_TO_DO.to_string(),
        open: decode(&row, "open")?,
        overdue: decode(&row, "overdue")?,
        today: decode(&row, "today")?,
        by_urgency,
        items,
        deisolation_pending,
        deisolation_items,
    })
}

/// [`MyWorkOrders::deisolation_pending`] of `user_id`: the count and the first ones.
async fn my_deisolation(db: &DatabaseConnection, user_id: i64) -> AppResult<(i64, Vec<MyWorkOrderItem>)> {
    let from = format!(
        "FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         LEFT JOIN reference_values ul ON ul.id = wo.urgency_id \
         LEFT JOIN equipment e ON e.id = wo.equipment_id \
         WHERE {pending} \
           AND (wo.primary_responsible_id = ? \
                OR EXISTS (SELECT 1 FROM permit_isolations pi JOIN work_permits p ON p.id = pi.permit_id \
                           WHERE p.linked_work_order_id = wo.id AND pi.applied_at IS NOT NULL \
                             AND pi.removal_verified_at IS NULL AND pi.applied_by_id = ?))",
        pending = crate::permit::wo_gate::deisolation_pending_sql("wo")
    );
    let count = one_row(
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS n {from}"),
            vec![user_id.into(), user_id.into()],
        ))
        .await?,
        "my de-isolation count",
    )?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, wo.code, wo.title, s.code AS status_code, ul.code AS urgency_code, \
                        wo.planned_start, wo.planned_end, \
                        COALESCE(e.asset_id_code || ' — ' || e.name, e.name) AS asset_label \
                 {from} \
                 ORDER BY wo.mechanically_completed_at ASC, wo.id ASC \
                 LIMIT {ITEM_LIMIT}"
            ),
            vec![user_id.into(), user_id.into()],
        ))
        .await?;
    let items = rows
        .iter()
        .map(|r| {
            Ok(MyWorkOrderItem {
                id: decode(r, "id")?,
                code: decode(r, "code")?,
                title: decode(r, "title")?,
                status_code: decode(r, "status_code")?,
                urgency_code: decode(r, "urgency_code")?,
                planned_start: decode(r, "planned_start")?,
                planned_end: decode(r, "planned_end")?,
                asset_label: decode(r, "asset_label")?,
                // The work is done: a planned end passed is not "overdue" work.
                overdue: false,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((decode(&count, "n")?, items))
}

pub async fn my_requests(db: &DatabaseConnection, user_id: i64) -> AppResult<MyRequests> {
    let counts = crate::di::queries::count_di_queues(db, user_id).await?;
    Ok(MyRequests {
        open: counts.mine,
        awaiting_my_answer: counts.mine_awaiting_info,
    })
}

pub async fn my_rounds(db: &DatabaseConnection, user_id: i64, now: DateTime<Utc>) -> AppResult<MyRounds> {
    let Some(personnel_id) = crate::qualification::gate::personnel_for_user(db, user_id).await? else {
        return Ok(MyRounds {
            personnel_linked: false,
            overdue: 0,
            today: 0,
            next_7_days: 0,
            todo: 0,
            items: Vec::new(),
        });
    };
    let at = instant(now);
    let row = one_row(
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(SUM(CASE WHEN {ROUND_OVERDUE_SQL} THEN 1 ELSE 0 END), 0) AS overdue, \
                        COALESCE(SUM(CASE WHEN NOT ({ROUND_OVERDUE_SQL}) \
                             AND date(ir.scheduled_at, 'localtime') = date(?, 'localtime') THEN 1 ELSE 0 END), 0) AS today, \
                        COALESCE(SUM(CASE WHEN date(ir.scheduled_at, 'localtime') > date(?, 'localtime') \
                             AND julianday(ir.scheduled_at) <= julianday(?) + 7 THEN 1 ELSE 0 END), 0) AS next_7_days, \
                        COUNT(*) AS todo \
                 FROM inspection_rounds ir \
                 WHERE ir.assigned_to_id = ? AND {ROUND_TODO_SQL}"
            ),
            vec![
                at.clone().into(),
                at.clone().into(),
                at.into(),
                personnel_id.into(),
            ],
        ))
        .await?,
        "my rounds",
    )?;
    let item_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ir.id, t.code AS template_code, t.name AS template_name, ir.status, ir.scheduled_at, \
                        CASE WHEN {ROUND_OVERDUE_SQL} THEN 1 ELSE 0 END AS overdue \
                 FROM inspection_rounds ir JOIN inspection_templates t ON t.id = ir.template_id \
                 WHERE ir.assigned_to_id = ? AND {ROUND_TODO_SQL} \
                 ORDER BY overdue DESC, CASE WHEN ir.scheduled_at IS NULL THEN 1 ELSE 0 END, \
                          julianday(ir.scheduled_at) ASC, ir.id ASC \
                 LIMIT {ITEM_LIMIT}"
            ),
            vec![personnel_id.into()],
        ))
        .await?;
    let items = item_rows
        .iter()
        .map(|r| {
            Ok(MyRoundItem {
                id: decode(r, "id")?,
                template_code: decode(r, "template_code")?,
                template_name: decode(r, "template_name")?,
                status: decode(r, "status")?,
                scheduled_at: decode(r, "scheduled_at")?,
                overdue: decode::<i64>(r, "overdue")? == 1,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(MyRounds {
        personnel_linked: true,
        overdue: decode(&row, "overdue")?,
        today: decode(&row, "today")?,
        next_7_days: decode(&row, "next_7_days")?,
        todo: decode(&row, "todo")?,
        items,
    })
}

pub async fn my_pm(db: &DatabaseConnection, user_id: i64, now: DateTime<Utc>) -> AppResult<MyPm> {
    let at = instant(now);
    let from = "FROM pm_occurrences po \
         JOIN pm_plans pp ON pp.id = po.pm_plan_id \
         JOIN work_orders wo ON wo.id = po.linked_work_order_id \
         WHERE wo.primary_responsible_id = ?";
    let binds = |extra: Vec<Value>| -> Vec<Value> {
        let mut v = extra;
        v.push(user_id.into());
        v
    };
    let row = one_row(
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(SUM(CASE WHEN {PM_OVERDUE_SQL} THEN 1 ELSE 0 END), 0) AS overdue, \
                        COALESCE(SUM(CASE WHEN {PM_DUE_NEXT_7D_SQL} THEN 1 ELSE 0 END), 0) AS due_next_7_days, \
                        COALESCE(SUM(CASE WHEN {PM_PENDING_SQL} THEN 1 ELSE 0 END), 0) AS pending \
                 {from}"
            ),
            binds(vec![at.clone().into(), at.clone().into(), at.clone().into()]),
        ))
        .await?,
        "my pm",
    )?;
    let item_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT po.id, pp.code AS plan_code, pp.title AS plan_title, po.due_at, wo.code AS work_order_code, \
                        CASE WHEN {PM_OVERDUE_SQL} THEN 1 ELSE 0 END AS overdue \
                 {from} AND {PM_PENDING_SQL} \
                 ORDER BY overdue DESC, CASE WHEN po.due_at IS NULL THEN 1 ELSE 0 END, po.due_at ASC, po.id ASC \
                 LIMIT {ITEM_LIMIT}"
            ),
            binds(vec![at.into()]),
        ))
        .await?;
    let items = item_rows
        .iter()
        .map(|r| {
            Ok(MyPmItem {
                id: decode(r, "id")?,
                plan_code: decode(r, "plan_code")?,
                plan_title: decode(r, "plan_title")?,
                due_at: decode(r, "due_at")?,
                work_order_code: decode(r, "work_order_code")?,
                overdue: decode::<i64>(r, "overdue")? == 1,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(MyPm {
        overdue: decode(&row, "overdue")?,
        due_next_7_days: decode(&row, "due_next_7_days")?,
        pending: decode(&row, "pending")?,
        items,
    })
}

pub async fn my_notifications(db: &DatabaseConnection, user_id: i32) -> AppResult<MyNotifications> {
    let visibility = crate::notifications::inbox::visible_to(db, user_id).await?;
    let counts = crate::notifications::inbox::counts(db, i64::from(user_id), &visibility).await?;
    Ok(MyNotifications {
        unread: counts.unread,
        to_acknowledge: counts.to_acknowledge,
    })
}

/// Which sections the person may see (the module's own view permission, checked by the caller).
#[derive(Debug, Clone, Copy, Default)]
pub struct MyWorkAccess {
    pub work_orders: bool,
    pub requests: bool,
    pub rounds: bool,
    pub pm: bool,
}

pub async fn build_my_work(
    db: &DatabaseConnection,
    user_id: i32,
    access: MyWorkAccess,
    now: DateTime<Utc>,
) -> AppResult<HomeMyWork> {
    let uid = i64::from(user_id);
    Ok(HomeMyWork {
        as_of: instant(now),
        work_orders: if access.work_orders {
            Some(my_work_orders(db, uid, now).await?)
        } else {
            None
        },
        requests: if access.requests {
            Some(my_requests(db, uid).await?)
        } else {
            None
        },
        rounds: if access.rounds {
            Some(my_rounds(db, uid, now).await?)
        } else {
            None
        },
        pm: if access.pm {
            Some(my_pm(db, uid, now).await?)
        } else {
            None
        },
        notifications: my_notifications(db, user_id).await?,
    })
}
