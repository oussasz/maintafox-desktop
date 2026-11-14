import { useEffect, useState } from "react";

import { getSessionInfo } from "@/services/auth-service";
import type { SessionInfo } from "@shared/ipc-types";

import { useMobileStartup } from "./hooks/use-startup";
import { useMobileT } from "./i18n";
import { type UserError, toUserError } from "./lib/user-error";
import { ActivationGate } from "./screens/ActivationGate";
import { DeviceSecurityGate } from "./screens/DeviceSecurityScreen";
import { FieldApp } from "./screens/FieldApp";
import { SignInScreen } from "./screens/SignInScreen";
import { StartupScreen } from "./screens/StartupScreen";
import { OnboardingScreen } from "./ui/OnboardingScreen";
import { Progress } from "./ui/kit";

type Session =
  | { kind: "checking" }
  | { kind: "signed_out" }
  | { kind: "signed_in"; info: SessionInfo };

/** Reads the session once the plant is joined: an engine session still open goes straight in. */
function SessionRoute() {
  const { t } = useMobileT();
  const [session, setSession] = useState<Session>({ kind: "checking" });
  const [error, setError] = useState<UserError | null>(null);

  useEffect(() => {
    let cancelled = false;
    getSessionInfo()
      .then((info) => {
        if (cancelled) return;
        setSession(
          info.is_authenticated && !info.is_locked
            ? { kind: "signed_in", info }
            : { kind: "signed_out" },
        );
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(toUserError(err, "session"));
        setSession({ kind: "signed_out" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (session.kind === "checking") {
    return (
      <OnboardingScreen title={t("signin.title")}>
        <Progress text={t("common.loading")} />
      </OnboardingScreen>
    );
  }
  if (session.kind === "signed_in") {
    return (
      <FieldApp session={session.info} onSignedOut={() => setSession({ kind: "signed_out" })} />
    );
  }
  return (
    <SignInScreen
      error={error}
      onSignedIn={(info) => {
        setError(null);
        setSession({ kind: "signed_in", info });
      }}
    />
  );
}

/** Engine startup, then joining the plant, then the session. */
function StartupRoute() {
  const { state, retryProbe } = useMobileStartup();

  if (state.status !== "ready") return <StartupScreen state={state} onRetry={retryProbe} />;
  return (
    <ActivationGate>
      <SessionRoute />
    </ActivationGate>
  );
}

/**
 * The mobile shell: one screen at a time, in a fixed order. The phone's screen-lock check first
 * (it needs no database, and without a lock the engine cannot open its database at all), then
 * engine startup, then joining the plant, then sign-in, then the field app.
 */
export function MobileApp() {
  return (
    <DeviceSecurityGate>
      <StartupRoute />
    </DeviceSecurityGate>
  );
}
