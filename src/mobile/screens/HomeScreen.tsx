import { useState } from "react";

import { logout } from "@/services/auth-service";
import { toErrorMessage } from "@/utils/errors";
import type { SessionInfo } from "@shared/ipc-types";

import { MyWorkOrders } from "../components/MyWorkOrders";
import { ReportFault } from "../components/ReportFault";
import { ErrorBox, Shell } from "../components/Shell";
import { SyncCard } from "../components/SyncCard";
import { useMobileT } from "../i18n";

/** Home after sign-in: my work orders, report a fault, sync, sign out. */
export function HomeScreen({
  session,
  onSignedOut,
}: {
  session: SessionInfo;
  onSignedOut: () => void;
}) {
  const { t } = useMobileT();
  const [error, setError] = useState<string | null>(null);

  async function onSignOut() {
    setError(null);
    try {
      await logout();
      onSignedOut();
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }

  if (session.force_password_change === true) {
    return (
      <Shell title={t("signin.passwordChangeTitle")}>
        <div className="mf-refusal" role="alert">
          <p>{t("signin.passwordChangeBody")}</p>
        </div>
        {error ? <ErrorBox title={t("home.signOutError")} message={error} /> : null}
        <button type="button" className="mf-btn" onClick={() => void onSignOut()}>
          {t("home.signOut")}
        </button>
      </Shell>
    );
  }

  const name = session.display_name ?? session.username ?? "";
  return (
    <Shell title={t("home.title")}>
      <p className="mf-muted">{t("home.signedInAs", { name })}</p>
      {session.user_id == null ? null : <MyWorkOrders userId={session.user_id} />}
      <ReportFault userId={session.user_id} />
      <SyncCard />
      {error ? <ErrorBox title={t("home.signOutError")} message={error} /> : null}
      <button type="button" className="mf-btn" onClick={() => void onSignOut()}>
        {t("home.signOut")}
      </button>
    </Shell>
  );
}
