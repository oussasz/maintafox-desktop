/**
 * AU-106: the account lockout is shown in the app language with the local end time, never as the
 * server's English sentence with a UTC instant. Times are built from local dates, so the
 * assertions hold in any time zone.
 */
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { accountLockedErrorText, accountLockedText, lockoutUntil } from "@/lib/account-lockout";
import { appErrorLines, describeAppError } from "@/lib/app-error";
import { sessionErrorText } from "@/store/session-store";
import { formatDateTime, formatTime } from "@/utils/formatters";

/** 22 Oct 2026, 08:00 local: "now" for every case. */
const NOW = new Date(2026, 9, 22, 8, 0);
/** 09:15 local the same day, as the server sends it (UTC RFC 3339). */
const SAME_DAY = new Date(2026, 9, 22, 9, 15);
/** 09:15 local the next day. */
const NEXT_DAY = new Date(2026, 9, 23, 9, 15);

function lockout(until: string | null) {
  return {
    code: "ACCOUNT_LOCKED",
    message: `Account locked until ${until ?? ""}`,
    details: until === null ? null : [{ code: "ACCOUNT_LOCKED", message: "x", params: { until } }],
  };
}

describe("account lockout text", () => {
  beforeAll(() => {
    initI18n();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
  });
  afterAll(() => {
    vi.useRealTimers();
  });
  afterEach(async () => {
    await i18n.changeLanguage("fr");
  });

  it("reads the end the server sent with the refusal", () => {
    expect(lockoutUntil(lockout("2026-10-22T09:15:00Z"))).toBe("2026-10-22T09:15:00Z");
    expect(lockoutUntil(lockout(null))).toBeNull();
    expect(lockoutUntil({ code: "AUTH_ERROR", message: "x" })).toBeNull();
    expect(lockoutUntil(new Error("Account locked until 2026-10-22T09:15:00Z"))).toBeNull();
  });

  it("says the local time in French when the lockout ends today", async () => {
    await i18n.changeLanguage("fr");
    expect(accountLockedText(SAME_DAY.toISOString())).toBe(
      "Compte verrouillé après trop de mots de passe erronés, jusqu'à 09:15.",
    );
  });

  it("says the local time in English when the lockout ends today", async () => {
    await i18n.changeLanguage("en");
    const text = accountLockedText(SAME_DAY.toISOString());
    expect(text).toBe(
      `Account locked after too many wrong passwords, until ${formatTime(SAME_DAY, "en")}.`,
    );
    expect(text).toMatch(/until 09:15\sAM\.$/);
  });

  it("adds the date when the lockout ends another day", async () => {
    await i18n.changeLanguage("fr");
    expect(accountLockedText(NEXT_DAY.toISOString())).toBe(
      "Compte verrouillé après trop de mots de passe erronés, jusqu'au 23/10/2026 09:15.",
    );
    await i18n.changeLanguage("en");
    expect(accountLockedText(NEXT_DAY.toISOString())).toBe(
      `Account locked after too many wrong passwords, until ${formatDateTime(NEXT_DAY, "en")}.`,
    );
  });

  it("has no time when the end is missing or unreadable", async () => {
    await i18n.changeLanguage("fr");
    const none = "Compte verrouillé après trop de mots de passe erronés. Réessayez plus tard.";
    expect(accountLockedText(null)).toBe(none);
    expect(accountLockedText("demain")).toBe(none);
    expect(accountLockedErrorText(lockout(null))).toBe(none);
    await i18n.changeLanguage("en");
    expect(accountLockedText(undefined)).toBe(
      "Account locked after too many wrong passwords. Try again later.",
    );
  });

  it("is the one text of a lockout refusal everywhere, the server's sentence only technical", async () => {
    await i18n.changeLanguage("fr");
    const err = lockout(SAME_DAY.toISOString());
    const described = describeAppError(err);
    expect(described.title).toBe(
      "Compte verrouillé après trop de mots de passe erronés, jusqu'à 09:15.",
    );
    expect(described.description).toBeNull();
    expect(described.technical).toBe(err.message);
    expect(appErrorLines(err)).toEqual([described.title]);
  });

  it("follows a language switch for a lockout kept in the session", async () => {
    const state = {
      error: null,
      errorCode: "ACCOUNT_LOCKED",
      lockedUntil: SAME_DAY.toISOString(),
    };
    await i18n.changeLanguage("fr");
    expect(sessionErrorText(state)).toContain("jusqu'à 09:15");
    await i18n.changeLanguage("en");
    expect(sessionErrorText(state)).toMatch(
      /^Account locked after too many wrong passwords, until/,
    );
    expect(sessionErrorText({ error: "Autre", errorCode: "AUTH_ERROR", lockedUntil: null })).toBe(
      "Autre",
    );
  });
});
