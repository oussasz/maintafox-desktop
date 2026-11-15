import { DatabaseZap, History, OctagonAlert } from "lucide-react";

import type { MobileStartupState } from "../hooks/use-startup";
import { useMobileT } from "../i18n";
import { toUserError } from "../lib/user-error";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Hero, Progress } from "../ui/kit";

/** Startup progress, or why the engine did not start. `ready` is handled by the caller. */
export function StartupScreen({
  state,
  onRetry,
}: {
  state: MobileStartupState;
  onRetry: () => void;
}) {
  const { t } = useMobileT();

  if (state.status === "failed") {
    return (
      <OnboardingScreen title={t("startup.failedTitle")}>
        <Hero
          icon={<OctagonAlert size={40} strokeWidth={2.2} aria-hidden />}
          tone="red"
          title={t("startup.failedTitle")}
        />
        <ErrorBanner error={toUserError(state.reason, "startup")} onRetry={onRetry} />
      </OnboardingScreen>
    );
  }
  if (state.status === "database_key_required") {
    toUserError(state.reason, "startup.database_key");
    return (
      <OnboardingScreen title={t("startup.databaseKeyTitle")}>
        <div role="alert">
          <Hero
            icon={<DatabaseZap size={40} strokeWidth={2.2} aria-hidden />}
            tone="red"
            title={t("startup.databaseKeyTitle")}
            text={t("startup.databaseKeyBody")}
          />
        </div>
      </OnboardingScreen>
    );
  }
  if (state.status === "update_rollback_required") {
    toUserError(state.reason, "startup.update_rollback");
    return (
      <OnboardingScreen title={t("startup.rollbackTitle")}>
        <div role="alert">
          <Hero
            icon={<History size={40} strokeWidth={2.2} aria-hidden />}
            tone="red"
            title={t("startup.rollbackTitle")}
            text={t("startup.rollbackBody", { from: state.from, to: state.to })}
          />
        </div>
      </OnboardingScreen>
    );
  }

  const progress =
    state.status === "loading" && state.stage !== "waiting"
      ? t(`startup.stage.${state.stage}`)
      : t("startup.waiting");
  return (
    <OnboardingScreen title={t("startup.title")}>
      <Hero title={t("app.name")} text={t("startup.subtitle")} />
      <Progress text={progress} />
    </OnboardingScreen>
  );
}
