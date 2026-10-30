// What the technician reads when something fails: a plain sentence in their language, never the
// engine's or the server's technical English. The technical detail goes to the log (the Android
// WebView console is written to logcat), where support reads it.

import { lockoutUntil } from "@/lib/account-lockout";
import { toErrorMessage } from "@/utils/errors";

/** Translation key under `errors.` (each has a `title` and a `text`). */
export type UserErrorKind =
  | "serverUnreachable"
  | "serverError"
  | "badCredentials"
  | "accountLocked"
  | "sessionExpired"
  | "permissionDenied"
  | "accessScope"
  | "passwordChange"
  | "licenseNotFound"
  | "licenseRevoked"
  | "licenseExpired"
  | "licenseDenied"
  | "deviceRevoked"
  | "deviceLimit"
  | "mobileNotLicensed"
  | "tenantInactive"
  | "updateRequired"
  | "notFound"
  | "validation"
  | "databaseKey"
  | "enrolInputRequired"
  | "enrolQrInvalid"
  | "enrolOtherServer"
  | "enrolCodeInvalid"
  | "enrolDesktopRefused"
  | "enrolAlreadyActivated"
  | "enrolPhoneNotEmpty"
  | "enrolRemovalInProgress"
  | "enrolWipePending"
  | "enrolDeviceId"
  | "enrolAlreadyEnrolled"
  | "enrolRateLimited"
  | "enrolServerRefused"
  | "phoneAssigned"
  | "phoneNotEnrolled"
  | "phoneRemovalInProgress"
  | "phoneLocked"
  | "generic";

export interface UserError {
  kind: UserErrorKind;
  /** The end of an account lockout (UTC instant), shown in local time by the screen. */
  until?: string;
  /** The person a phone is assigned to (`phoneAssigned`), shown in the sentence. */
  person?: string;
}

/** Where the failure happened: the same engine code means different things at sign-in. */
export type ErrorContext = "signin" | "activation" | "general";

/** The engine's own network failures (sync/transport/client.rs) and the HTTP client's wording. */
const NETWORK_TEXT =
  /cannot reach the control api|control api (request|answer) interrupted|error sending request|connection (refused|reset)|operation timed out|failed to fetch/i;

/** Codes sent by the licence server (activation and policy refresh). */
const SERVER_CODES: Record<string, UserErrorKind> = {
  license_not_found: "licenseNotFound",
  license_revoked: "licenseRevoked",
  license_expired: "licenseExpired",
  activation_revoked: "deviceRevoked",
  slot_limit_reached: "deviceLimit",
  activation_conflict: "deviceLimit",
  mobile_not_licensed: "mobileNotLicensed",
  device_class_mismatch: "mobileNotLicensed",
  tenant_inactive: "tenantInactive",
  force_update_required: "updateRequired",
  offline: "serverUnreachable",
  interrupted: "serverUnreachable",
};

/** Codes of the engine's own refusals (`AppError::code`). */
const ENGINE_CODES: Record<string, UserErrorKind> = {
  ACCOUNT_LOCKED: "accountLocked",
  PERMISSION_DENIED: "permissionDenied",
  SESSION_LOCKED: "sessionExpired",
  SESSION_CLAIM_INVALID: "sessionExpired",
  STEP_UP_REQUIRED: "sessionExpired",
  TENANT_SCOPE_VIOLATION: "accessScope",
  PASSWORD_CHANGE_REQUIRED: "passwordChange",
  LICENSE_DENIED: "licenseDenied",
  NOT_FOUND: "notFound",
  VALIDATION_FAILED: "validation",
  DATABASE_KEY_UNAVAILABLE: "databaseKey",
};

/**
 * Enrolment refusals (`ENROLMENT_*`, see services/enrolment.ts) and sign-in refusals on a phone
 * (`PHONE_*`). The engine sends them as the `code` of the first entry of `details`.
 */
