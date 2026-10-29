// The typed enrolment code: 8 characters shown as XXXX-XXXX. The person may type it in lower case,
// with spaces or dashes, or paste it; the engine (and the server) validate the characters.

/** Number of characters of an enrolment code (without the dash). */
export const ENROLMENT_CODE_LENGTH = 8;

/** The code as the field shows it: upper case, only letters and digits, at most 8, dash after 4. */
export function formatEnrolmentCode(raw: string): string {
  const chars = raw
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, "")
    .slice(0, ENROLMENT_CODE_LENGTH);
  return chars.length > 4 ? `${chars.slice(0, 4)}-${chars.slice(4)}` : chars;
}

/** Whether all 8 characters are there. */
export function isCompleteEnrolmentCode(formatted: string): boolean {
  return formatted.replace("-", "").length === ENROLMENT_CODE_LENGTH;
}
