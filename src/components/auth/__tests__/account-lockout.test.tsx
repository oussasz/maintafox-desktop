/**
 * AU-37: the unlock and step-up passwords are limited by the server's account lockout. The
 * step-up dialog has no attempt count of its own; an `ACCOUNT_LOCKED` refusal (the server ended
 * the session) sends the user to sign-in with the lockout message.
 * AU-106: that message is in the app language with the local end time, and the session keeps the
 * code and the end (not the server's English sentence), so it follows a language switch.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { StepUpDialog } from "@/components/auth/StepUpDialog";
import { i18n, initI18n } from "@/i18n";
import { sessionErrorText, useSessionStore } from "@/store/session-store";
import type { SessionInfo } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  // The app's i18n instance (lockout text) still initialises; the dialog's own keys stay raw.
  initReactI18next: { type: "3rdParty", init: () => undefined },
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", resolvedLanguage: "fr" },
  }),
}));

const svc = vi.hoisted(() => ({
  verifyStepUp: vi.fn(),
  unlockSession: vi.fn(),
}));

vi.mock("@/services/rbac-service", () => ({ verifyStepUp: svc.verifyStepUp }));
vi.mock("@/services/auth-service", () => ({
  unlockSession: svc.unlockSession,
  getSessionInfo: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
  forceChangePassword: vi.fn(),
}));

/** "Now" is 08:00 local and the lockout ends at 09:15 local the same day, in any time zone. */
const NOW = new Date(2026, 9, 22, 8, 0);
const UNTIL = new Date(2026, 9, 22, 9, 15).toISOString();
const LOCKOUT = {
  code: "ACCOUNT_LOCKED",
  message: `Account locked until ${UNTIL}`,
  details: [
    { code: "ACCOUNT_LOCKED", message: `Account locked until ${UNTIL}`, params: { until: UNTIL } },
  ],
};
const LOCKOUT_FR = "Compte verrouillé après trop de mots de passe erronés, jusqu'à 09:15.";
const WRONG = { code: "AUTH_ERROR", message: "Mot de passe incorrect." };

function openSession(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    is_authenticated: true,
    is_locked: false,
    user_id: 7,
    username: "tech",
    display_name: "Technicien",
    is_admin: false,
    force_password_change: false,
    expires_at: null,
    last_activity_at: null,
    idle_timeout_minutes: 30,
    password_expires_in_days: null,
    pin_configured: false,
    ...overrides,
  };
}

function submit(password: string) {
  fireEvent.change(screen.getByLabelText("stepUp.passwordLabel"), {
    target: { value: password },
  });
  fireEvent.click(screen.getByRole("button", { name: "stepUp.submit" }));
}

describe("step-up dialog and the account lockout", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
  });
  afterAll(() => {
    vi.useRealTimers();
  });

  beforeEach(() => {
    svc.verifyStepUp.mockReset();
    svc.unlockSession.mockReset();
    useSessionStore.getState().resetForTests();
    useSessionStore.getState().applySessionInfo(openSession());
  });

  it("has no attempt limit of its own: wrong passwords are refused by the server only", async () => {
    svc.verifyStepUp.mockRejectedValue(WRONG);
    render(<StepUpDialog open onVerified={vi.fn()} onCancel={vi.fn()} />);
    for (let i = 0; i < 5; i += 1) {
      submit("wrong");
      await waitFor(() => expect(svc.verifyStepUp).toHaveBeenCalledTimes(i + 1));
      await screen.findByText("stepUp.error");
      await waitFor(() => expect(screen.getByLabelText("stepUp.passwordLabel")).toBeEnabled());
    }
    expect(useSessionStore.getState().info?.is_authenticated).toBe(true);
  });

  it("shows the server's lockout and sends the user to sign-in with it", async () => {
    svc.verifyStepUp.mockRejectedValue(LOCKOUT);
    const onCancel = vi.fn();
    render(<StepUpDialog open onVerified={vi.fn()} onCancel={onCancel} />);
    submit("wrong");

    await screen.findByText(LOCKOUT_FR);
    expect(screen.queryByText(LOCKOUT.message)).toBeNull();
    expect(onCancel).toHaveBeenCalled();
    const state = useSessionStore.getState();
    expect(state.info?.is_authenticated).toBe(false);
    expect(state.errorCode).toBe("ACCOUNT_LOCKED");
    expect(state.lockedUntil).toBe(UNTIL);
    expect(state.error).toBeNull();
    expect(sessionErrorText(state)).toBe(LOCKOUT_FR);
  });

  it("the lock screen's unlock ends the session on a lockout, with the sign-in message", async () => {
    useSessionStore
      .getState()
      .applySessionInfo(openSession({ is_authenticated: false, is_locked: true }));
    svc.unlockSession.mockRejectedValue(LOCKOUT);

    await expect(useSessionStore.getState().unlock("wrong")).rejects.toThrow(LOCKOUT_FR);
    const state = useSessionStore.getState();
    expect(state.info?.is_locked).toBe(false);
    expect(state.info?.user_id).toBeNull();
    expect(state.errorCode).toBe("ACCOUNT_LOCKED");
    expect(state.lockedUntil).toBe(UNTIL);
    expect(sessionErrorText(state)).toBe(LOCKOUT_FR);
  });

  it("a sign-in refused by the lockout keeps the end and shows it in the app language", async () => {
    useSessionStore.getState().applySessionInfo(openSession({ is_authenticated: false }));
    const { login } = await import("@/services/auth-service");
    vi.mocked(login).mockRejectedValueOnce(LOCKOUT);

    await expect(
      useSessionStore.getState().login({ username: "tech", password: "wrong" }),
    ).rejects.toEqual(LOCKOUT);
    const state = useSessionStore.getState();
    expect(state.errorCode).toBe("ACCOUNT_LOCKED");
    expect(state.lockedUntil).toBe(UNTIL);
    expect(state.error).toBeNull();
    expect(sessionErrorText(state)).toBe(LOCKOUT_FR);
    await i18n.changeLanguage("en");
    try {
      expect(sessionErrorText(useSessionStore.getState())).toMatch(
        /^Account locked after too many wrong passwords, until 09:15\sAM\.$/,
      );
    } finally {
      await i18n.changeLanguage("fr");
    }
  });

  it("a wrong unlock password keeps the lock screen", async () => {
    useSessionStore
      .getState()
      .applySessionInfo(openSession({ is_authenticated: false, is_locked: true }));
    svc.unlockSession.mockRejectedValue(WRONG);

    await expect(useSessionStore.getState().unlock("wrong")).rejects.toThrow(WRONG.message);
    const state = useSessionStore.getState();
    expect(state.info?.is_locked).toBe(true);
    expect(state.errorCode).toBeNull();
  });
});