const ENROLMENT_CODES: Record<string, UserErrorKind> = {
  ENROLMENT_INPUT_REQUIRED: "enrolInputRequired",
  ENROLMENT_QR_INVALID: "enrolQrInvalid",
  ENROLMENT_QR_OTHER_SERVER: "enrolOtherServer",
  ENROLMENT_CODE_INVALID: "enrolCodeInvalid",
  ENROLMENT_INVALID: "enrolCodeInvalid",
  ENROLMENT_DESKTOP_REFUSED: "enrolDesktopRefused",
  ENROLMENT_ALREADY_ACTIVATED: "enrolAlreadyActivated",
  ENROLMENT_PHONE_NOT_EMPTY: "enrolPhoneNotEmpty",
  ENROLMENT_REMOVAL_IN_PROGRESS: "enrolRemovalInProgress",
  ENROLMENT_WIPE_PENDING: "enrolWipePending",
  ENROLMENT_DEVICE_ID_UNAVAILABLE: "enrolDeviceId",
  ENROLMENT_ALREADY_ENROLLED: "enrolAlreadyEnrolled",
  ENROLMENT_LICENCE_REVOKED: "licenseRevoked",
  ENROLMENT_LICENCE_EXPIRED: "licenseExpired",
  ENROLMENT_LICENCE_INVALID: "licenseDenied",
  ENROLMENT_NOT_LICENSED: "mobileNotLicensed",
  ENROLMENT_NO_FREE_SLOT: "deviceLimit",
  ENROLMENT_DEVICE_CLASS_MISMATCH: "mobileNotLicensed",
  ENROLMENT_RATE_LIMITED: "enrolRateLimited",
  ENROLMENT_OFFLINE: "serverUnreachable",
  ENROLMENT_SERVER_UNAVAILABLE: "serverError",
  ENROLMENT_SERVER_REFUSED: "enrolServerRefused",
  ENROLMENT_INVALID_ANSWER: "serverError",
  PHONE_ASSIGNED_TO_OTHER_PERSON: "phoneAssigned",
  PHONE_NOT_ENROLLED: "phoneNotEnrolled",
  PHONE_REMOVAL_IN_PROGRESS: "phoneRemovalInProgress",
  PHONE_LOCKED: "phoneLocked",
};

/** The first entry of the engine's `details` (its rule code and parameters), when there is one. */
function firstDetail(err: unknown): { code?: unknown; params?: unknown } | null {
  const details = field(err, "details");
  const first = Array.isArray(details) ? (details[0] as unknown) : null;
  return typeof first === "object" && first !== null ? first : null;
}

function field(err: unknown, name: string): unknown {
  return typeof err === "object" && err !== null && name in err
    ? (err as Record<string, unknown>)[name]
    : undefined;
}

/** The kind of a failure, from its code first and its text only for network failures. */
export function classifyError(err: unknown, context: ErrorContext = "general"): UserError {
  const code = field(err, "code");
  const status = field(err, "status");
  const text = toErrorMessage(err);

  const detail = firstDetail(err);
  const detailCode = typeof detail?.code === "string" ? detail.code : undefined;
  const enrolment = detailCode ? ENROLMENT_CODES[detailCode] : undefined;
  if (enrolment) {
    const params = detail?.params;
    const person =
      typeof params === "object" && params !== null && "person_label" in params
        ? (params as Record<string, unknown>)["person_label"]
        : undefined;
    return enrolment === "phoneAssigned" && typeof person === "string"
      ? { kind: enrolment, person }
      : { kind: enrolment };
  }
  if (typeof code === "string") {
    const direct = ENROLMENT_CODES[code];
    if (direct) return { kind: direct };
    if (code === "ACCOUNT_LOCKED") {
      const until = lockoutUntil(err);
      return until ? { kind: "accountLocked", until } : { kind: "accountLocked" };
    }
    if (code === "AUTH_ERROR") {
      return { kind: context === "signin" ? "badCredentials" : "sessionExpired" };
    }
    const kind = ENGINE_CODES[code] ?? SERVER_CODES[code];
    if (kind) return { kind };
  }
  if (NETWORK_TEXT.test(text)) return { kind: "serverUnreachable" };
  if (typeof status === "number") {
    if (status === 404 && context === "activation") return { kind: "licenseNotFound" };
    if (status >= 500) return { kind: "serverError" };
  }
  return { kind: "generic" };
}

/**
 * Classifies a failure and writes its technical detail to the log. `where` names the screen action
 * (e.g. "activation.preview") so a support reader can find it.
 */
export function toUserError(
  err: unknown,
  where: string,
  context: ErrorContext = "general",
): UserError {
  const classified = classifyError(err, context);
  const code = field(err, "code");
  console.error(
    `[maintafox-mobile] ${where} failed (${classified.kind})`,
    typeof code === "string" ? `code=${code}` : "",
    toErrorMessage(err),
  );
  return classified;
}

/** A licence refusal recorded by the engine (`deny_reason_code`), as a user error. */
export function licenceDenial(reasonCode: string | null | undefined): UserError {
  const kind = reasonCode ? SERVER_CODES[reasonCode] : undefined;
  return { kind: kind ?? "licenseDenied" };
}
