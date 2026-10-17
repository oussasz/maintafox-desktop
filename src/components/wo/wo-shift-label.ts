import type { WorkOrder } from "@shared/ipc-types";

/**
 * The shift a work order is planned on, for display: the Classe horaire window it was planned with
 * ("Poste jour 06:00–14:00"), else the old fixed code of a work order planned before those shifts,
 * else null. `t` translates in the `ot` namespace.
 */
export function formatWoShift(
  wo: Pick<WorkOrder, "shift" | "shift_class_label" | "shift_start" | "shift_end">,
  t: (key: string) => string,
): string | null {
  if (wo.shift_start && wo.shift_end) {
    const window = `${wo.shift_start}–${wo.shift_end}`;
    return wo.shift_class_label ? `${wo.shift_class_label} ${window}` : window;
  }
  return wo.shift ? t(`shift.${wo.shift}`) : null;
}
