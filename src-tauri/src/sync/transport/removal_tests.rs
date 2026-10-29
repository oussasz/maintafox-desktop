//! The removal procedure (`run_removal`) with recorded steps: never an erase before the upload is
//! acknowledged, never a confirmation before the erase, nothing uploaded for a revoked phone.

use std::collections::{BTreeMap, VecDeque};

use super::*;

const OLD_TOKEN: &str = "old-device-token-5c1e";

/// Steps that record what the procedure asked, in order.
#[derive(Default)]
struct Recorded {
    events: Vec<String>,
    rounds: VecDeque<UploadRound>,
    leftover: BTreeMap<String, i64>,
    erase_fails: bool,
    confirm_answer: bool,
    published: Vec<PhoneRemovalStatus>,
}

impl RemovalSteps for Recorded {
    async fn end_session(&mut self) {
        self.events.push("end_session".into());
    }

    async fn upload_round(&mut self, kind: RemovalKind) -> AppResult<UploadRound> {
        self.events.push(format!("upload:{kind:?}"));
        Ok(self.rounds.pop_front().unwrap_or(UploadRound::More { uploaded: 1 }))
    }

    async fn leftover(&mut self) -> AppResult<BTreeMap<String, i64>> {
        self.events.push("leftover".into());
        Ok(self.leftover.clone())
    }

    async fn erase(&mut self) -> AppResult<()> {
        self.events.push("erase".into());
        if self.erase_fails {
            return Err(crate::errors::AppError::Internal(anyhow::anyhow!("disk full")));
        }
        Ok(())
    }

    async fn regenerate_identity(&mut self) -> AppResult<()> {
        self.events.push("regenerate".into());
        Ok(())
    }

    async fn confirm_wipe(&mut self, old_token: &str) -> bool {
        self.events.push(format!("confirm:{old_token}"));
        self.confirm_answer
    }

    fn publish(&mut self, status: PhoneRemovalStatus) {
        self.published.push(status);
    }
}

fn failure() -> SyncWorkerError {
    SyncWorkerError {
        code: "offline".into(),
        kind: "network_unreachable",
        message: "no route".into(),
        http_status: None,
        at: "2026-10-29T08:00:00Z".into(),
    }
}

fn position(events: &[String], prefix: &str) -> Option<usize> {
    events.iter().position(|e| e.starts_with(prefix))
}

async fn removal_of(db: &DatabaseConnection, kind: RemovalKind) -> PendingRemoval {
    record(db, kind).await.expect("recorded")
}

#[test]
fn only_these_refusals_remove_a_phone() {
    assert_eq!(
        kind_of_refusal(403, Some("device_wipe_required")),
        Some(RemovalKind::Removed)
    );
    assert_eq!(
        kind_of_refusal(403, Some("activation_revoked")),
        Some(RemovalKind::Revoked)
    );
    for (status, code) in [
        (401, Some("device_wipe_required")),
        (403, Some("license_revoked")),
        (403, Some("tenant_inactive")),
        (401, Some("token_superseded")),
        (403, None),
        (500, Some("activation_revoked")),
    ] {
        assert_eq!(kind_of_refusal(status, code), None, "{status} {code:?}");
    }
    assert!(!applies_on_this_device() || cfg!(target_os = "android"));
}

#[tokio::test]
async fn a_stronger_cause_replaces_a_weaker_one_never_the_reverse() {
    let db = crate::db::test_db::seeded().await;
    assert_eq!(pending(&db).await.expect("read"), None);
    assert_eq!(removal_of(&db, RemovalKind::Unbound).await.kind, RemovalKind::Unbound);
    assert_eq!(removal_of(&db, RemovalKind::Removed).await.kind, RemovalKind::Removed);
    assert_eq!(removal_of(&db, RemovalKind::Unbound).await.kind, RemovalKind::Removed);
    assert_eq!(removal_of(&db, RemovalKind::Revoked).await.kind, RemovalKind::Revoked);
    assert_eq!(removal_of(&db, RemovalKind::Removed).await.kind, RemovalKind::Revoked);
}

#[tokio::test]
async fn a_removed_phone_uploads_everything_then_erases_then_confirms_with_the_old_token() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::More { uploaded: 200 }, UploadRound::Empty { uploaded: 12 }]),
        leftover: BTreeMap::from([("media_files".to_string(), 2)]),
        confirm_answer: true,
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(
        steps.events,
        [
            "end_session",
            "upload:Removed",
            "upload:Removed",
            "leftover",
            "erase",
            "regenerate",
            format!("confirm:{OLD_TOKEN}").as_str(),
        ]
    );
    let RemovalOutcome::Done(status) = outcome else {
        panic!("not done: {outcome:?}");
    };
    assert_eq!(status.phase, RemovalPhase::Done);
    assert_eq!(status.uploaded_count, 212);
    assert_eq!(status.dropped_count, 2);
    assert_eq!(status.wipe_confirmed, Some(true));
    assert_eq!(steps.published.last(), Some(&status));
    assert_eq!(pending(&db).await.expect("read"), None, "the record goes once finished");
}

