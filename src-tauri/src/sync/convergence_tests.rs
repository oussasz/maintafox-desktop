//! Golden exchange fixtures (`fixtures/exchange_*_v1.json`).
//!
//! Two-device convergence of every entity type on the generic capture/apply engine is covered by
//! `row_sync_tests` (create, update and delete on device A arrive identical on device B).

use sea_orm::DatabaseConnection;

use crate::sync::domain::{ApplySyncBatchInput, SYNC_PROTOCOL_VERSION_V1};
use crate::sync::materialize::materialize_inbox_batch;
use crate::sync::queries::apply_sync_batch;

async fn setup_db() -> DatabaseConnection {
    crate::db::test_db::migrated().await
}

#[tokio::test]
async fn golden_exchange_request_fixture_validates() {
    let raw = include_str!("fixtures/exchange_request_v1.json");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let value: serde_json::Value = serde_json::from_str(raw).expect("parse request fixture");
    crate::vps::guards::validate_sync_exchange_request_json(&value).expect("request fixture must satisfy VPS contract");
}

#[tokio::test]
async fn golden_exchange_response_fixture_applies() {
    let db = setup_db().await;
    let raw = include_str!("fixtures/exchange_response_v1.json");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let batch: ApplySyncBatchInput = serde_json::from_str(raw).expect("parse response fixture");
    assert_eq!(batch.protocol_version, SYNC_PROTOCOL_VERSION_V1);
    apply_sync_batch(&db, batch).await.expect("apply fixture");
    let mat = materialize_inbox_batch(&db, Some(50))
        .await
        .expect("materialize fixture");
    assert!(mat.applied >= 1);
}
