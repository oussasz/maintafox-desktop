import { AlertTriangle, Download, Info, X } from "lucide-react";
import { type ReactNode, useContext, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SafeMarkdown } from "@/components/ui/safe-markdown";
import { PermissionContext } from "@/contexts/PermissionContext";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import { useUpdater } from "@/hooks/use-updater";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";
import { P } from "@shared/rbac/permissions.generated";

import { UpdateInstallDialog } from "./UpdateInstallDialog";
import { UpdateInstallLock } from "./UpdateInstallLock";
import { UpdateProgress } from "./UpdateProgress";
import { mandatoryReasonMessage, notesForLocale, refusalMessage } from "./update-messages";

type Tone = "info" | "warning" | "danger";

const TONE_CLASSES: Record<Tone, string> = {
  info: "border-tone-info-border bg-tone-info-bg",
  warning: "border-tone-warning-border bg-tone-warning-bg",
  danger: "border-tone-danger-border bg-tone-danger-bg",
};

/** Plain-language text of an install failure, by its IPC code. */
function installErrorKey(
  code: string | null,
): "updates.error.adminRequired" | "updates.error.signInRequired" | "updates.error.install" {
  switch (code) {
    case "PERMISSION_DENIED":
      return "updates.error.adminRequired";
    case "AUTH_ERROR":
    case "SESSION_LOCKED":
      return "updates.error.signInRequired";
    default:
      return "updates.error.install";
  }
}

function BannerBox({
  tone,
  icon,
  children,
  onClose,
  closeLabel,
}: {
  tone: Tone;
  icon: ReactNode;
  children: ReactNode;
  onClose?: () => void;
  closeLabel?: string;
}) {
  return (
    <div className={cn("flex items-start gap-3 rounded-md border px-4 py-3", TONE_CLASSES[tone])}>
      <span className="mt-0.5 shrink-0" aria-hidden>
        {icon}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2 text-sm text-text-primary">{children}</div>
      {onClose ? (
        <button
          type="button"
          className="shrink-0 rounded-sm p-1 text-text-muted hover:text-text-primary"
          onClick={onClose}
          aria-label={closeLabel}
          title={closeLabel}
        >
          <X className="h-4 w-4" aria-hidden />
        </button>
      ) : null}
    </div>
  );
}

/**
 * In-app update banner (master plan Phase 5.5), mounted once at the top of the main shell, and
 * inside the licence screen while the licence requires an update (`embedded`, see
 * `ForcedUpdatePanel`): the offered update (mandatory or not, upgrade or Maintafox-initiated
 * rollback), install progress, refusals and errors in plain language, and the outcome notice of
 * the last update. Installing asks for a fresh identity check (step-up); only an administrator of
 * this PC installs (the Rust command decides; a refusal is explained here). From the backup until
 * the restart, the whole app is locked behind `UpdateInstallLock`.
 */
