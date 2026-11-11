//! "Rejoindre" (`wo::join`, CMMS Advisor decision 2026-11-11, option b): joining a work order in
//! progress starts one's own labour and tells the assignee; the joiner does not become an
//! execution actor; one open labour entry per joiner.

use chrono::Duration;
use sea_orm::DatabaseConnection;

use crate::auth::scope_tests::{assign, exec, role, scalar, tree, user};
use crate::rbac::permissions::{OT_CREATE, OT_EDIT, OT_VIEW};
use crate::wo::depannage::{self, WoDepannageInput};
use crate::wo::join::{self, WoJoinInput, CODE_ALREADY_ON_IT, JOINED_CATEGORY};
use crate::wo::time::stamp;

async fn told(db: &DatabaseConnection, wo_id: i64, user: i32) -> bool {
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE e.category_code = '{JOINED_CATEGORY}' AND e.event_code = 'wo_joined' \
                AND e.source_record_id = '{wo_id}' AND n.recipient_user_id = {user}"
        ),
    )
    .await
        > 0
}

#[tokio::test]
async fn joining_starts_ones_own_labour_tells_the_assignee_and_never_makes_an_actor() {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let tech = user(&db, "join_tech").await;
    let helper = user(&db, "join_helper").await;
    let tech_role = role(&db, "Join tech A", &[OT_VIEW, OT_CREATE, OT_EDIT]).await;
    let helper_role = role(&db, "Join helper A", &[OT_VIEW, OT_EDIT]).await;
    assign(&db, tech, tech_role, "org_node", Some(t.site_a)).await;
    assign(&db, helper, helper_role, "org_node", Some(t.site_a)).await;
    exec(
        &db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('join-eq-a', 'JOIN-A', 'Equipment A', 'active_in_service', {},
                     datetime('now'), datetime('now'))",
            t.site_a
        ),
    )
    .await;
    let eq_a = scalar(&db, "SELECT id FROM equipment WHERE entity_sync_id = 'join-eq-a'").await;
    // A dépannage started by the technician, assigned to them, in progress.
    let wo = depannage::start_depannage(
        &db,
        WoDepannageInput {
            equipment_id: eq_a,
            type_code: None,
            urgency_code: Some("emergency".into()),
            subtype_code: None,
            title: "Fuite vanne".into(),
            description: None,
            breakdown_at: Some(stamp(crate::wo::time::now_utc_second() - Duration::minutes(10))),
            machine_stopped: Some(true),
            assignee_id: None,
            requires_permit: None,
            actor_id: i64::from(tech),
            permit_drafting: None,
        },
    )
    .await
    .expect("dépannage")
    .wo;
    assert_eq!(wo.status_code.as_deref(), Some("in_progress"));

    let entry = join::join_wo(
        &db,
        WoJoinInput {
            wo_id: wo.id,
            actor_id: i64::from(helper),
        },
    )
    .await
    .expect("the helper joins");
    assert_eq!(entry.intervener_id, Some(i64::from(helper)));
    assert!(
        entry.started_at.is_some() && entry.ended_at.is_none(),
        "labour running from now"
    );
    assert!(told(&db, wo.id, tech).await, "the assignee is told who joined");

    let err = join::join_wo(
        &db,
        WoJoinInput {
            wo_id: wo.id,
            actor_id: i64::from(helper),
        },
    )
    .await
    .expect_err("labour already running");
    assert!(format!("{err:?}").contains(CODE_ALREADY_ON_IT), "{err:?}");

    // The joiner is not an execution actor: the execution actions stay refused to them.
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let map = crate::auth::scope::access_map_cached(&db, &cache, helper)
        .await
        .expect("access map");
    let standing = crate::wo::execution_actor::standing(&db, &map, i64::from(helper), wo.id)
        .await
        .expect("standing");
    assert!(standing.refusal.is_some(), "joining never makes an execution actor");

    // The assignee joining their own work order tells nobody.
    join::join_wo(
        &db,
        WoJoinInput {
            wo_id: wo.id,
            actor_id: i64::from(tech),
        },
    )
    .await
    .expect("the assignee books their own labour");
    assert!(!told(&db, wo.id, helper).await);
}
