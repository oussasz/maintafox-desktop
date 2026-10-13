//! Protocol v2 transport: the Rust sync worker (Phase 2.3) and its HTTP client.
//!
//! See `docs/engineering/SYNC_STRATEGY.md` ("Transport") and
//! `maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md`.

pub mod client;
pub mod store;
pub mod wire;
pub mod worker;

#[cfg(test)]
mod tests;

pub use worker::{
    pause_sync, resume_sync, LicenseSyncAccess, SyncCommand, SyncTiming, SyncWorker, SyncWorkerConfig,
    SyncWorkerHandle, SyncWorkerStatus, SYNC_STATUS_EVENT, SYNC_TASK_ID,
};
