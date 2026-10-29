import { CloudUpload, Eraser, PhoneOff, ShieldAlert } from "lucide-react";
import { Fragment, type ReactNode, useEffect, useMemo, useState } from "react";

import { RemovalNoticeContext } from "../hooks/removal-notice";
import { useMobileT } from "../i18n";
import { toUserError } from "../lib/user-error";
import {
  type PhoneRemovalStatus,
  getPhoneRemovalStatus,
  onPhoneRemovalStatus,
} from "../services/enrolment";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Banner, Hero, Progress, Row, Section } from "../ui/kit";

type RemovalKind = "removed" | "unbound" | "revoked";

/** The engine reports no kind only before it has recorded one: read it as an administrator removal. */
function kindOf(status: PhoneRemovalStatus): RemovalKind {
  return status.kind ?? "removed";
}

/** Files sent for a work order are counted under this key of `dropped_by_kind`. */
const MEDIA_FILES = "media_files";

/**
 * A phone removed by its administrator (or reported lost, or activated before enrolment existed)
 * uploads what it still holds, then erases itself. This screen only reports; it has no button
 * that could cancel or skip the upload, because the data must reach the server before it goes.
 */
export function PhoneRemovalScreen({ status }: { status: PhoneRemovalStatus }) {
  const { t } = useMobileT();
  const kind = kindOf(status);
  const waiting = status.phase === "waiting_to_upload";
  const erasing = status.phase === "erasing";

  const text =
    kind === "revoked"
      ? t("removal.text.revoked")
      : t(
          `removal.text.${kind}.${erasing ? "erasing" : waiting ? "waiting_to_upload" : "uploading"}`,
        );
  const Icon =
    kind === "revoked" ? ShieldAlert : erasing ? Eraser : waiting ? PhoneOff : CloudUpload;

  // The engine's technical failure is classified (and logged) once per failure, never shown as is.
  const lastError = status.last_error;
  const failure = useMemo(
    () => (lastError ? toUserError(lastError, "removal.upload") : null),
    [lastError],
  );

  const photosDropped = status.dropped_by_kind[MEDIA_FILES] ?? 0;
  const showCounts = kind !== "revoked" || status.dropped_count > 0;

  return (
    <OnboardingScreen title={t(`removal.title.${kind}`)}>
      <div role="status" aria-live="polite">
        <Hero
          icon={<Icon size={40} strokeWidth={2.2} aria-hidden />}
          tone={kind === "revoked" || waiting ? "red" : "tint"}
          title={t(`removal.title.${kind}`)}
          text={text}
        />
      </div>
      {waiting && failure ? (
        <>
          <ErrorBanner error={failure} />
          <Banner tone="info">{t("removal.waiting")}</Banner>
        </>
      ) : null}
      {showCounts ? (
        <Section>
          {kind !== "revoked" ? (
            <Row title={t("removal.uploaded", { n: status.uploaded_count })} />
          ) : null}
          {status.dropped_count > 0 ? (
            <Row title={t("removal.dropped", { n: status.dropped_count })} />
          ) : null}
          {photosDropped > 0 ? (
            <Row title={t("removal.droppedPhotos", { n: photosDropped })} />
          ) : null}
        </Section>
      ) : null}
      <Banner tone="warning">{t("removal.locked")}</Banner>
    </OnboardingScreen>
  );
}

/**
 * Shows the removal screen whenever the phone is removing itself: at startup (the engine's
 * status) and live (its event). When the erase is done, the children (the enrolment) are shown
 * again from a clean state; a phone that had been activated with the licence key then also tells
 * the person that the administrator must free the old slot.
 */
export function PhoneRemovalGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<PhoneRemovalStatus | null>(null);
  const [rounds, setRounds] = useState(0);
  const [unboundErased, setUnboundErased] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const apply = (next: PhoneRemovalStatus) => {
      if (cancelled) return;
      setStatus((previous) => {
        if (next.phase === "done" && previous?.phase !== "done") setRounds((n) => n + 1);
        return next;
      });
      if (next.phase === "done" && next.kind === "unbound") setUnboundErased(true);
    };
    // Subscribe first, then read: a change between the two is never missed.
    void onPhoneRemovalStatus(apply)
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch((err: unknown) => toUserError(err, "removal.subscribe"));
    getPhoneRemovalStatus()
      .then((current) => {
        // An event that arrived first is newer than this answer.
        if (!cancelled) setStatus((previous) => previous ?? current);
        if (!cancelled && current.phase === "done" && current.kind === "unbound") {
          setUnboundErased(true);
        }
      })
      .catch((err: unknown) => {
        // Unreadable status: the app goes on (the engine still refuses sign-in while erasing).
        toUserError(err, "removal.status");
        if (!cancelled) setStatus((previous) => previous ?? { ...NO_REMOVAL });
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const notice = useMemo(() => ({ oldSlotToRemove: unboundErased }), [unboundErased]);

  if (status === null) {
    return <RemovalChecking />;
  }
  if (status.phase !== "none" && status.phase !== "done") {
    return <PhoneRemovalScreen status={status} />;
  }
  return (
    <RemovalNoticeContext.Provider value={notice}>
      <Fragment key={rounds}>{children}</Fragment>
    </RemovalNoticeContext.Provider>
  );
}

const NO_REMOVAL: PhoneRemovalStatus = {
  phase: "none",
  kind: null,
  uploaded_count: 0,
  dropped_count: 0,
  dropped_by_kind: {},
  last_error: null,
  wipe_confirmed: null,
};

function RemovalChecking() {
  const { t } = useMobileT();
  return (
    <OnboardingScreen title={t("startup.title")}>
      <Progress text={t("common.loading")} />
    </OnboardingScreen>
  );
}
