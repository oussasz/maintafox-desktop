import { AlertTriangle, ClipboardList, KeyRound, RefreshCw, Settings } from "lucide-react";
import { useState } from "react";

import { logout } from "@/services/auth-service";
import type { SessionInfo } from "@shared/ipc-types";

import { useMobileT } from "../i18n";
import { type UserError, toUserError } from "../lib/user-error";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Button, Hero, TabBar, type TabItem } from "../ui/kit";

import { ReportFaultScreen } from "./ReportFaultScreen";
import { SettingsScreen } from "./SettingsScreen";
import { SyncScreen } from "./SyncScreen";
import { WorkOrdersScreen } from "./WorkOrdersScreen";

export type FieldTab = "work" | "report" | "sync" | "settings";

/**
 * The signed-in app: a tab bar (my work orders, report a fault, sync, settings). Every tab stays
 * mounted so a half-filled request or an opened work order survives a tab switch.
 */
export function FieldApp({
  session,
  onSignedOut,
  initialTab = "work",
}: {
  session: SessionInfo;
  onSignedOut: () => void;
  initialTab?: FieldTab;
}) {
  const { t } = useMobileT();
  const [tab, setTab] = useState<FieldTab>(initialTab);
  const [signOutError, setSignOutError] = useState<UserError | null>(null);

  async function signOut() {
    setSignOutError(null);
    try {
      await logout();
      onSignedOut();
    } catch (err) {
      setSignOutError(toUserError(err, "sign_out"));
    }
  }

  if (session.force_password_change === true) {
    return (
      <OnboardingScreen title={t("signin.passwordChangeTitle")}>
        <div role="alert">
          <Hero
            icon={<KeyRound size={40} strokeWidth={2.2} aria-hidden />}
            title={t("signin.passwordChangeTitle")}
            text={t("signin.passwordChangeBody")}
          />
        </div>
        {signOutError ? <ErrorBanner error={signOutError} /> : null}
        <Actions>
          <Button variant="tinted" onClick={() => void signOut()}>
            {t("settings.signOut")}
          </Button>
        </Actions>
      </OnboardingScreen>
    );
  }

  const tabs: Array<TabItem<FieldTab>> = [
    { key: "work", label: t("tabs.work"), icon: <ClipboardList aria-hidden /> },
    { key: "report", label: t("tabs.report"), icon: <AlertTriangle aria-hidden /> },
    { key: "sync", label: t("tabs.sync"), icon: <RefreshCw aria-hidden /> },
    { key: "settings", label: t("tabs.settings"), icon: <Settings aria-hidden /> },
  ];

  return (
    <>
      <div hidden={tab !== "work"}>
        {session.user_id == null ? null : (
          <WorkOrdersScreen userId={session.user_id} name={session.display_name ?? null} />
        )}
      </div>
      <div hidden={tab !== "report"}>
        <ReportFaultScreen userId={session.user_id} />
      </div>
      <div hidden={tab !== "sync"}>
        <SyncScreen />
      </div>
      <div hidden={tab !== "settings"}>
        <SettingsScreen session={session} onSignOut={signOut} signOutError={signOutError} />
      </div>
      <TabBar label={t("tabs.label")} items={tabs} active={tab} onSelect={setTab} />
    </>
  );
}
