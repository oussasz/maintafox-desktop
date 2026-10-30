//! The removal procedure (`run_removal`) with recorded steps: never an erase before the upload is
//! acknowledged, never a confirmation before the erase, nothing uploaded for a phone the server
//! marked for erasing, nothing erased for a phone refused without that marker. The worker's own
//! steps against the HTTP stub are in `tests.rs` ("phone removal").

use std::collections::{BTreeMap, VecDeque};

use super::*;

const OLD_TOKEN: &str = "old-device-token-5c1e";

/// Steps that record what the procedure asked, in order.
#[derive(Default)]
struct Recorded {
    events: Vec<String>,
    rounds: VecDeque<UploadRound>,
    probes: VecDeque<Probe>,
    requeued: i64,
    /// Unsent work found right before the erase, one answer per check (then 0).
    unsent_before_erase: VecDeque<i64>,
    leftover: BTreeMap<String, i64>,
    erase_fails: bool,
    confirm_answer: bool,
    confirmed_dropped: Option<BTreeMap<String, i64>>,
    published: Vec<PhoneRemovalStatus>,
}

impl RemovalSteps for Recorded {
    async fn end_session(&mut self) {
        self.events.push("end_session".into());
    }

    async fn probe(&mut self) -> Probe {
        self.events.push("probe".into());
        self.probes
            .pop_front()
            .unwrap_or(Probe::Refused(RemovalKind::RevokedUnconfirmed))
    }

    async fn requeue_failures(&mut self) -> AppResult<i64> {
        self.events.push("requeue".into());
        Ok(self.requeued)
    }

    async fn upload_round(&mut self, kind: RemovalKind) -> AppResult<UploadRound> {
        self.events.push(format!("upload:{kind:?}"));
        Ok(self.rounds.pop_front().unwrap_or(UploadRound::More {
            uploaded: up(&[("x", 1)]),
        }))
    }

