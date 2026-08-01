/**
 * Port coordinates in IEC viewBox space for RAMS schematic symbols.
 * Handles use the same box as the SVG (strict pin alignment).
 */
export const IEC_VB = { w: 48, h: 40 } as const;
export const BE_VB = { w: 48, h: 48 } as const;

/** CSS size of the symbol box (must match RamsFlowNodes symbol wrapper). */
export const GATE_SYM_PX = { w: 72, h: 60 } as const;
export const BE_SYM_PX = { w: 72, h: 72 } as const;
export const RBD_AUX_SYM_PX = { w: 72, h: 56 } as const;

function pct(
  vbX: number,
  vbY: number,
  vb: { w: number; h: number } = IEC_VB,
): { left: string; top: string } {
  return {
    left: `${(100 * vbX) / vb.w}%`,
    top: `${(100 * vbY) / vb.h}%`,
  };
}

/**
 * AND D-gate: ins on flat back (~8,14)(~8,26), out at semicircle tip (~34,20).
 * Matches IconAndGate path (tip ~34,20).
 */
export function andHandlePct() {
  return {
    inTop: pct(8, 14),
    inBot: pct(8, 26),
    out: pct(34, 20),
  };
}

/** OR shield: ins (~10,12)(~10,28), out stem (~44,20). */
export function orHandlePct() {
  return {
    inTop: pct(10, 12),
    inBot: pct(10, 28),
    out: pct(44, 20),
  };
}

/** BE: single source at top of stem — pin (~24,4) in 48×48 viewBox. */
export function beHandlePct() {
  return {
    out: pct(24, 4, BE_VB),
  };
}

/** RBD series / parallel: in/out on horizontal spine. */
export function rbdAuxHandlePct(which: "series" | "parallel") {
  void which;
  return { in: pct(4, 20), out: pct(46, 20) };
}
