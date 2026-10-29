import { Building2, CloudDownload, UserX, WifiOff } from "lucide-react";
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

import { useMobileT } from "../i18n";
import { deviceFingerprint, deviceLabel } from "../lib/device-identity";
import { type UserError, licenceDenial, toUserError } from "../lib/user-error";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Banner, Button, Hero, Progress, Row, Section, Steps, TextCell } from "../ui/kit";

type Phase =
  | { kind: "loading" }
  | { kind: "error"; error: UserError }
  | { kind: "key"; denied: UserError | null }
  | { kind: "preview"; preview: ProductActivationPreview; key: string }
  | { kind: "plant"; join: PlantJoinState; actionError: UserError | null }
  | { kind: "ready" };

/** How often the "joining the plant" screen asks the engine again. */
const JOIN_POLL_MS = 3000;

/** Onboarding steps shown as progress dots: licence key, plant, sign-in. */
export const ONBOARDING_STEPS = 3;

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
  const [formError, setFormError] = useState<UserError | null>(null);
  const requestedJoin = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const onboarding = await getProductLicenseOnboardingState();
      if (!onboarding.complete || onboarding.status.startsWith("denied")) {
        let denied: UserError | null = null;
        if (onboarding.status.startsWith("denied")) {
          console.error(
            "[maintafox-mobile] licence denied",
            onboarding.deny_reason_code ?? "",
            onboarding.deny_message ?? onboarding.status,
          );
          denied = licenceDenial(onboarding.deny_reason_code);
        }
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
      const error = toUserError(err, "activation.state", "activation");
      // While the plant is downloading, a failed check keeps the download screen with a banner.
      setPhase((current) =>
        current.kind === "plant" ? { ...current, actionError: error } : { kind: "error", error },
      );
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
      const error = toUserError(err, "activation.plant_join");
      setPhase((current) =>
        current.kind === "plant" ? { ...current, actionError: error } : current,
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
      setFormError(toUserError(err, "activation.preview", "activation"));
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
      setFormError(toUserError(err, "activation.claim", "activation"));
    } finally {
      setBusy(false);
    }
  }

  if (phase.kind === "ready") return <>{children}</>;

  const steps = (current: number) => (
    <Steps count={ONBOARDING_STEPS} current={current} label={t("onboarding.steps")} />
  );

  if (phase.kind === "loading") {
    return (
      <OnboardingScreen title={t("activation.title")}>
        <Progress text={t("activation.checking")} />
      </OnboardingScreen>
    );
  }

  if (phase.kind === "error") {
    return (
      <OnboardingScreen title={t("activation.title")}>
        <Hero title={t("activation.title")} />
        <ErrorBanner
          error={phase.error}
          onRetry={() => {
            setPhase({ kind: "loading" });
            void refresh();
          }}
        />
      </OnboardingScreen>
    );
  }

  if (phase.kind === "plant") {
    if (phase.join === "create_first_admin") {
      return (
        <OnboardingScreen title={t("activation.noAdminTitle")}>
          <div role="alert">
            <Hero
              icon={<UserX size={40} strokeWidth={2.2} aria-hidden />}
              tone="red"
              title={t("activation.noAdminTitle")}
              text={t("activation.noAdminBody")}
            />
          </div>
          <Actions>
            <Button variant="tinted" onClick={() => void refresh()}>
              {t("activation.recheck")}
            </Button>
          </Actions>
        </OnboardingScreen>
      );
    }
    if (phase.join === "connection_needed") {
      return (
        <OnboardingScreen title={t("activation.connectionNeededTitle")}>
          <Hero
            icon={<WifiOff size={40} strokeWidth={2.2} aria-hidden />}
            title={t("activation.connectionNeededTitle")}
            text={t("activation.connectionNeeded")}
          />
          {steps(1)}
          {phase.actionError ? <ErrorBanner error={phase.actionError} /> : null}
          <Actions>
            <Button onClick={() => void join()}>{t("common.retry")}</Button>
          </Actions>
        </OnboardingScreen>
      );
    }
    return (
      <OnboardingScreen title={t("activation.plantJoiningTitle")}>
        <Hero
          icon={<CloudDownload size={40} strokeWidth={2.2} aria-hidden />}
          title={t("activation.plantJoiningTitle")}
          text={t("activation.plantJoiningHint")}
        />
        {steps(1)}
        <Progress bar text={t("activation.plantJoining")} />
        {phase.actionError ? <ErrorBanner error={phase.actionError} /> : null}
      </OnboardingScreen>
    );
  }

  if (phase.kind === "preview") {
    const { preview, key } = phase;
    const company = preview.tenant_display_name ?? preview.tenant_slug ?? preview.tenant_id;
    const initialised = preview.is_initialized !== false;
    return (
      <OnboardingScreen title={t("activation.confirmTitle")}>
        <Hero
          icon={<Building2 size={40} strokeWidth={2.2} aria-hidden />}
          title={t("activation.confirmTitle")}
          text={t("activation.confirmText")}
        />
        {steps(1)}
        <Section>
          <Row title={t("activation.company")} detail={company} />
          {preview.edition ? (
            <Row title={t("activation.edition")} detail={preview.edition} />
          ) : null}
        </Section>
        {initialised ? null : (
          <Banner tone="warning" alert title={t("activation.notInitializedTitle")}>
            {t("activation.notInitialized")}
          </Banner>
        )}
        {formError ? <ErrorBanner error={formError} /> : null}
        <Actions>
          {initialised ? (
            <Button disabled={busy} onClick={() => void onJoinPlant(preview, key)}>
              {busy ? t("activation.joining") : t("activation.join")}
            </Button>
          ) : null}
          <Button
            variant="plain"
            disabled={busy}
            onClick={() => {
              setFormError(null);
              setPhase({ kind: "key", denied: null });
            }}
          >
            {t("activation.cancel")}
          </Button>
        </Actions>
      </OnboardingScreen>
    );
  }

  return (
    <OnboardingScreen title={t("activation.title")} language>
      <Hero title={t("activation.title")} text={t("activation.joinOnly")} />
      {steps(0)}
      {phase.denied ? <ErrorBanner error={phase.denied} /> : null}
      <form className="mf-form-stack" onSubmit={(e) => void onValidateKey(e)}>
        <Section footer={t("activation.keyHint")}>
          <TextCell
            label={t("activation.keyLabel")}
            value={keyInput}
            placeholder={t("activation.keyPlaceholder")}
            autoCapitalize="characters"
            autoComplete="off"
            spellCheck={false}
            dir="ltr"
            onChange={(e) => setKeyInput(e.target.value)}
          />
        </Section>
        {formError ? <ErrorBanner error={formError} /> : null}
        <Actions>
          <Button type="submit" disabled={busy || keyInput.trim() === ""}>
            {busy ? t("activation.validating") : t("activation.validate")}
          </Button>
        </Actions>
      </form>
    </OnboardingScreen>
  );
}
