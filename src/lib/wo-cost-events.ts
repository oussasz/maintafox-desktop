/**
 * "The cost of this work order changed": labour or parts were booked, closed or marked unused.
 * Execution calls announce it; the cost panel listens and re-reads the figures, so the costs
 * shown never lag behind what was just recorded (they used to reload only on a status change).
 */
const EVENT = "mf:wo-cost-changed";

export function notifyWoCostChanged(woId: number): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent<number>(EVENT, { detail: woId }));
}

/** Calls `onChange` when the cost of `woId` changes; returns the unsubscribe function. */
export function onWoCostChanged(woId: number, onChange: () => void): () => void {
  const handler = (e: Event) => {
    if ((e as CustomEvent<number>).detail === woId) onChange();
  };
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