export function UpdateBanner({ embedded = false }: { embedded?: boolean } = {}) {
  const { t, i18n } = useTranslation("shell");
  const appVersion = useAppStore((s) => s.appVersion);
  const u = useUpdater();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [confirmOpen, setConfirmOpen] = useState(false);
  // Installing is for the administrator of this PC (`install_authority`: adm.settings). In the app
  // a user without it gets the explanation instead of a button the server refuses. On the licence
  // screen there is no permission context (nobody signed in to the app yet): the button stays and
  // the server decides, its refusal is explained below.
  const permissions = useContext(PermissionContext);
  const mayInstall = permissions == null || permissions.can(P.ADM_SETTINGS);
  const installRefused = permissions != null && !permissions.isLoading && !mayInstall;

  const result = u.result;
  const currentVersion = result?.current_version ?? appVersion;
  const offeredVersion = result?.available ? result.version : null;
  const isRollback = result?.kind === "rollback";
  const sections: ReactNode[] = [];

  if (u.notice) {
    const notice = u.notice;
    sections.push(
      <BannerBox
        key="notice"
        tone={notice.state === "rolled_back" ? "warning" : "info"}
        icon={
          notice.state === "rolled_back" ? (
            <AlertTriangle className="h-4 w-4 text-text-warning" />
          ) : (
            <Info className="h-4 w-4 text-primary" />
          )
        }
      >
        <p>
          {notice.state === "rolled_back"
            ? t("updates.notice.rolledBack", { from: notice.from_version, to: notice.to_version })
            : t("updates.notice.installNotCompleted", {
                from: notice.from_version,
                to: notice.to_version,
              })}
        </p>
        <div>
          <Button size="sm" variant="outline" onClick={u.acknowledgeNotice}>
            {t("updates.notice.acknowledge")}
          </Button>
        </div>
      </BannerBox>,
    );
  }

  if (u.phase !== "idle") {
    sections.push(
      <BannerBox key="progress" tone="info" icon={<Download className="h-4 w-4 text-primary" />}>
        <p className="font-semibold">{t("updates.progress.title")}</p>
        <UpdateProgress
          phase={u.phase}
          downloadedBytes={u.downloadedBytes}
          totalBytes={u.totalBytes}
        />
      </BannerBox>,
    );
  } else {
    if (u.refusal) {
      sections.push(
        <BannerBox
          key="refusal"
          tone="warning"
          icon={<AlertTriangle className="h-4 w-4 text-text-warning" />}
          onClose={u.clearOutcome}
          closeLabel={t("updates.banner.dismissOutcome")}
        >
          <p role="alert">
            {refusalMessage(
              t,
              u.refusal,
              u.installTargetVersion ?? offeredVersion ?? currentVersion,
            )}
          </p>
        </BannerBox>,
      );
    }

    if (u.error) {
      sections.push(
        <BannerBox
          key="error"
          tone="danger"
          icon={<AlertTriangle className="h-4 w-4 text-text-danger" />}
          onClose={u.clearOutcome}
          closeLabel={t("updates.banner.dismissOutcome")}
        >
          <p role="alert">{t(installErrorKey(u.errorCode), { version: currentVersion })}</p>
          <p className="text-xs text-text-muted">
            {t("updates.error.detail", { detail: u.error })}
          </p>
        </BannerBox>,
      );
    }

    if (u.showOffer && result && offeredVersion) {
      const notes = notesForLocale(i18n.language, result);
      sections.push(
        <BannerBox
          key="offer"
          tone={u.mandatory || isRollback ? "warning" : "info"}
          icon={<Download className="h-4 w-4 text-primary" />}
        >
          <div className="flex flex-wrap items-center gap-2">
            <p className="font-semibold">
              {isRollback
                ? t("updates.banner.rollback", { version: offeredVersion })
                : t("updates.banner.available", { version: offeredVersion })}
            </p>
            {u.mandatory ? (
              <Badge variant="destructive">{t("updates.banner.mandatory")}</Badge>
            ) : null}
          </div>
          {u.mandatory ? (
            <p className="text-text-secondary">
              {result.mandatory_reason || !u.forceReason
                ? mandatoryReasonMessage(t, result.mandatory_reason)
                : u.forceReason}
            </p>
          ) : null}
          {u.mandatory && result.mandatory_message?.trim() ? (
            // Operator text: rendered as text (React escapes it), never as HTML.
            <p className="whitespace-pre-line text-text-secondary">{result.mandatory_message}</p>
          ) : null}
          {notes ? (
            <details className="rounded-md border border-surface-border bg-surface-0/60 px-2 py-1">
              <summary className="cursor-pointer select-none text-xs font-medium text-text-secondary hover:text-text-primary">
                {t("updates.banner.notes")}
              </summary>
              <SafeMarkdown source={notes} skipLeadingHeading className="mt-1.5" />
            </details>
          ) : null}
          {installRefused ? (
            <p className="text-text-secondary" data-testid="update-admin-required">
              {t("updates.error.adminRequired")}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            {mayInstall ? (
              <Button size="sm" onClick={() => setConfirmOpen(true)}>
                {t("updates.banner.installNow")}
              </Button>
            ) : null}
            {!u.mandatory ? (
              <Button size="sm" variant="ghost" onClick={u.dismiss}>
                {t("updates.banner.dismiss")}
              </Button>
            ) : null}
          </div>
        </BannerBox>,
      );
    } else if (u.forceRequired && result?.status === "up_to_date") {
      // The licence requires an update but the server publishes none for this PC (204): say so;
      // the licence stays denied (nothing here unblocks it).
      sections.push(
        <BannerBox
          key="forced"
          tone="warning"
          icon={<AlertTriangle className="h-4 w-4 text-text-warning" />}
        >
          <p>{t("updates.banner.forcedNotOffered")}</p>
          {u.forceReason ? <p className="text-text-secondary">{u.forceReason}</p> : null}
        </BannerBox>,
      );
    }
  }

  if (sections.length === 0) return null;

  return (
    <section
      aria-label={t("updates.banner.regionLabel")}
      className={
        embedded
          ? "flex flex-col gap-2"
          : "sticky top-0 z-20 flex flex-col gap-2 border-b border-surface-border bg-surface-0 px-4 py-2"
      }
    >
      {sections}
      <UpdateInstallLock phase={u.phase} />
      {offeredVersion ? (
        <UpdateInstallDialog
          open={confirmOpen}
          version={offeredVersion}
          isRollback={isRollback}
          onCancel={() => setConfirmOpen(false)}
          onConfirm={() => {
            setConfirmOpen(false);
            u.install({ run: withStepUp, isCancelled: isStepUpCancelled });
          }}
        />
      ) : null}
      {StepUpDialogElement}
    </section>
  );
}
