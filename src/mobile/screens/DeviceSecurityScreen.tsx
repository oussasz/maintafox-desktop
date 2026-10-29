import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";

import { toErrorMessage } from "@/utils/errors";

import { ErrorBox, Shell } from "../components/Shell";
import { useMobileT } from "../i18n";
import { getMobileDeviceSecurity, isScreenLockRefused } from "../services/device-security";

type Check =
  | { kind: "checking" }
  | { kind: "ok" }
  | { kind: "refused" }
  | { kind: "error"; message: string };

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
      setCheck({ kind: "error", message: toErrorMessage(err) });
    }
  }, []);

  useEffect(() => {
    void run();
  }, [run]);

  if (check.kind === "ok") return <>{children}</>;

  if (check.kind === "refused") {
    return (
      <Shell title={t("security.title")}>
        <div className="mf-refusal" role="alert">
          <p>{t("security.body")}</p>
        </div>
        <button type="button" className="mf-btn" onClick={() => void run()}>
          {t("security.recheck")}
        </button>
      </Shell>
    );
  }

  if (check.kind === "error") {
    return (
      <Shell title={t("security.errorTitle")}>
        <ErrorBox title={t("security.errorTitle")} message={check.message} />
        <button type="button" className="mf-btn" onClick={() => void run()}>
          {t("common.retry")}
        </button>
      </Shell>
    );
  }

  return (
    <Shell title={t("security.title")}>
      <p role="status" className="mf-status">
        {t("security.checking")}
      </p>
    </Shell>
  );
}