#[tokio::test]
async fn a_failed_upload_keeps_everything_and_the_phone_locked() {
    for kind in [RemovalKind::Removed, RemovalKind::Unbound] {
        let db = crate::db::test_db::seeded().await;
        let removal = removal_of(&db, kind).await;
        let mut steps = Recorded {
            rounds: VecDeque::from([
                UploadRound::More { uploaded: 5 },
                UploadRound::Failed {
                    error: failure(),
                    retry_after: None,
                },
            ]),
            ..Recorded::default()
        };
        let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
            .await
            .expect("run");
        assert!(
            matches!(&outcome, RemovalOutcome::Waiting { error: Some(e), .. } if e.code == "offline"),
            "{outcome:?}"
        );
        assert_eq!(position(&steps.events, "erase"), None, "{:?}", steps.events);
        assert_eq!(position(&steps.events, "confirm"), None);
        assert_eq!(position(&steps.events, "regenerate"), None);
        let still = pending(&db).await.expect("read").expect("still pending");
        assert!(!still.erased);
        assert_eq!(
            steps.published.last().map(|s| s.phase),
            Some(RemovalPhase::WaitingToUpload)
        );
        assert_eq!(
            steps.published.last().and_then(|s| s.last_error.clone()),
            Some(failure())
        );
    }
}

#[tokio::test]
async fn an_upload_never_finished_in_one_attempt_is_never_erased() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded::default(); // every round: more remains
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(
        outcome,
        RemovalOutcome::Waiting {
            error: None,
            retry_after: None
        }
    );
    assert_eq!(position(&steps.events, "erase"), None);
    let progress = pending(&db).await.expect("read").expect("pending");
    assert_eq!(progress.uploaded_count, i64::from(REMOVAL_UPLOAD_ROUNDS_PER_ATTEMPT));
}

#[tokio::test]
async fn a_revoked_phone_uploads_nothing_and_logs_what_it_drops() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Revoked).await;
    let mut steps = Recorded {
        leftover: BTreeMap::from([("work_orders".to_string(), 3), ("media_files".to_string(), 1)]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(steps.events, ["end_session", "leftover", "erase", "regenerate"]);
    let RemovalOutcome::Done(status) = outcome else {
        panic!("not done: {outcome:?}");
    };
    assert_eq!(status.dropped_count, 4);
    assert_eq!(status.dropped_by_kind.get("work_orders"), Some(&3));
    assert_eq!(status.wipe_confirmed, None, "a revoked phone confirms nothing");
}

#[tokio::test]
async fn revoked_during_the_upload_erases_the_rest_without_confirming() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::More { uploaded: 4 }, UploadRound::Revoked]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(position(&steps.events, "confirm"), None, "{:?}", steps.events);
    assert!(position(&steps.events, "leftover") < position(&steps.events, "erase"));
    let RemovalOutcome::Done(status) = outcome else {
        panic!("not done: {outcome:?}");
    };
    assert_eq!(status.kind, Some(RemovalKind::Revoked));
    assert_eq!(status.uploaded_count, 4);
}

#[tokio::test]
async fn an_unbound_phone_uploads_erases_and_confirms_nothing() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Unbound).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty { uploaded: 7 }]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(
        steps.events,
        ["end_session", "upload:Unbound", "leftover", "erase", "regenerate"]
    );
    assert!(matches!(outcome, RemovalOutcome::Done(ref s) if s.wipe_confirmed.is_none() && s.uploaded_count == 7));
}

#[tokio::test]
async fn a_failed_erase_regenerates_and_confirms_nothing_and_is_retried() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty { uploaded: 0 }]),
        erase_fails: true,
        confirm_answer: true,
        ..Recorded::default()
    };
    run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect_err("erase failed");
    assert_eq!(position(&steps.events, "regenerate"), None);
    assert_eq!(position(&steps.events, "confirm"), None);
    assert!(!pending(&db).await.expect("read").expect("pending").erased);
}

#[tokio::test]
async fn after_a_restart_past_the_erase_it_only_regenerates_and_lets_the_window_expire() {
    let db = crate::db::test_db::seeded().await;
    let mut removal = removal_of(&db, RemovalKind::Removed).await;
    removal.erased = true;
    removal.uploaded_count = 9;
    save(&db, &removal, "test").await.expect("save");
    let mut steps = Recorded::default();
    let outcome = run_removal(&db, &mut steps, removal, None).await.expect("run");
    assert_eq!(steps.events, ["end_session", "regenerate"]);
    assert!(matches!(outcome, RemovalOutcome::Done(ref s) if s.wipe_confirmed == Some(false) && s.uploaded_count == 9));
    assert_eq!(pending(&db).await.expect("read"), None);
}

#[tokio::test]
async fn unsent_work_is_counted_per_kind() {
    let db = crate::db::test_db::seeded().await;
    let counts = unsent_by_kind(&db).await.expect("counts");
    assert!(counts.values().all(|n| *n > 0), "{counts:?}");
}
