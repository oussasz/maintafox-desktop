/**
 * Presentation helpers shared by the supplier sourcing sections
 * (article sources list, comparison card, scorecard).
 */

import type { SupplierArticleSource } from "@shared/ipc-types";

export type RiskLevel = "LOW" | "MEDIUM" | "HIGH";

export const SUPPLIER_CONTACT_ROLES = ["PURCHASING", "SALES", "TECHNICAL"] as const;

export type SupplierContactRole = (typeof SUPPLIER_CONTACT_ROLES)[number];

const RISK_RANK: Record<RiskLevel, number> = { LOW: 0, MEDIUM: 1, HIGH: 2 };

/** Normalizes a backend risk string; unknown / missing values yield `null`. */
export function normalizeRiskLevel(value: string | null | undefined): RiskLevel | null {
  if (!value) return null;
  const upper = value.toUpperCase();
  return upper in RISK_RANK ? (upper as RiskLevel) : null;
}

export function riskBadgeVariant(
  value: string | null | undefined,
): "secondary" | "outline" | "destructive" {
  switch (normalizeRiskLevel(value)) {
    case "HIGH":
      return "destructive";
    case "MEDIUM":
      return "outline";
    default:
      return "secondary";
  }
}

/** Effective unit price used for comparison: actual last price, else the negotiated hint. */
export function effectiveUnitPrice(source: SupplierArticleSource): number | null {
  return source.last_price ?? source.unit_price_hint;
}

export interface SourceRating {
  sourceId: number;
  /** 1–5 stars; higher is better. */
  stars: number;
}

/**
 * Composite rating from the ranks of price, lead time and risk across the
 * candidate set. Sources missing a criterion rank last for that criterion, so a
 * fully-unknown supplier never outranks one with real data.
 *
 * Ranks are averaged then mapped onto 1–5 stars, best candidate always 5.
 */
export function computeCompositeRatings(sources: SupplierArticleSource[]): SourceRating[] {
  if (sources.length === 0) return [];

  const rankBy = (score: (s: SupplierArticleSource) => number | null): Map<number, number> => {
    const known = sources
      .map((s) => ({ id: s.id, value: score(s) }))
      .filter((entry): entry is { id: number; value: number } => entry.value != null)
      .sort((a, b) => a.value - b.value);

    const ranks = new Map<number, number>();
    known.forEach((entry, index) => ranks.set(entry.id, index));
    // Unknown values rank behind every known one.
    sources.forEach((s) => {
      if (!ranks.has(s.id)) ranks.set(s.id, known.length);
    });
    return ranks;
  };

  const priceRanks = rankBy(effectiveUnitPrice);
  const leadRanks = rankBy((s) => s.lead_time_days);
  const riskRanks = rankBy((s) => {
    const risk = normalizeRiskLevel(s.risk_level);
    return risk == null ? null : RISK_RANK[risk];
  });

  const scores = sources.map((s) => ({
    sourceId: s.id,
    score:
      ((priceRanks.get(s.id) ?? 0) + (leadRanks.get(s.id) ?? 0) + (riskRanks.get(s.id) ?? 0)) / 3,
  }));

  const best = Math.min(...scores.map((s) => s.score));
  const worst = Math.max(...scores.map((s) => s.score));
  const span = worst - best;

  return scores.map(({ sourceId, score }) => ({
    sourceId,
    // No spread between candidates → they are equally rated.
    stars: span === 0 ? 5 : Math.max(1, Math.round(5 - ((score - best) / span) * 4)),
  }));
}
