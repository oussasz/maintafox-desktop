/**
 * Display-only engineering metrics for RAMS Visual Lab nodes.
 * Values are derived from persisted P / R where the backend stores only probability or reliability.
 */

const DEFAULT_REF_MISSION_H = 8760; // 1 year — reference for λ display when no explicit τ

function clamp01(x: number): number {
  if (Number.isNaN(x)) return 0;
  return Math.min(1, Math.max(0, x));
}

/** Exponential approximation: λ ≈ −ln(1−p) / τ for rare-event failure probability over interval τ. */
export function failureRatePerHourFromP(p: number, tauHours: number): number {
  const pp = clamp01(p);
  const tau = tauHours > 0 ? tauHours : DEFAULT_REF_MISSION_H;
  if (pp >= 1) return Number.POSITIVE_INFINITY;
  if (pp <= 0) return 0;
  return -Math.log(1 - pp) / tau;
}

export function formatSci(value: number, digits = 3): string {
  if (!Number.isFinite(value)) return "—";
  if (value === 0) return "0";
  const a = Math.abs(value);
  if (a >= 1e-2 && a < 1e6) return value.toFixed(digits);
  return value.toExponential(Math.min(3, digits));
}

export function ftaBasicEventDisplay(p: number, tauHours = DEFAULT_REF_MISSION_H) {
  const pp = clamp01(p);
  const lambda = failureRatePerHourFromP(pp, tauHours);
  return {
    p: pp,
    u: pp,
    lambdaPerH: lambda,
    tauH: tauHours,
    mttrH: null as number | null,
  };
}

export function rbdBlockDisplay(r: number, tauHours = DEFAULT_REF_MISSION_H) {
  const rr = clamp01(r);
  const u = 1 - rr;
  const lambda = failureRatePerHourFromP(u, tauHours);
  return {
    r: rr,
    u,
    lambdaPerH: lambda,
    tauH: tauHours,
    mttrH: null as number | null,
  };
}
