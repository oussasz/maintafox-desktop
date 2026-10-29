import {
  Format,
  checkPermissions,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";
import {
  CircleCheck,
  CloudDownload,
  Keyboard,
  QrCode,
  ScanLine,
  UserX,
  WifiOff,
} from "lucide-react";
import { type FormEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import {
  type PlantJoinState,
  getActivationBootstrapState,
  getProductLicenseOnboardingState,
  requestPlantJoin,
} from "@/services/product-license-service";
import { toErrorMessage as toErrorText } from "@/utils/errors";

import { useRemovalNotice } from "../hooks/removal-notice";
import { useMobileT } from "../i18n";
import { formatEnrolmentCode, isCompleteEnrolmentCode } from "../lib/enrolment-code";
import { SLOW_HINT_MS, withUiDeadline } from "../lib/ui-deadline";
import { type UserError, licenceDenial, toUserError } from "../lib/user-error";
import {
  type MobileEnrolmentRedeemed,
  type RedeemMobileEnrolmentInput,
  redeemMobileEnrolment,
} from "../services/enrolment";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Banner, Button, Hero, Progress, Section, Steps, TextCell } from "../ui/kit";

type Phase =
  | { kind: "loading" }
  | { kind: "error"; error: UserError }
  | { kind: "enrol"; denied: UserError | null }
  | { kind: "enrolled"; result: MobileEnrolmentRedeemed }
  | { kind: "plant"; join: PlantJoinState; actionError: UserError | null }
  | { kind: "ready" };

/** How often the "joining the plant" screen asks the engine again. */
const JOIN_POLL_MS = 3000;

/** Onboarding steps shown as progress dots: enrolment, plant, sign-in. */
export const ONBOARDING_STEPS = 3;

/** What the enrolment screen shows: the two ways in, the camera explanation, the typed code. */
type EnrolMode = "choose" | "camera" | "code";

/** A scan the person cancelled (the plugin rejects it): nothing to report. */
const SCAN_CANCELLED = /cancel/i;

/**
 * Activation of the phone on an existing plant, with the QR code or the code its administrator
 * issued (never a licence key). The phone always JOINS: it never bootstraps a
 * tenant (an organisation not initialised is refused, not initialised here) and never creates
 * the first administrator (the engine's `create_first_admin` state is a refusal screen).
 * `children` render once the plant allows signing in.
 */
export function ActivationGate({ children }: { children: ReactNode }) {
  const { t } = useMobileT();
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  const [codeInput, setCodeInput] = useState("");
  const [mode, setMode] = useState<EnrolMode>("choose");
  const [notice, setNotice] = useState<"cameraRefused" | "scanFailed" | null>(null);
  const lastInput = useRef<RedeemMobileEnrolmentInput | null>(null);
  const removalNotice = useRemovalNotice();
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<UserError | null>(null);
  const requestedJoin = useRef(false);
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
        setPhase({ kind: "enrol", denied });
        return;
      }
      const bootstrap = await withUiDeadline(getActivationBootstrapState(), "activation.state");
      switch (bootstrap.plant_join) {
        case null:
          setPhase({ kind: "enrol", denied: null });
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

  // A slow answer: after a few seconds the screen says so.
  const waiting = busy || phase.kind === "loading";
  useEffect(() => {
    setSlow(false);
    if (!waiting) return;
    const timer = setTimeout(() => setSlow(true), SLOW_HINT_MS);
    return () => clearTimeout(timer);
  }, [waiting]);

  /** Sends the scanned text or typed code to the engine, which validates it and enrols. */
  async function redeem(input: RedeemMobileEnrolmentInput) {
    lastInput.current = input;
    setBusy(true);
    setFormError(null);
    setNotice(null);
    try {
      // The engine's own request limit (20 s) comes first; this deadline covers a lost answer.
      const result = await withUiDeadline(redeemMobileEnrolment(input), "activation.enrol");
      setCodeInput("");
      setPhase({ kind: "enrolled", result });
    } catch (err) {
      const error = toUserError(err, "activation.enrol", "activation");
      setFormError(error);
      // An earlier attempt whose answer was lost did activate the phone: go on from its state.
      if (error.kind === "enrolAlreadyActivated") void refresh();
    } finally {
      setBusy(false);
    }
  }

  async function scanNow() {
    let content: string | undefined;
    try {
      const scanned = await scan({ formats: [Format.QRCode] });
      content = scanned.content;
    } catch (err) {
      if (!SCAN_CANCELLED.test(toErrorText(err))) setNotice("scanFailed");
      setMode("choose");
      return;
    }
    if (!content) {
      setNotice("scanFailed");
      setMode("choose");
      return;
    }
    // The raw text goes to the engine untouched: it alone decides whether it is a valid code.
    await redeem({ qr_payload: content });
  }

  function cameraRefused() {
    setNotice("cameraRefused");
    setMode("code");
  }

  async function onScan() {
    setNotice(null);
    setFormError(null);
    let permission: string;
    try {
      permission = await checkPermissions();
    } catch {
      cameraRefused();
      return;
    }
    if (permission === "granted") await scanNow();
    else if (permission === "denied") cameraRefused();
    else setMode("camera");
  }

  async function onAllowCamera() {
    let permission: string;
    try {
      permission = await requestPermissions();
    } catch {
      permission = "denied";
    }
    if (permission === "granted") await scanNow();
    else cameraRefused();
  }

  function onSubmitCode(event: FormEvent) {
    event.preventDefault();
    if (!isCompleteEnrolmentCode(codeInput) || busy) return;
    void redeem({ code: codeInput });
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

  if (phase.kind === "enrolled") {
    const { result } = phase;
    return (
      <OnboardingScreen title={t("activation.enrol.successTitle")}>
        <Hero
          icon={<CircleCheck size={40} strokeWidth={2.2} aria-hidden />}
          title={t("activation.enrol.successTitle")}
          text={t("activation.enrol.assigned", {
            person: result.person_label,
            site: result.site_label,
          })}
        />
        {steps(1)}
        <Actions>
          <Button
            onClick={() => {
              requestedJoin.current = false;
              setPhase({ kind: "loading" });
              void refresh();
            }}
          >
            {t("activation.enrol.continue")}
          </Button>
        </Actions>
      </OnboardingScreen>
    );
  }

  const retryable =
    formError !== null &&
    lastInput.current !== null &&
    (formError.kind === "serverUnreachable" || formError.kind === "serverError");
  const banners = (
    <>
      {removalNotice.oldSlotToRemove ? (
        <Banner tone="info">{t("activation.enrol.unboundNotice")}</Banner>
      ) : null}
      {phase.denied ? <ErrorBanner error={phase.denied} /> : null}
      {notice ? <Banner tone="info">{t(`activation.enrol.${notice}`)}</Banner> : null}
      {formError ? (
        <ErrorBanner
          error={formError}
          {...(retryable && !busy
            ? {
                onRetry: () => {
                  if (lastInput.current) void redeem(lastInput.current);
                },
              }
            : {})}
        />
      ) : null}
      {busy ? <Progress text={t("activation.enrol.enrolling")} /> : null}
      {busy && slow ? <Banner tone="info">{t("activation.slow")}</Banner> : null}
    </>
  );

  if (mode === "camera") {
    return (
      <OnboardingScreen title={t("activation.enrol.cameraTitle")} language>
        <Hero
          icon={<ScanLine size={40} strokeWidth={2.2} aria-hidden />}
          title={t("activation.enrol.cameraTitle")}
          text={t("activation.enrol.cameraText")}
        />
        {steps(0)}
        <Actions>
          <Button onClick={() => void onAllowCamera()}>{t("activation.enrol.cameraAllow")}</Button>
          <Button variant="plain" onClick={() => setMode("code")}>
            {t("activation.enrol.typeCode")}
          </Button>
        </Actions>
      </OnboardingScreen>
    );
  }

  if (mode === "code") {
    return (
      <OnboardingScreen title={t("activation.title")} language>
        <Hero title={t("activation.title")} text={t("activation.joinOnly")} />
        {steps(0)}
        {banners}
        <form className="mf-form-stack" onSubmit={onSubmitCode}>
          <Section footer={t("activation.enrol.codeHint")}>
            <TextCell
              label={t("activation.enrol.codeLabel")}
              value={codeInput}
              placeholder={t("activation.enrol.codePlaceholder")}
              autoCapitalize="characters"
              autoComplete="off"
              spellCheck={false}
              dir="ltr"
              onChange={(e) => setCodeInput(formatEnrolmentCode(e.target.value))}
            />
          </Section>
          <Actions>
            <Button type="submit" disabled={busy || !isCompleteEnrolmentCode(codeInput)}>
              {busy ? t("activation.enrol.enrolling") : t("activation.enrol.codeSubmit")}
            </Button>
            <Button
              variant="plain"
              disabled={busy}
              onClick={() => {
                setFormError(null);
                setNotice(null);
                setMode("choose");
              }}
            >
              <QrCode size={20} aria-hidden />
              {t("activation.enrol.backToScan")}
            </Button>
          </Actions>
        </form>
      </OnboardingScreen>
    );
  }

  return (
    <OnboardingScreen title={t("activation.title")} language>
      <Hero
        title={t("activation.title")}
        text={`${t("activation.joinOnly")} ${t("activation.enrolHint")}`}
      />
      {steps(0)}
      {banners}
      <Actions>
        <Button disabled={busy} onClick={() => void onScan()}>
          <QrCode size={20} aria-hidden />
          {t("activation.enrol.scan")}
        </Button>
        <Button
          variant="gray"
          disabled={busy}
          onClick={() => {
            setFormError(null);
            setNotice(null);
            setMode("code");
          }}
        >
          <Keyboard size={20} aria-hidden />
          {t("activation.enrol.typeCode")}
        </Button>
      </Actions>
    </OnboardingScreen>
  );
}
