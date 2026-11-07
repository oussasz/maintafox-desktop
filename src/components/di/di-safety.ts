/**
 * Least urgent level of a request flagged "sécurité" (`di::domain::SAFETY_MIN_URGENCY`, CMMS
 * Advisor decision on L4-11, 2026-11-06). The server raises a lower reported urgency to it; the
 * request form chooses it and offers only it or a more urgent level; a reviewer may validate a
 * lower urgency only with the reason of the review, which the server audits.
 */
export const SAFETY_MIN_URGENCY = "urgent";
