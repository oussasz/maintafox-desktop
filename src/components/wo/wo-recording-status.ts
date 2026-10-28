/**
 * Work-order statuses in which execution records are entered: task results (and their NOK
 * evidence), task reopening, tool use and labour. Same set as the backend
 * (`wo::tasks::TASK_RECORDING_STATUSES`, CMMS Advisor O3); recording during a hold never resumes
 * the work order. Parts, downtime and status actions keep their own rules.
 */
export const RECORDING_STATUSES: ReadonlySet<string> = new Set(["in_progress", "on_hold"]);

/** Whether execution records can be entered on a work order in `statusCode`. */
export function isRecordingStatus(statusCode: string | null | undefined): boolean {
  return statusCode != null && RECORDING_STATUSES.has(statusCode);
}