    async fn unsent_remaining(&mut self) -> AppResult<i64> {
        self.events.push("unsent".into());
        Ok(self.unsent_before_erase.pop_front().unwrap_or(0))
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

    async fn confirm_wipe(&mut self, old_token: &str, dropped: &BTreeMap<String, i64>) -> bool {
        self.events.push(format!("confirm:{old_token}"));
        self.confirmed_dropped = Some(dropped.clone());
        self.confirm_answer
    }

    fn publish(&mut self, status: PhoneRemovalStatus) {
        self.published.push(status);
    }
}

fn up(kinds: &[(&str, i64)]) -> Uploaded {
    kinds.iter().map(|(k, n)| ((*k).to_string(), *n)).collect()
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
fn only_these_refusals_remove_or_lock_a_phone() {
    assert_eq!(
        kind_of_refusal(403, Some("device_wipe_required"), false),
        Some(RemovalKind::Removed)
    );
    assert_eq!(
        kind_of_refusal(403, Some("activation_revoked"), true),
        Some(RemovalKind::Revoked)
    );
    assert_eq!(
        kind_of_refusal(403, Some("activation_revoked"), false),
        Some(RemovalKind::RevokedUnconfirmed),
        "without the marker: locked, never erased"
    );
    for (status, code) in [
        (401, Some("device_wipe_required")),
        (403, Some("license_revoked")),
        (403, Some("tenant_inactive")),
        (401, Some("token_superseded")),
        (403, None),
        (500, Some("activation_revoked")),
    ] {
        assert_eq!(kind_of_refusal(status, code, true), None, "{status} {code:?}");
    }
    assert!(!applies_on_this_device() || cfg!(target_os = "android"));
}

#[test]
fn only_the_explicit_marker_erases_without_upload() {
    let marked = |reason: &str| {
        serde_json::json!({ "error": "activation_revoked", "message": "m", "wipe": "without_upload", "reason": reason })
            .to_string()
    };
    for reason in WIPE_REASONS {
        assert_eq!(wipe_marker_of_body(403, &marked(reason)).as_deref(), Some(reason));
        assert_eq!(kind_of_refusal_body(403, &marked(reason)), Some(RemovalKind::Revoked));
    }
    for body in [
        // A missing activation row, a vendor revocation, a released slot: no marker.
        serde_json::json!({ "error": "activation_revoked" }).to_string(),
        serde_json::json!({ "error": "activation_revoked", "wipe": "later", "reason": "lost" }).to_string(),
        serde_json::json!({ "error": "activation_revoked", "wipe": "without_upload" }).to_string(),
        marked("unknown_reason"),
        "not json".to_string(),
    ] {
        assert_eq!(wipe_marker_of_body(403, &body), None, "{body}");
        assert_ne!(kind_of_refusal_body(403, &body), Some(RemovalKind::Revoked), "{body}");
    }
    assert_eq!(wipe_marker_of_body(401, &marked("lost")), None);
    assert_eq!(
        kind_of_refusal_body(403, &serde_json::json!({ "error": "activation_revoked" }).to_string()),
        Some(RemovalKind::RevokedUnconfirmed)
    );
    let err = super::super::client::http_error(403, &marked("lost"), None);
    assert_eq!(kind_of_transport_error(&err), Some(RemovalKind::Revoked));
    let err = super::super::client::http_error(403, r#"{"error":"activation_revoked"}"#, None);
    assert_eq!(kind_of_transport_error(&err), Some(RemovalKind::RevokedUnconfirmed));
}

#[tokio::test]
async fn the_marker_is_final_and_the_other_causes_follow_the_server() {
    let db = crate::db::test_db::seeded().await;
    assert_eq!(pending(&db).await.expect("read"), None);
    assert_eq!(removal_of(&db, RemovalKind::Unbound).await.kind, RemovalKind::Unbound);
    assert_eq!(removal_of(&db, RemovalKind::Removed).await.kind, RemovalKind::Removed);
    assert_eq!(removal_of(&db, RemovalKind::Unbound).await.kind, RemovalKind::Removed);
    assert_eq!(
        removal_of(&db, RemovalKind::RevokedUnconfirmed).await.kind,
        RemovalKind::RevokedUnconfirmed
    );
    assert_eq!(
        removal_of(&db, RemovalKind::Unbound).await.kind,
        RemovalKind::RevokedUnconfirmed
    );
    assert_eq!(removal_of(&db, RemovalKind::Removed).await.kind, RemovalKind::Removed);
    assert_eq!(removal_of(&db, RemovalKind::Revoked).await.kind, RemovalKind::Revoked);
    for later in [
        RemovalKind::Removed,
        RemovalKind::RevokedUnconfirmed,
        RemovalKind::Unbound,
    ] {
        assert_eq!(removal_of(&db, later).await.kind, RemovalKind::Revoked);
    }
}

#[tokio::test]
async fn an_unreadable_record_locks_the_phone_and_erases_nothing() {
    let db = crate::db::test_db::seeded().await;
    crate::settings::set_setting(&db, REMOVAL_KEY, REMOVAL_SCOPE, "{\"kind\":\"???\"}", None, "test")
        .await
        .expect("write");
    let read = pending(&db).await.expect("read").expect("pending");
    assert_eq!(read.kind, RemovalKind::RevokedUnconfirmed);
}

#[tokio::test]
async fn a_removed_phone_uploads_everything_then_erases_then_confirms_with_the_old_token_and_the_counts() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([
            UploadRound::More {
                uploaded: up(&[("work_orders", 200)]),
            },
            UploadRound::Empty {
                uploaded: up(&[("work_orders", 10), (MEDIA_FILES_KIND, 2)]),
            },
        ]),
        requeued: 3,
        leftover: BTreeMap::from([(MEDIA_FILES_KIND.to_string(), 2)]),
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
            "requeue",
            "upload:Removed",
            "upload:Removed",
            "unsent",
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
    assert_eq!(status.uploaded_by_kind.get("work_orders"), Some(&210));
    assert_eq!(status.uploaded_by_kind.get(MEDIA_FILES_KIND), Some(&2));
    assert_eq!(status.dropped_count, 2);
    assert_eq!(status.wipe_confirmed, Some(true));
    assert_eq!(
        steps.confirmed_dropped,
        Some(BTreeMap::from([(MEDIA_FILES_KIND.to_string(), 2)])),
        "the counts go with the confirmation"
    );
    assert_eq!(steps.published.last(), Some(&status));
    assert_eq!(pending(&db).await.expect("read"), None, "the record goes once finished");
}

#[tokio::test]
async fn given_up_work_is_put_back_once_per_removal() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Failed {
            uploaded: Uploaded::new(),
            error: failure(),
            retry_after: None,
        }]),
        ..Recorded::default()
    };
    run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    let again = pending(&db).await.expect("read").expect("pending");
    assert!(again.failures_requeued);
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty {
            uploaded: Uploaded::new(),
        }]),
        ..Recorded::default()
    };
    run_removal(&db, &mut steps, again, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(position(&steps.events, "requeue"), None, "{:?}", steps.events);
}

