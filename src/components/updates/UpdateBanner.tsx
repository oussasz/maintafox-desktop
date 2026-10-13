import { AlertTriangle, Download, Info, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useUpdater } from "@/hooks/use-updater";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";

import { UpdateInstallDialog } from "./UpdateInstallDialog";
import { UpdateProgress } from "./UpdateProgress";
import { notesForLocale, refusalMessage } from "./update-messages";

type Tone = "info" | "warning" | "danger";

const TONE_CLASSES: Record<Tone, string> = {
  info: "border-primary/30 bg-primary/5",
  warning: "border-status-warning/40 bg-status-warning/10",
  danger: "border-status-danger/30 bg-status-danger/5",
};

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
 * In-app update banner (master plan Phase 5.5), mounted once at the top of the main shell:
 * the offered update (mandatory or not, upgrade or Maintafox-initiated rollback), install
 * progress, refusals and errors in plain language, and the outcome notice of the last update.
 */
export function UpdateBanner() {
  const { t, i18n } = useTranslation("shell");
  const appVersion = useAppStore((s) => s.appVersion);
  const u = useUpdater();
  const [confirmOpen, setConfirmOpen] = useState(false);

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
          <p role="alert">{t("updates.error.install", { version: currentVersion })}</p>
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
              {result.mandatory_reason ?? u.forceReason ?? t("updates.banner.mandatoryDefault")}
            </p>
          ) : null}
          {notes ? (
            <div>
              <p className="text-xs font-medium text-text-secondary">{t("updates.banner.notes")}</p>
              <p className="whitespace-pre-line text-text-secondary">{notes}</p>
            </div>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" onClick={() => setConfirmOpen(true)}>
              {t("updates.banner.installNow")}
            </Button>
            {!u.mandatory ? (
              <Button size="sm" variant="ghost" onClick={u.dismiss}>
                {t("updates.banner.dismiss")}
              </Button>
            ) : null}
          </div>
        </BannerBox>,
      );
    } else if (u.forceRequired && !result?.available) {
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
      className="sticky top-0 z-20 flex flex-col gap-2 border-b border-surface-border bg-surface-0 px-4 py-2"
    >
      {sections}
      {offeredVersion ? (
        <UpdateInstallDialog
          open={confirmOpen}
          version={offeredVersion}
          isRollback={isRollback}
          onCancel={() => setConfirmOpen(false)}
          onConfirm={() => {
            setConfirmOpen(false);
            u.install();
          }}
        />
      ) : null}
    </section>
  );
}
