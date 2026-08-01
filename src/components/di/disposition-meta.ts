/** Governed DI close disposition codes (excludes convert-only `converted_to_wo`). */
export const DI_DISPOSITION_FILTER_CODES = [
  "rejected_invalid",
  "duplicate",
  "cancelled_by_requester",
  "cancelled_by_planner",
  "no_work_required",
  "solved_immediately",
  "information_only",
  "converted_to_wo",
  "other",
] as const;

export type DiDispositionCode = (typeof DI_DISPOSITION_FILTER_CODES)[number];

export function dispositionLabelKey(code: string | null | undefined): string | null {
  if (!code?.trim()) return null;
  return `disposition.${code}`;
}