#[tokio::test]
async fn a_failed_upload_keeps_everything_and_the_phone_locked() {
    for kind in [RemovalKind::Removed, RemovalKind::Unbound] {
        let db = crate::db::test_db::seeded().await;
        let removal = removal_of(&db, kind).await;
        let mut steps = Recorded {
            rounds: VecDeque::from([
                UploadRound::More {
                    uploaded: up(&[("x", 5)]),
                },
                UploadRound::Failed {
                    uploaded: up(&[(MEDIA_FILES_KIND, 1)]),
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
        assert_eq!(still.uploaded_count, 6, "what did get through is counted");
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
async fn work_captured_right_before_the_erase_is_uploaded_first() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty {
            uploaded: Uploaded::new(),
        }]),
        unsent_before_erase: VecDeque::from([1]),
        ..Recorded::default()
    };
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
    assert_eq!(position(&steps.events, "erase"), None, "{:?}", steps.events);
    assert!(!pending(&db).await.expect("read").expect("pending").erased);
}

#[tokio::test]
async fn the_marker_erases_without_uploading_and_counts_what_it_drops() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Revoked).await;
    let mut steps = Recorded {
        leftover: BTreeMap::from([("work_orders".to_string(), 3), (MEDIA_FILES_KIND.to_string(), 1)]),
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
async fn the_marker_during_the_upload_erases_the_rest_without_confirming() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([
            UploadRound::More {
                uploaded: up(&[("x", 4)]),
            },
            UploadRound::Refused {
                uploaded: Uploaded::new(),
                kind: RemovalKind::Revoked,
                error: None,
            },
        ]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(position(&steps.events, "confirm"), None, "{:?}", steps.events);
    assert_eq!(
        position(&steps.events, "unsent"),
        None,
        "nothing more is sent after the marker"
    );
    assert!(position(&steps.events, "leftover") < position(&steps.events, "erase"));
    let RemovalOutcome::Done(status) = outcome else {
        panic!("not done: {outcome:?}");
    };
    assert_eq!(status.kind, Some(RemovalKind::Revoked));
    assert_eq!(status.uploaded_count, 4);
}

#[tokio::test]
async fn a_refusal_without_the_marker_during_the_upload_locks_and_keeps_everything() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Refused {
            uploaded: Uploaded::new(),
            kind: RemovalKind::RevokedUnconfirmed,
            error: None,
        }]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(outcome, RemovalOutcome::Blocked { error: None });
    assert_eq!(position(&steps.events, "erase"), None, "{:?}", steps.events);
    assert_eq!(position(&steps.events, "leftover"), None);
    let locked = pending(&db).await.expect("read").expect("pending");
    assert_eq!(locked.kind, RemovalKind::RevokedUnconfirmed);
    assert!(!locked.erased);
    assert_eq!(steps.published.last().map(|s| s.phase), Some(RemovalPhase::Blocked));
}

#[tokio::test]
async fn a_locked_phone_asks_the_server_and_stays_locked_until_it_answers() {
    let db = crate::db::test_db::seeded().await;
    for probe in [
        Probe::Refused(RemovalKind::RevokedUnconfirmed),
        Probe::Unanswered(failure()),
    ] {
        let removal = removal_of(&db, RemovalKind::RevokedUnconfirmed).await;
        let mut steps = Recorded {
            probes: VecDeque::from([probe.clone()]),
            ..Recorded::default()
        };
        let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
            .await
            .expect("run");
        assert!(matches!(outcome, RemovalOutcome::Blocked { .. }), "{outcome:?}");
        assert_eq!(steps.events, ["end_session", "probe"], "{probe:?}");
        let published = steps.published.last().expect("published");
        assert_eq!(published.phase, RemovalPhase::Blocked);
        assert_eq!(published.kind, Some(RemovalKind::RevokedUnconfirmed));
    }
}

