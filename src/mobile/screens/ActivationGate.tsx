import { type FormEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import { getAppInfo } from "@/services/app.service";
import {
  type PlantJoinState,
  type ProductActivationPreview,
  activationRequiresLocalDataReset,
  checkProductLicenseKeyEntry,
  claimProductActivation,
  getActivationBootstrapState,
  getProductLicenseOnboardingState,
  previewProductActivation,
  requestPlantJoin,
  resetLocalTenantRuntimeData,
  submitProductLicenseKey,
} from "@/services/product-license-service";
import { toErrorMessage } from "@/utils/errors";

import { ErrorBox, Shell } from "../components/Shell";
import { useMobileT } from "../i18n";
import { deviceFingerprint, deviceLabel } from "../lib/device-identity";

type Phase =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "key"; denied: string | null }
  | { kind: "preview"; preview: ProductActivationPreview; key: string }
  | { kind: "plant"; join: PlantJoinState; actionError: string | null }
  | { kind: "ready" };

/** How often the "joining the plant" screen asks the engine again. */
const JOIN_POLL_MS = 3000;

/**
 * Activation of the phone on an existing plant. The phone always JOINS: it never bootstraps a
 * tenant (an organisation not initialised is refused, not initialised here) and never creates
 * the first administrator (the engine's `create_first_admin` state is a refusal screen).
 * `children` render once the plant allows signing in.
 */
