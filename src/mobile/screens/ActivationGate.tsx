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
import { SLOW_HINT_MS, withUiDeadline } from "../lib/ui-deadline";
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
  // Bumped when the person stops waiting for a licence check: its late answer is then ignored.
  const previewAttempt = useRef(0);
  const [slow, setSlow] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const onboarding = await withUiDeadline(
        getProductLicenseOnboardingState(),
        "activation.state",
      );
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
      const bootstrap = await withUiDeadline(getActivationBootstrapState(), "activation.state");
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

  // A slow answer: after a few seconds the screen says so (and, for the licence check, offers to
  // stop waiting).
  const waiting = busy || phase.kind === "loading";
  useEffect(() => {
    setSlow(false);
    if (!waiting) return;
    const timer = setTimeout(() => setSlow(true), SLOW_HINT_MS);
    return () => clearTimeout(timer);
  }, [waiting]);

  async function onValidateKey(event: FormEvent) {
    event.preventDefault();
    const key = keyInput.trim();
    if (!key) return;
    const attempt = ++previewAttempt.current;
    setBusy(true);
    setFormError(null);
    try {
      const preview = await withUiDeadline(
        getAppInfo().then((info) =>
          previewProductActivation({
            license_key: key,
            machine_fingerprint: deviceFingerprint(),
            machine_label: deviceLabel(),
            app_version: info.version,
          }),
        ),
        "activation.preview",
      );
      if (attempt !== previewAttempt.current) return;
      setPhase({ kind: "preview", preview, key });
    } catch (err) {
      if (attempt !== previewAttempt.current) return;
      setFormError(toUserError(err, "activation.preview", "activation"));
    } finally {
      if (attempt === previewAttempt.current) setBusy(false);
    }
  }

  /** Stops waiting for the licence check (a preview only: it takes no device slot). */
  function stopWaitingForPreview() {
    previewAttempt.current += 1;
    setBusy(false);
  }

  async function onJoinPlant(preview: ProductActivationPreview, key: string) {
    setBusy(true);
    setFormError(null);
    try {
      // Each step has its own deadline; there is no "stop waiting" here, because the claim takes a
      // device slot on the server and must be followed through (a retry claims the same slot).
      const where = "activation.claim";
      const info = await withUiDeadline(getAppInfo(), where);
      const label = deviceLabel();
      // The engine is asked first whether it would accept this licence, before a device slot is
      // taken on the server (same sequence as the desktop activation).
      await withUiDeadline(checkProductLicenseKeyEntry(preview), where);
      const claim = await withUiDeadline(
        claimProductActivation({
          license_key: key,
          machine_fingerprint: deviceFingerprint(),
          machine_label: label,
          app_version: info.version,
        }),
        where,
      );
      const onboarding = await withUiDeadline(getProductLicenseOnboardingState(), where);
      if (activationRequiresLocalDataReset(onboarding.tenant_id, claim.tenant_id)) {
        await withUiDeadline(resetLocalTenantRuntimeData(), where);
      }
      await withUiDeadline(
        submitProductLicenseKey({
          key,
          tenant_id: claim.tenant_id,
          app_version: info.version,
        }),
        where,
      );
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
        {slow ? <Banner tone="info">{t("activation.slow")}</Banner> : null}
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
        {busy && slow ? <Banner tone="info">{t("activation.slow")}</Banner> : null}
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
        {busy && slow ? <Banner tone="info">{t("activation.slow")}</Banner> : null}
        <Actions>
          <Button type="submit" disabled={busy || keyInput.trim() === ""}>
            {busy ? t("activation.validating") : t("activation.validate")}
          </Button>
          {busy && slow ? (
            <Button variant="plain" onClick={stopWaitingForPreview}>
              {t("common.cancel")}
            </Button>
          ) : null}
        </Actions>
      </form>
    </OnboardingScreen>
  );
}