#[tokio::test]
async fn a_locked_phone_accepted_again_is_unlocked_with_nothing_erased() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::RevokedUnconfirmed).await;
    let mut steps = Recorded {
        probes: VecDeque::from([Probe::Accepted]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(outcome, RemovalOutcome::Lifted);
    assert_eq!(steps.events, ["end_session", "probe"]);
    assert_eq!(pending(&db).await.expect("read"), None);
    assert_eq!(steps.published.last().map(|s| s.phase), Some(RemovalPhase::None));
}

#[tokio::test]
async fn a_locked_phone_then_removed_uploads_and_one_then_marked_erases() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::RevokedUnconfirmed).await;
    let mut steps = Recorded {
        probes: VecDeque::from([Probe::Refused(RemovalKind::Removed)]),
        rounds: VecDeque::from([UploadRound::Empty {
            uploaded: up(&[("x", 2)]),
        }]),
        confirm_answer: true,
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert!(position(&steps.events, "upload:Removed") < position(&steps.events, "erase"));
    assert!(matches!(outcome, RemovalOutcome::Done(ref s) if s.kind == Some(RemovalKind::Removed)));

    let removal = removal_of(&db, RemovalKind::RevokedUnconfirmed).await;
    let mut steps = Recorded {
        probes: VecDeque::from([Probe::Refused(RemovalKind::Revoked)]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(position(&steps.events, "upload"), None);
    assert!(matches!(outcome, RemovalOutcome::Done(ref s) if s.kind == Some(RemovalKind::Revoked)));
}

#[tokio::test]
async fn an_unbound_phone_uploads_erases_and_confirms_nothing() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Unbound).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty {
            uploaded: up(&[("x", 7)]),
        }]),
        ..Recorded::default()
    };
    let outcome = run_removal(&db, &mut steps, removal, Some(OLD_TOKEN.into()))
        .await
        .expect("run");
    assert_eq!(
        steps.events,
        [
            "end_session",
            "requeue",
            "upload:Unbound",
            "unsent",
            "leftover",
            "erase",
            "regenerate"
        ]
    );
    assert!(matches!(outcome, RemovalOutcome::Done(ref s) if s.wipe_confirmed.is_none() && s.uploaded_count == 7));
}

#[tokio::test]
async fn a_failed_erase_regenerates_and_confirms_nothing_and_is_retried() {
    let db = crate::db::test_db::seeded().await;
    let removal = removal_of(&db, RemovalKind::Removed).await;
    let mut steps = Recorded {
        rounds: VecDeque::from([UploadRound::Empty {
            uploaded: Uploaded::new(),
        }]),
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
async fn unsent_and_given_up_work_is_counted_per_kind() {
    let db = crate::db::test_db::seeded().await;
    let counts = unsent_by_kind(&db).await.expect("counts");
    assert!(counts.values().all(|n| *n > 0), "{counts:?}");
}

#[test]
fn uploaded_is_what_left_less_what_was_given_up() {
    let before = WorkCounts {
        unsent: BTreeMap::from([("a".into(), 10), (MEDIA_FILES_KIND.into(), 3)]),
        given_up: BTreeMap::from([("a".into(), 1)]),
    };
    let after = WorkCounts {
        unsent: BTreeMap::from([("a".into(), 2)]),
        given_up: BTreeMap::from([("a".into(), 3), (MEDIA_FILES_KIND.into(), 1)]),
    };
    assert_eq!(
        before.uploaded_until(&after),
        BTreeMap::from([("a".to_string(), 6), (MEDIA_FILES_KIND.to_string(), 2)])
    );
    assert_eq!(
        after.leftover(),
        BTreeMap::from([("a".to_string(), 5), (MEDIA_FILES_KIND.to_string(), 1)])
    );
}

#[test]
fn the_wipe_confirmation_body_follows_the_server_limits() {
    assert_eq!(wipe_confirmation_body(&BTreeMap::new()), serde_json::json!({}));
    let body = wipe_confirmation_body(&BTreeMap::from([
        ("work_orders".into(), 2),
        (MEDIA_FILES_KIND.into(), 1),
    ]));
    assert_eq!(
        body,
        serde_json::json!({ "dropped": { "total": 3, "by_kind": { "work_orders": 2, "media_files": 1 } } })
    );
    let many: BTreeMap<String, i64> = (0..40).map(|i| (format!("kind_{i:02}"), i64::from(i) + 1)).collect();
    let body = wipe_confirmation_body(&many);
    let by_kind = body["dropped"]["by_kind"].as_object().expect("by_kind");
    assert_eq!(by_kind.len(), 32);
    assert!(by_kind.contains_key("other"));
    assert_eq!(body["dropped"]["total"], many.values().sum::<i64>());
    let long = wipe_confirmation_body(&BTreeMap::from([("k".repeat(65), 4)]));
    assert_eq!(long["dropped"]["by_kind"], serde_json::json!({ "other": 4 }));
}
