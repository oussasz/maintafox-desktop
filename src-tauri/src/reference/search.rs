//! Alias-aware reference value search.
//!
//! Phase 2 - Sub-phase 03 - File 03 - Sprint S3.
//!
//! Provides `search_reference_values` with a five-tier ranking model:
//!
//!   1. Exact canonical code match
//!   2. Exact canonical label match
//!   3. Preferred alias match in the requested locale
//!   4. Other alias match in the requested locale
//!   5. Fallback alias matches in other locales
//!
//! Within each tier LIKE-prefix matches are ranked after exact matches.
//! Search is case-insensitive. Results are deduplicated by value id — only
//! the highest-ranked match per value is returned.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

/// A single search hit with ranking metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSearchHit {
    pub value_id: i64,
    pub code: String,
    pub label: String,
    pub matched_text: String,
    pub match_source: String,
    pub alias_type: Option<String>,
    pub rank: i64,
}

// ─── Internal ranking constants ───────────────────────────────────────────────

const RANK_EXACT_CODE: i64 = 100;
const RANK_PREFIX_CODE: i64 = 90;
const RANK_EXACT_LABEL: i64 = 80;
const RANK_PREFERRED_ALIAS_EXACT: i64 = 75;
const RANK_PREFIX_LABEL: i64 = 70;
const RANK_PREFERRED_ALIAS_PREFIX: i64 = 65;
const RANK_ALIAS_IN_LOCALE_EXACT: i64 = 60;
const RANK_ALIAS_IN_LOCALE_PREFIX: i64 = 55;
const RANK_ALIAS_FALLBACK_EXACT: i64 = 40;
const RANK_ALIAS_FALLBACK_PREFIX: i64 = 35;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Search reference values across canonical fields and aliases.
///
/// Returns at most `limit` hits, deduplicated by value id (highest rank wins).
/// The `domain_code` resolves the latest published set for that domain.
/// If no published set exists, the latest draft set is used as fallback.
///
/// `query` is matched case-insensitively against:
///   - value code  (exact → prefix)
///   - value label (exact → prefix)
///   - alias labels (exact → prefix), tiered by locale + preferred status
pub async fn search_reference_values(
    db: &impl ConnectionTrait,
    domain_code: &str,
    query: &str,
    locale: &str,
    limit: i64,
) -> AppResult<Vec<ReferenceSearchHit>> {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let effective_limit = limit.clamp(1, 200);

    // Resolve domain → latest published set (fallback to latest draft).
    let set_id = resolve_search_set(db, domain_code).await?;

    let query_upper = query_trimmed.to_ascii_uppercase();
    let query_lower = query_trimmed.to_lowercase();
    let query_like = format!("{}%", query_lower);

    // ── Phase 1: canonical code + label matches ───────────────────────────
    let canonical_hits = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label FROM reference_values \
             WHERE set_id = ? AND is_active = 1 \
               AND (UPPER(code) = ? OR code LIKE ? COLLATE NOCASE \
                    OR LOWER(label) = ? OR label LIKE ? COLLATE NOCASE)",
            [
                set_id.into(),
                query_upper.clone().into(),
                query_like.clone().into(),
                query_lower.clone().into(),
                query_like.clone().into(),
            ],
        ))
        .await?;

    // ── Phase 2: alias matches ────────────────────────────────────────────
    let alias_hits = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.reference_value_id, a.alias_label, a.locale, \
                    a.alias_type, a.is_preferred, \
                    v.code, v.label \
             FROM reference_aliases a \
             JOIN reference_values v ON v.id = a.reference_value_id \
             WHERE v.set_id = ? AND v.is_active = 1 \
               AND (LOWER(a.alias_label) = ? \
                    OR a.alias_label LIKE ? COLLATE NOCASE)",
            [
                set_id.into(),
                query_lower.clone().into(),
                query_like.clone().into(),
            ],
        ))
        .await?;

    // ── Rank and deduplicate ──────────────────────────────────────────────
    let mut best: std::collections::HashMap<i64, ReferenceSearchHit> =
        std::collections::HashMap::new();

    // Score canonical hits.
    for row in &canonical_hits {
        let id: i64 = row.try_get("", "id").unwrap_or(0);
        let code: String = row.try_get("", "code").unwrap_or_default();
        let label: String = row.try_get("", "label").unwrap_or_default();

        // Code match?
        if code.eq_ignore_ascii_case(query_trimmed) {
            maybe_insert(&mut best, id, &code, &label, &code, "canonical_code", None, RANK_EXACT_CODE);
        } else if code.to_ascii_uppercase().starts_with(&query_upper) {
            maybe_insert(&mut best, id, &code, &label, &code, "canonical_code", None, RANK_PREFIX_CODE);
        }

        // Label match?
        if label.eq_ignore_ascii_case(query_trimmed) {
            maybe_insert(&mut best, id, &code, &label, &label, "canonical_label", None, RANK_EXACT_LABEL);
        } else if label.to_lowercase().starts_with(&query_lower) {
            maybe_insert(&mut best, id, &code, &label, &label, "canonical_label", None, RANK_PREFIX_LABEL);
        }
    }

    // Score alias hits.
    for row in &alias_hits {
        let value_id: i64 = row.try_get("", "reference_value_id").unwrap_or(0);
        let alias_label: String = row.try_get("", "alias_label").unwrap_or_default();
        let alias_locale: String = row.try_get("", "locale").unwrap_or_default();
        let alias_type: String = row.try_get("", "alias_type").unwrap_or_default();
        let is_preferred: bool = row
            .try_get::<i32>("", "is_preferred")
            .map(|v| v != 0)
            .unwrap_or(false);
        let code: String = row.try_get("", "code").unwrap_or_default();
        let label: String = row.try_get("", "label").unwrap_or_default();

        let is_exact = alias_label.eq_ignore_ascii_case(query_trimmed);
        let in_locale = alias_locale.eq_ignore_ascii_case(locale);

        let rank = match (in_locale, is_preferred, is_exact) {
            // Tier 3: preferred alias in locale
            (true, true, true) => RANK_PREFERRED_ALIAS_EXACT,
            (true, true, false) => RANK_PREFERRED_ALIAS_PREFIX,
            // Tier 4: other alias in locale
            (true, false, true) => RANK_ALIAS_IN_LOCALE_EXACT,
            (true, false, false) => RANK_ALIAS_IN_LOCALE_PREFIX,
            // Tier 5: fallback alias in other locales
            (false, _, true) => RANK_ALIAS_FALLBACK_EXACT,
            (false, _, false) => RANK_ALIAS_FALLBACK_PREFIX,
        };

        maybe_insert(
            &mut best,
            value_id,
            &code,
            &label,
            &alias_label,
            "alias",
            Some(alias_type),
            rank,
        );
    }

    // Sort by rank descending, then by code for stability.
    let mut results: Vec<ReferenceSearchHit> = best.into_values().collect();
    results.sort_by(|a, b| b.rank.cmp(&a.rank).then_with(|| a.code.cmp(&b.code)));
    results.truncate(effective_limit as usize);

    Ok(results)
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Insert or upgrade a hit in the dedup map if the new rank is higher.
fn maybe_insert(
    best: &mut std::collections::HashMap<i64, ReferenceSearchHit>,
    value_id: i64,
    code: &str,
    label: &str,
    matched_text: &str,
    match_source: &str,
    alias_type: Option<String>,
    rank: i64,
) {
    let entry = best.entry(value_id);
    match entry {
        std::collections::hash_map::Entry::Vacant(e) => {
            e.insert(ReferenceSearchHit {
                value_id,
                code: code.to_string(),
                label: label.to_string(),
                matched_text: matched_text.to_string(),
                match_source: match_source.to_string(),
                alias_type,
                rank,
            });
        }
        std::collections::hash_map::Entry::Occupied(mut e) => {
            if rank > e.get().rank {
                let hit = e.get_mut();
                hit.matched_text = matched_text.to_string();
                hit.match_source = match_source.to_string();
                hit.alias_type = alias_type;
                hit.rank = rank;
            }
        }
    }
}

/// Resolve domain code to the best set id for searching.
/// Prefers the latest published set; falls back to the latest draft.
async fn resolve_search_set(
    db: &impl ConnectionTrait,
    domain_code: &str,
) -> AppResult<i64> {
    // Resolve domain id from code.
    let domain_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE code = ?",
            [domain_code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceDomain".into(),
            id: domain_code.to_string(),
        })?;

    let domain_id: i64 = domain_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("decode domain id: {e}")))?;

    // Try published first.
    if let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'published' \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?
    {
        let set_id: i64 = row
            .try_get("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("decode set id: {e}")))?;
        return Ok(set_id);
    }

    // Fallback to draft.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_sets \
             WHERE domain_id = ? AND status = 'draft' \
             ORDER BY version_no DESC LIMIT 1",
            [domain_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReferenceSet".into(),
            id: format!("domain:{domain_code}"),
        })?;

    let set_id: i64 = row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("decode set id: {e}")))?;

    Ok(set_id)
}
