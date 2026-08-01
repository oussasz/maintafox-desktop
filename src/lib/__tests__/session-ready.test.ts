import { beforeEach, describe, expect, it } from "vitest";

import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import { useSessionStore, UNAUTHENTICATED_SESSION } from "@/store/session-store";
import { fixtures } from "@/test/mocks/tauri";
import type { SessionInfo } from "@shared/ipc-types";

function asSession(
  info: (typeof fixtures)["authenticatedSession"] | (typeof fixtures)["lockedSession"],
): SessionInfo {
  return {
    ...info,
    password_expires_in_days: null,
    pin_configured: null,
  };
}

describe("isSessionActiveForBackgroundWork", () => {
  beforeEach(() => {
    useSessionStore.getState().resetForTests();
  });

  it("returns false before bootstrap / with no session", () => {
    expect(isSessionActiveForBackgroundWork()).toBe(false);
    useSessionStore.setState({ info: UNAUTHENTICATED_SESSION, hasBootstrapped: true });
    expect(isSessionActiveForBackgroundWork()).toBe(false);
  });

  it("returns true only for authenticated unlocked sessions", () => {
    useSessionStore.setState({
      info: asSession(fixtures.authenticatedSession),
      hasBootstrapped: true,
    });
    expect(isSessionActiveForBackgroundWork()).toBe(true);

    useSessionStore.setState({
      info: asSession(fixtures.lockedSession),
      hasBootstrapped: true,
    });
    expect(isSessionActiveForBackgroundWork()).toBe(false);
  });
});
