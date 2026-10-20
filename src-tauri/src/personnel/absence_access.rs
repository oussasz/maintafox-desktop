//! Who may read a person's absence TYPE (decision register 2026-10-19, Q2, and the CMMS Advisor's
//! decision (b)): the type — its code, label, reason and colour — is sensitive personal data. It
//! is sent only to holders of the sensitive-data right `per.sensitiveview` (plant-wide, with
//! `per.view`, the personnel module entitled and licensed for them), and to the person on their own
//! record. Everyone else receives availability only: when the person is away and whether it is
//! critical for planning (the type's `critical` tag in Données de référence, or the block marked
//! critical) — shown as the neutral tag "Absent" / "Absent — critical for planning".
//!
//! ONE decision for every screen that shows absences: the planning board, the availability block
//! list and calendar, the availability state and the person's capacity days. The fields are
//! removed from the payload on the server; the screens never receive them. Any failure while
//! deciding falls back to the neutral tag (fail closed), never to an error that hides the screen.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::RwLock;

use crate::auth::rbac::{check_permission_cached, PermissionScope};
use crate::errors::AppResult;
use crate::personnel::availability::{AvailabilityCalendarEntry, PersonnelAvailabilityBlock};
use crate::personnel::capacity::PersonDayCapacity;
use crate::personnel::domain::PersonnelAvailabilityState;
use crate::personnel::reference_lists::{ABSENCE_TAG_CRITICAL, BLOCK_TAG_SQL};
use crate::rbac::cache::PermissionCache;
use crate::rbac::permissions::{PER_SENSITIVEVIEW, PER_VIEW};

/// What a caller may read of one person's absences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsenceDetail {
    /// The absence type (code, the screen names and colours it from the plant's list) and reason.
    Full,
    /// Only when the person is away and whether it is critical for planning.
    AvailabilityOnly,
}

/// The caller's access to absence types, loaded once per request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsenceAccess {
    /// `per.view` and `per.sensitiveview` plant-wide, and the personnel module entitled and
    /// licensed for this user.
    sensitive: bool,
    /// The personnel record linked to the caller's account (their own absences stay readable).
    own_personnel_id: Option<i64>,
}

impl AbsenceAccess {
    /// Decides the caller's access. Never fails: a refusal or an error on any check gives the
    /// neutral tag (logged), so the screens keep working without the types.
    pub async fn load(db: &DatabaseConnection, cache: &RwLock<PermissionCache>, user_id: i32) -> Self {
        let sensitive = match sensitive_right(db, cache, user_id).await {
            Ok(granted) => granted,
            Err(err) => {
                tracing::warn!(user_id, error = %err, "absence type access check failed; neutral tag only");
                false
            }
        };
        let own_personnel_id = if sensitive {
            None
        } else {
            match linked_personnel_id(db, user_id).await {
                Ok(id) => id,
                Err(err) => {
                    tracing::warn!(user_id, error = %err, "linked personnel lookup failed; neutral tag only");
                    None
                }
            }
        };
        Self {
            sensitive,
            own_personnel_id,
        }
    }

    /// No absence type for anyone (internal readers that never show absences, e.g. the PDF).
    pub fn neutral() -> Self {
        Self {
            sensitive: false,
            own_personnel_id: None,
        }
    }

    /// Full access to every record (tests of the full payload).
    #[cfg(test)]
    pub(crate) fn full_for_tests() -> Self {
        Self {
            sensitive: true,
            own_personnel_id: None,
        }
    }

    pub fn detail_for(&self, personnel_id: i64) -> AbsenceDetail {
        if self.sensitive || self.own_personnel_id == Some(personnel_id) {
            AbsenceDetail::Full
        } else {
            AbsenceDetail::AvailabilityOnly
        }
    }

    /// Blocks as returned by the availability screens: `critical_for_planning` set on each; for a
    /// person the caller may not read, the type and reason removed (`type_masked`, `reason_masked`).
    pub async fn apply_to_blocks(
        &self,
        db: &impl ConnectionTrait,
        blocks: &mut [PersonnelAvailabilityBlock],
    ) -> AppResult<()> {
        let critical_types = critical_tags(db, blocks.iter().map(|b| b.id)).await?;
        for block in blocks.iter_mut() {
            block.critical_for_planning = block.is_critical || critical_types.get(&block.id).copied().unwrap_or(false);
            if self.detail_for(block.personnel_id) == AbsenceDetail::AvailabilityOnly {
                block.block_type = String::new();
                block.type_masked = true;
                block.mask_sensitive();
            }
        }
        Ok(())
    }

    /// Calendar days: the types of the day removed for people the caller may not read.
    pub fn apply_to_calendar(&self, entries: &mut [AvailabilityCalendarEntry]) {
        for entry in entries.iter_mut() {
            if self.detail_for(entry.personnel_id) == AbsenceDetail::AvailabilityOnly {
                entry.block_types.clear();
            }
        }
    }

    /// A person's capacity days: the absence types of each day removed unless readable.
    pub fn apply_to_person_capacity(&self, personnel_id: i64, days: &mut [PersonDayCapacity]) {
        if self.detail_for(personnel_id) == AbsenceDetail::AvailabilityOnly {
            for day in days.iter_mut() {
                day.absence_types.clear();
            }
        }
    }

    /// The availability state: the active blocks' type codes removed from its reasons unless
    /// readable (the program's own reason codes stay).
    pub fn apply_to_availability_state(&self, state: &mut PersonnelAvailabilityState) {
        if self.detail_for(state.personnel_id) == AbsenceDetail::AvailabilityOnly {
            state
                .reasons
                .retain(|r| crate::personnel::availability::PROGRAM_REASON_CODES.contains(&r.as_str()));
        }
    }
}

async fn sensitive_right(db: &DatabaseConnection, cache: &RwLock<PermissionCache>, user_id: i32) -> AppResult<bool> {
    for permission in [PER_VIEW, PER_SENSITIVEVIEW] {
        if !check_permission_cached(db, cache, user_id, permission, &PermissionScope::Global).await? {
            return Ok(false);
        }
    }
    // The module checks the personnel screens run (`require_permission!`): a refusal downgrades to
    // the neutral tag.
    if let Err(err) = crate::entitlements::queries::enforce_capability_for_permission(db, PER_SENSITIVEVIEW).await {
        tracing::info!(user_id, error = %err, "personnel module not entitled; absence types withheld");
        return Ok(false);
    }
    if let Err(err) = crate::license::queries::enforce_permission_matrix(db, user_id, PER_SENSITIVEVIEW).await {
        tracing::info!(user_id, error = %err, "personnel sensitive data not licensed; absence types withheld");
        return Ok(false);
    }
    Ok(true)
}

async fn linked_personnel_id(db: &DatabaseConnection, user_id: i32) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => r.try_get::<Option<i64>>("", "personnel_id")?,
        None => None,
    })
}

/// Block id → whether its absence type carries the `critical` tag in Données de référence.
async fn critical_tags(db: &impl ConnectionTrait, ids: impl Iterator<Item = i64>) -> AppResult<HashMap<i64, bool>> {
    let ids: Vec<i64> = ids.collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT b.id, {BLOCK_TAG_SQL} AS tag FROM personnel_availability_blocks b WHERE b.id IN ({placeholders})"),
            ids.iter().map(|id| (*id).into()).collect::<Vec<sea_orm::Value>>(),
        ))
        .await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row.try_get("", "id")?;
        let tag: Option<String> = row.try_get("", "tag")?;
        out.insert(id, tag.as_deref() == Some(ABSENCE_TAG_CRITICAL));
    }
    Ok(out)
}
