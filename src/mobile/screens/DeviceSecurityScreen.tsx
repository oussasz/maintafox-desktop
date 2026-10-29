import { Lock, ShieldAlert } from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";

import { useMobileT } from "../i18n";
import { type UserError, toUserError } from "../lib/user-error";
import { getMobileDeviceSecurity, isScreenLockRefused } from "../services/device-security";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Button, Hero, Progress } from "../ui/kit";

type Check =
  | { kind: "checking" }
  | { kind: "ok" }
  | { kind: "refused" }
  | { kind: "error"; error: UserError };

/**
 * Runs the phone's security check before anything else may happen. A phone without a screen lock
 * gets a full-screen refusal with no way forward: only a new check. `children` render once it passes.
 */
export function DeviceSecurityGate({ children }: { children: ReactNode }) {
  const { t } = useMobileT();
  const [check, setCheck] = useState<Check>({ kind: "checking" });

  const run = useCallback(async () => {
    setCheck({ kind: "checking" });
    try {
      const security = await getMobileDeviceSecurity();
      setCheck(isScreenLockRefused(security) ? { kind: "refused" } : { kind: "ok" });
    } catch (err) {
      setCheck({ kind: "error", error: toUserError(err, "device_security") });
    }
  }, []);

  useEffect(() => {
    void run();
  }, [run]);

  if (check.kind === "ok") return <>{children}</>;

  if (check.kind === "refused") {
    return (
      <OnboardingScreen title={t("security.title")}>
        <div role="alert">
          <Hero
            icon={<Lock size={40} strokeWidth={2.2} aria-hidden />}
            tone="red"
            title={t("security.title")}
            text={t("security.body")}
          />
        </div>
        <Actions>
          <Button onClick={() => void run()}>{t("security.recheck")}</Button>
        </Actions>
      </OnboardingScreen>
    );
  }

  if (check.kind === "error") {
    return (
      <OnboardingScreen title={t("security.errorTitle")}>
        <Hero
          icon={<ShieldAlert size={40} strokeWidth={2.2} aria-hidden />}
          tone="red"
          title={t("security.errorTitle")}
        />
        <ErrorBanner error={check.error} onRetry={() => void run()} />
      </OnboardingScreen>
    );
  }

  return (
    <OnboardingScreen title={t("security.title")}>
      <Progress text={t("security.checking")} />
    </OnboardingScreen>
  );
}
