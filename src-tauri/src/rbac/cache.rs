//! In-memory permission cache — Phase 2 SP06-S4 (GAP-02).
//!
//! Sits in `AppState` as `Arc<RwLock<PermissionCache>>`. Commands that
//! mutate RBAC data (`assign_role_scope`, `update_role`, …) invalidate
//! the relevant entries and emit a `rbac-changed` Tauri event so the
//! frontend `PermissionProvider` can refresh its own state.
//!
//! Safety-net: entries older than `max_age` are silently evicted on read.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Default max age for cache entries (seconds).
const DEFAULT_MAX_AGE_SECS: u64 = 120;

/// In-memory permission cache keyed by `(user_id, scope_key)`.
///
/// `scope_key` is a string like `"tenant"`, `"entity:42"`, `"site:7"`, or
/// `"node:15"` so that different scope resolutions are cached independently.
#[derive(Debug)]
pub struct PermissionCache {
    entries: HashMap<(i64, String), (HashSet<String>, Instant)>,
    max_age: Duration,
}

impl PermissionCache {
    /// Create a new cache with the given TTL (in seconds).
    pub fn new(max_age_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_age: Duration::from_secs(max_age_secs),
        }
    }

    /// Get cached permissions for `(user_id, scope_key)`.
    ///
    /// Returns `None` if:
    /// - no entry exists, or
    /// - the entry has expired (older than `max_age`).
    pub fn get(&self, user_id: i64, scope_key: &str) -> Option<&HashSet<String>> {
        let key = (user_id, scope_key.to_string());
        self.entries.get(&key).and_then(|(perms, ts)| {
            if ts.elapsed() < self.max_age {
                Some(perms)
            } else {
                None
            }
        })
    }

    /// Store permissions for a `(user_id, scope_key)` pair.
    pub fn put(&mut self, user_id: i64, scope_key: String, perms: HashSet<String>) {
        self.entries.insert((user_id, scope_key), (perms, Instant::now()));
    }

    /// Invalidate **all** entries for a given `user_id`.
    ///
    /// Called when a role assignment is changed for that user.
    pub fn invalidate_user(&mut self, user_id: i64) {
        self.entries.retain(|(uid, _), _| *uid != user_id);
    }

    /// Invalidate **all** entries regardless of user.
    ///
    /// Called when a role definition is modified (permissions added/removed)
    /// because any number of users may hold that role.
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }
}

impl Default for PermissionCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_AGE_SECS)
    }
}

/// Payload emitted on the `rbac-changed` Tauri event so both the Rust cache
/// and the frontend `PermissionProvider` know what changed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RbacChangedPayload {
    /// If `Some`, only that user's permissions were affected.
    /// If `None`, the change is global (e.g. role definition edited).
    pub affected_user_id: Option<i64>,
    /// Human-readable action label for debugging / logging.
    pub action: String,
}
