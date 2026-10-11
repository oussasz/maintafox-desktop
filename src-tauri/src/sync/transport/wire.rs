//! Protocol v2 wire shapes (`maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md`).
//!
//! Inbound rows, acknowledgements, rejections and conflicts decode straight into the domain types
//! `apply_sync_batch` already takes, so the wire and the local apply path cannot drift apart.

use serde::{Deserialize, Serialize};

use crate::sync::domain::{
    ApplySyncBatchInput, SyncAckInput, SyncInboundItemInput, SyncMediaManifestItem, SyncOutboxItem,
    SyncRejectedItemInput, SyncServerConflictInput, TenantConfigSyncPayload, SYNC_PROTOCOL_VERSION_V2,
};

/// `device_class` of this application (contract §3: `"desktop" | "mobile"`).
pub const DEVICE_CLASS_DESKTOP: &str = "desktop";

/// Contract §3: at most this many users in `current_user_sync_ids`.
pub const MAX_CURRENT_USER_SYNC_IDS: usize = 100;

/// Contract §6: snapshot `scope`.
pub const SNAPSHOT_SCOPE_ALL: &str = "all";
pub const SNAPSHOT_SCOPE_USER: &str = "user";

#[derive(Debug, Serialize)]
pub struct ExchangeOutboxItem<'a> {
    pub idempotency_key: &'a str,
    pub entity_type: &'a str,
    pub entity_sync_id: &'a str,
    pub operation: &'a str,
    pub row_version: i64,
    /// Always serialized: `null` means the row is created by this change.
    pub base_row_version: Option<i64>,
    pub owner_user_sync_id: Option<&'a str>,
    pub payload_json: &'a str,
    pub payload_hash: &'a str,
}

impl<'a> From<&'a SyncOutboxItem> for ExchangeOutboxItem<'a> {
    fn from(item: &'a SyncOutboxItem) -> Self {
        Self {
            idempotency_key: &item.idempotency_key,
            entity_type: &item.entity_type,
            entity_sync_id: &item.entity_sync_id,
            operation: &item.operation,
            row_version: item.row_version,
            base_row_version: item.base_row_version,
            owner_user_sync_id: item.owner_user_sync_id.as_deref(),
            payload_json: &item.payload_json,
            payload_hash: &item.payload_hash,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ExchangeRequest<'a> {
    pub protocol_version: &'static str,
    pub idempotency_key: &'a str,
    pub checkpoint_token: Option<&'a str>,
    pub current_user_sync_ids: &'a [String],
    pub device_class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_config: Option<&'a TenantConfigSyncPayload>,
    #[serde(skip_serializing_if = "<[SyncMediaManifestItem]>::is_empty")]
    pub media_manifest: &'a [SyncMediaManifestItem],
    pub outbox_batch: Vec<ExchangeOutboxItem<'a>>,
}

/// The fields of a stored request body the worker needs again after the response.
#[derive(Debug, Deserialize)]
pub struct StoredExchangeRequest {
    pub current_user_sync_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeResponse {
    pub protocol_version: String,
    pub server_batch_id: String,
    pub checkpoint_token: String,
    pub acknowledged_items: Vec<SyncAckInput>,
    pub rejected_items: Vec<SyncRejectedItemInput>,
    pub conflict_items: Vec<SyncServerConflictInput>,
    pub inbound_items: Vec<SyncInboundItemInput>,
    pub has_more: bool,
    #[serde(default)]
    pub user_snapshot_required: Vec<String>,
    #[serde(default)]
    pub policy_metadata_json: Option<String>,
}

impl ExchangeResponse {
    pub fn into_apply_input(self) -> ApplySyncBatchInput {
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
            server_batch_id: self.server_batch_id,
            checkpoint_token: self.checkpoint_token,
            acknowledged_items: self.acknowledged_items,
            rejected_items: self.rejected_items,
            inbound_items: self.inbound_items,
            conflict_items: self.conflict_items,
            policy_metadata_json: self.policy_metadata_json,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SnapshotRequest<'a> {
    pub protocol_version: &'static str,
    pub page_token: Option<&'a str>,
    pub page_size: u32,
    pub scope: &'static str,
    pub current_user_sync_ids: &'a [String],
}

#[derive(Debug, Deserialize)]
pub struct SnapshotResponse {
    pub protocol_version: String,
    pub items: Vec<SyncInboundItemInput>,
    pub has_more: bool,
    #[serde(default)]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub checkpoint_token: Option<String>,
}

/// `POST /api/v1/sync/media/presign-put` body (server `SyncMediaPresignPutBody`).
#[derive(Debug, Serialize)]
pub struct PresignPutRequest<'a> {
    pub parent_entity_type: &'a str,
    pub parent_entity_sync_id: &'a str,
    pub content_sha256: &'a str,
    pub object_key: &'a str,
}

/// `POST /api/v1/sync/media/presign-get` body (server `SyncMediaPresignGetBody`).
#[derive(Debug, Serialize)]
pub struct PresignGetRequest<'a> {
    pub object_key: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct PresignResponse {
    pub url: String,
    pub method: String,
    pub object_key: String,
}

/// Control API error body: `{ "error": code, "message"?: text, "details"?: any }`.
#[derive(Debug, Deserialize)]
pub struct ErrorBody {
    pub error: Option<String>,
    pub message: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// `POST /api/v1/notifications/email` body: the server sends one notification email to the
/// recipient's address on record (the desktop never handles SMTP or addresses). Contract in
/// `notifications::email`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NotificationEmailRequest {
    /// Idempotency key: the same notification gives the same id on every device, so the server
    /// sends one email even when several devices hold (and dispatch) the notification.
    pub request_id: String,
    pub notification_sync_id: String,
    pub recipient_user_sync_id: String,
    pub category_code: String,
    pub severity: String,
    pub title: String,
    pub body: Option<String>,
    pub action_url: Option<String>,
    pub occurred_at: String,
    pub source_module: String,
    pub source_record_id: Option<String>,
}

/// `202 Accepted` answer of `POST /api/v1/notifications/email`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NotificationEmailAccepted {
    pub request_id: String,
    /// `accepted` (queued for sending) or `duplicate` (this `request_id` was already accepted).
    pub status: String,
}
