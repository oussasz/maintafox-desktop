import { ErrorBox, Shell } from "../components/Shell";
import type { MobileStartupState } from "../hooks/use-startup";
import { useMobileT } from "../i18n";

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
      <Shell title={t("startup.failedTitle")}>
        <ErrorBox title={t("startup.failedTitle")} message={state.reason} />
        <button type="button" className="mf-btn" onClick={onRetry}>
          {t("common.retry")}
        </button>
      </Shell>
    );
  }
  if (state.status === "database_key_required") {
    return (
      <Shell title={t("startup.databaseKeyTitle")}>
        <p>{t("startup.databaseKeyBody")}</p>
        <ErrorBox title={t("common.errorTitle")} message={state.reason} />
      </Shell>
    );
  }
  if (state.status === "update_rollback_required") {
    return (
      <Shell title={t("startup.rollbackTitle")}>
        <p>{t("startup.rollbackBody", { from: state.from, to: state.to })}</p>
        <ErrorBox title={t("common.errorTitle")} message={state.reason} />
      </Shell>
    );
  }

  const progress =
    state.status === "loading" && state.stage !== "waiting"
      ? t(`startup.stage.${state.stage}`, { applied: state.applied ?? 0 })
      : t("startup.waiting");
  return (
    <Shell title={t("startup.title")}>
      <p role="status" className="mf-status">
        {progress}
      </p>
    </Shell>
  );
}
