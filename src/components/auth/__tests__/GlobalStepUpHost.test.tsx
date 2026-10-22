import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { GlobalStepUpHost } from "@/components/auth/GlobalStepUpHost";
import { requestGlobalStepUp } from "@/lib/step-up-bridge";
import { useSessionStore } from "@/store/session-store";
import { fixtures } from "@/test/mocks/tauri";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const EXTRA = { password_expires_in_days: null, pin_configured: false };
const signedIn = { ...fixtures.authenticatedSession, ...EXTRA };
const locked = { ...fixtures.lockedSession, ...EXTRA };

describe("GlobalStepUpHost", () => {
  beforeEach(() => {
    useSessionStore.getState().applySessionInfo(signedIn);
  });
  afterEach(() => useSessionStore.getState().resetForTests());

  it("drops a waiting command when the session locks", async () => {
    render(<GlobalStepUpHost />);
    const pending = requestGlobalStepUp();
    act(() => useSessionStore.getState().applySessionInfo(locked));
    await expect(pending).resolves.toBe(false);
  });

  it("drops a waiting command when another user signs in", async () => {
    render(<GlobalStepUpHost />);
    const pending = requestGlobalStepUp();
    act(() => useSessionStore.getState().applySessionInfo({ ...signedIn, user_id: 2 }));
    await expect(pending).resolves.toBe(false);
  });

  it("does not leave a waiting command hanging when the host unmounts", async () => {
    const { unmount } = render(<GlobalStepUpHost />);
    const pending = requestGlobalStepUp();
    unmount();
    await expect(pending).resolves.toBe(false);
  });
});
