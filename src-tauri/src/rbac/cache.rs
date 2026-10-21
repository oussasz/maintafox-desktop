//! In-memory permission cache — Phase 2 SP06-S4 (GAP-02).
//!
//! Sits in `AppState` as `Arc<RwLock<PermissionCache>>`. Commands that
//! mutate RBAC data (`assign_role_scope`, `update_role`, …) invalidate
//! the relevant entries and emit a `rbac-changed` Tauri event so the
//! frontend `PermissionProvider` can refresh its own state.
//!
//! Safety-net: entries older than `max_age` are silently evicted on read.
//! An entry also ends at the next instant one of the user's assignments starts, ends or expires
//! (`valid_until`): an expired emergency grant stops counting at its expiry, without a sweep.

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
    entries: HashMap<(i64, String), CachedSet>,
    /// Site-aware access per user (`auth::scope`), under the same freshness rules.
    access: HashMap<i64, CachedAccess>,
    max_age: Duration,
}

impl PermissionCache {
    /// Create a new cache with the given TTL (in seconds).
    pub fn new(max_age_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            access: HashMap::new(),
            max_age: Duration::from_secs(max_age_secs),
        }
    }

    /// The user's site-aware access map, while fresh (same rules as [`Self::get`]).
    pub fn get_access(&self, user_id: i64) -> Option<&crate::auth::scope::AccessMap> {
        let now = chrono::Utc::now();
        self.access.get(&user_id).and_then(|entry| {
            let fresh = entry.stored_at.elapsed() < self.max_age && entry.valid_until.is_none_or(|until| now < until);
            fresh.then_some(&entry.map)
        })
    }

    /// Store the user's site-aware access map until `valid_until` (next assignment change).
    pub fn put_access(
        &mut self,
        user_id: i64,
        map: crate::auth::scope::AccessMap,
        valid_until: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        self.access.insert(
            user_id,
            CachedAccess {
                map,
                stored_at: Instant::now(),
                valid_until,
            },
        );
    }

    /// Get cached permissions for `(user_id, scope_key)`.
    ///
    /// Returns `None` if:
    /// - no entry exists, or
    /// - the entry has expired (older than `max_age`), or
    /// - one of the user's assignments started, ended or expired since it was computed.
    pub fn get(&self, user_id: i64, scope_key: &str) -> Option<&HashSet<String>> {
        let key = (user_id, scope_key.to_string());
        let now = chrono::Utc::now();
        self.entries.get(&key).and_then(|entry| {
            let fresh = entry.stored_at.elapsed() < self.max_age && entry.valid_until.is_none_or(|until| now < until);
            fresh.then_some(&entry.perms)
        })
    }

    /// Store permissions for a `(user_id, scope_key)` pair, valid until `valid_until` (the next
    /// change of the user's assignments; `None` when none is scheduled).
    pub fn put(
        &mut self,
        user_id: i64,
        scope_key: String,
        perms: HashSet<String>,
        valid_until: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        self.entries.insert(
            (user_id, scope_key),
            CachedSet {
                perms,
                stored_at: Instant::now(),
                valid_until,
            },
        );
    }

    /// Invalidate **all** entries for a given `user_id`.
    ///
    /// Called when a role assignment is changed for that user.
    pub fn invalidate_user(&mut self, user_id: i64) {
        self.entries.retain(|(uid, _), _| *uid != user_id);
        self.access.remove(&user_id);
    }

    /// Invalidate **all** entries regardless of user.
    ///
    /// Called when a role definition is modified (permissions added/removed)
    /// because any number of users may hold that role.
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.access.clear();
    }
}

#[derive(Debug)]
struct CachedAccess {
    map: crate::auth::scope::AccessMap,
    stored_at: Instant,
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug)]
struct CachedSet {
    perms: HashSet<String>,
    stored_at: Instant,
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_ends_at_the_next_assignment_change() {
        let mut cache = PermissionCache::default();
        let perms: HashSet<String> = [crate::rbac::permissions::ADM_USERS.to_string()].into();
        cache.put(1, "tenant".into(), perms.clone(), None);
        assert!(cache.get(1, "tenant").is_some());

        let past = chrono::Utc::now() - chrono::Duration::seconds(1);
        cache.put(1, "tenant".into(), perms.clone(), Some(past));
        assert!(
            cache.get(1, "tenant").is_none(),
            "an expired grant is not served from cache"
        );

        let future = chrono::Utc::now() + chrono::Duration::minutes(5);
        cache.put(1, "tenant".into(), perms, Some(future));
        assert!(cache.get(1, "tenant").is_some());
    }
}
