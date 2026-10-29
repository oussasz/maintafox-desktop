import { useEffect, useState } from "react";

import { getSessionInfo } from "@/services/auth-service";
import type { SessionInfo } from "@shared/ipc-types";

import { ErrorBox, Shell } from "./components/Shell";
import { useMobileStartup } from "./hooks/use-startup";
import { useMobileT } from "./i18n";
import { ActivationGate } from "./screens/ActivationGate";
import { DeviceSecurityGate } from "./screens/DeviceSecurityScreen";
import { HomeScreen } from "./screens/HomeScreen";
import { SignInScreen } from "./screens/SignInScreen";
import { StartupScreen } from "./screens/StartupScreen";

type Session =
  | { kind: "checking" }
  | { kind: "signed_out" }
  | { kind: "signed_in"; info: SessionInfo };

/** Reads the session once the plant is joined: an engine session still open goes straight home. */
function SessionRoute() {
  const { t } = useMobileT();
  const [session, setSession] = useState<Session>({ kind: "checking" });
  const [error, setError] = useState<string | null>(null);

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
        setError(err instanceof Error ? err.message : String(err));
        setSession({ kind: "signed_out" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (session.kind === "checking") {
    return (
      <Shell title={t("signin.title")}>
        <p role="status" className="mf-status">
          {t("common.loading")}
        </p>
      </Shell>
    );
  }
  if (session.kind === "signed_in") {
    return (
      <HomeScreen session={session.info} onSignedOut={() => setSession({ kind: "signed_out" })} />
    );
  }
  return (
    <>
      {error ? <ErrorBox title={t("common.errorTitle")} message={error} /> : null}
      <SignInScreen onSignedIn={(info) => setSession({ kind: "signed_in", info })} />
    </>
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
 * engine startup, then joining the plant, then sign-in, then home.
 */
export function MobileApp() {
  return (
    <DeviceSecurityGate>
      <StartupRoute />
    </DeviceSecurityGate>
  );
}