export function ActivationGate({ children }: { children: ReactNode }) {
  const { t } = useMobileT();
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const requestedJoin = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const onboarding = await getProductLicenseOnboardingState();
      if (!onboarding.complete || onboarding.status.startsWith("denied")) {
        const denied = onboarding.status.startsWith("denied")
          ? (onboarding.deny_message ?? onboarding.deny_reason_code ?? onboarding.status)
          : null;
        setPhase({ kind: "key", denied });
        return;
      }
      const bootstrap = await getActivationBootstrapState();
      switch (bootstrap.plant_join) {
        case null:
          setPhase({ kind: "key", denied: null });
          return;
        case "sign_in":
          setPhase({ kind: "ready" });
          return;
        default:
          setPhase((current) =>
            current.kind === "plant" && current.join === bootstrap.plant_join
              ? current
              : { kind: "plant", join: bootstrap.plant_join as PlantJoinState, actionError: null },
          );
      }
    } catch (err) {
      setPhase({ kind: "error", message: toErrorMessage(err) });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // The first-start screen asks the engine to join the plant (the pull), once, and again on retry.
  const join = useCallback(async () => {
    try {
      await requestPlantJoin();
      setPhase((current) =>
        current.kind === "plant" ? { ...current, actionError: null } : current,
      );
    } catch (err) {
      setPhase((current) =>
        current.kind === "plant" ? { ...current, actionError: toErrorMessage(err) } : current,
      );
    }
    await refresh();
  }, [refresh]);

  const plantJoin = phase.kind === "plant" ? phase.join : null;
  useEffect(() => {
    if ((plantJoin === "joining" || plantJoin === "connection_needed") && !requestedJoin.current) {
      requestedJoin.current = true;
      void join();
    }
  }, [plantJoin, join]);

  useEffect(() => {
    if (plantJoin !== "joining") return;
    const timer = setInterval(() => void refresh(), JOIN_POLL_MS);
    return () => clearInterval(timer);
  }, [plantJoin, refresh]);

  async function onValidateKey(event: FormEvent) {
    event.preventDefault();
    const key = keyInput.trim();
    if (!key) return;
    setBusy(true);
    setFormError(null);
    try {
      const info = await getAppInfo();
      const preview = await previewProductActivation({
        license_key: key,
        machine_fingerprint: deviceFingerprint(),
        machine_label: deviceLabel(),
        app_version: info.version,
      });
      setPhase({ kind: "preview", preview, key });
    } catch (err) {
      setFormError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function onJoinPlant(preview: ProductActivationPreview, key: string) {
    setBusy(true);
    setFormError(null);
    try {
      const info = await getAppInfo();
      const label = deviceLabel();
      // The engine is asked first whether it would accept this licence, before a device slot is
      // taken on the server (same sequence as the desktop activation).
      await checkProductLicenseKeyEntry(preview);
      const claim = await claimProductActivation({
        license_key: key,
        machine_fingerprint: deviceFingerprint(),
        machine_label: label,
        app_version: info.version,
      });
      const onboarding = await getProductLicenseOnboardingState();
      if (activationRequiresLocalDataReset(onboarding.tenant_id, claim.tenant_id)) {
        await resetLocalTenantRuntimeData();
      }
      await submitProductLicenseKey({
        key,
        tenant_id: claim.tenant_id,
        app_version: info.version,
      });
      setKeyInput("");
      requestedJoin.current = false;
      await refresh();
    } catch (err) {
      setFormError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  if (phase.kind === "ready") return <>{children}</>;

  if (phase.kind === "loading") {
    return (
      <Shell title={t("activation.title")}>
        <p role="status" className="mf-status">
          {t("activation.checking")}
        </p>
      </Shell>
    );
  }

  if (phase.kind === "error") {
    return (
      <Shell title={t("activation.title")}>
        <ErrorBox title={t("common.errorTitle")} message={phase.message} />
        <button
          type="button"
          className="mf-btn"
          onClick={() => {
            setPhase({ kind: "loading" });
            void refresh();
          }}
        >
          {t("common.retry")}
        </button>
      </Shell>
    );
  }

  if (phase.kind === "plant") {
    if (phase.join === "create_first_admin") {
      return (
        <Shell title={t("activation.noAdminTitle")}>
          <div className="mf-refusal" role="alert">
            <p>{t("activation.noAdminBody")}</p>
          </div>
          <button type="button" className="mf-btn" onClick={() => void refresh()}>
            {t("activation.recheck")}
          </button>
        </Shell>
      );
    }
    const connectionNeeded = phase.join === "connection_needed";
    return (
      <Shell title={t("activation.title")}>
        <p role="status" className="mf-status">
          {connectionNeeded ? t("activation.connectionNeeded") : t("activation.plantJoining")}
        </p>
        {phase.actionError ? (
          <ErrorBox title={t("common.errorTitle")} message={phase.actionError} />
        ) : null}
        {connectionNeeded ? (
          <button type="button" className="mf-btn" onClick={() => void join()}>
            {t("common.retry")}
          </button>
        ) : null}
      </Shell>
    );
  }

  if (phase.kind === "preview") {
    const { preview, key } = phase;
    const company = preview.tenant_display_name ?? preview.tenant_slug ?? preview.tenant_id;
    const initialised = preview.is_initialized !== false;
    return (
      <Shell title={t("activation.title")}>
        <p>{t("activation.joinOnly")}</p>
        <dl className="mf-facts">
          <dt>{t("activation.company")}</dt>
          <dd>{company}</dd>
          {preview.edition ? (
            <>
              <dt>{t("activation.edition")}</dt>
              <dd>{preview.edition}</dd>
            </>
          ) : null}
        </dl>
        {initialised ? null : (
          <div className="mf-refusal" role="alert">
            <p>{t("activation.notInitialized")}</p>
          </div>
        )}
        {formError ? <ErrorBox title={t("common.errorTitle")} message={formError} /> : null}
        {initialised ? (
          <button
            type="button"
            className="mf-btn mf-btn--primary"
            disabled={busy}
            onClick={() => void onJoinPlant(preview, key)}
          >
            {busy ? t("activation.joining") : t("activation.join")}
          </button>
        ) : null}
        <button
          type="button"
          className="mf-btn"
          disabled={busy}
          onClick={() => {
            setFormError(null);
            setPhase({ kind: "key", denied: null });
          }}
        >
          {t("activation.cancel")}
        </button>
      </Shell>
    );
  }

  return (
    <Shell title={t("activation.title")}>
      <p>{t("activation.joinOnly")}</p>
      {phase.denied ? (
        <ErrorBox title={t("activation.deniedTitle")} message={phase.denied} />
      ) : null}
      <form className="mf-form" onSubmit={(e) => void onValidateKey(e)}>
        <label className="mf-field">
          <span>{t("activation.keyLabel")}</span>
          <input
            className="mf-input"
            value={keyInput}
            autoCapitalize="characters"
            autoComplete="off"
            spellCheck={false}
            dir="ltr"
            onChange={(e) => setKeyInput(e.target.value)}
          />
          <small>{t("activation.keyHint")}</small>
        </label>
        {formError ? <ErrorBox title={t("common.errorTitle")} message={formError} /> : null}
        <button
          type="submit"
          className="mf-btn mf-btn--primary"
          disabled={busy || keyInput.trim() === ""}
        >
          {busy ? t("activation.validating") : t("activation.validate")}
        </button>
      </form>
    </Shell>
  );
}
