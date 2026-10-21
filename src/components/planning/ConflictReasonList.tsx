/**
 * Why a job is blocked or flagged, one reason per line: a blocking reason (refuses the
 * commitment) in red with a lock, a warning (enforced later — the permit at OT start, the
 * named technician at commit) in amber. Each reason leads to what it is about: the permit,
 * the stock article, or the planning windows.
 */

import { AlertTriangle, Lock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import {
  conflictReasonLinks,
  conflictReasonText,
  isBlockingSeverity,
} from "@/components/planning/planning-model";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import type { ConflictReason } from "@shared/ipc-types";

type ConflictReasonListProps = {
  reasons: ConflictReason[];
  /** Opens the planning windows (a missing shutdown window); no link when absent. */
  onOpenWindows?: (() => void) | undefined;
  className?: string;
};

export function ConflictReasonList({ reasons, onOpenWindows, className }: ConflictReasonListProps) {
  const { t } = useTranslation("planning");
  const navigate = useNavigate();
  if (reasons.length === 0) return null;
  // Blocking reasons first.
  const ordered = [...reasons].sort(
    (a, b) => Number(isBlockingSeverity(b.severity)) - Number(isBlockingSeverity(a.severity)),
  );
  return (
    <ul className={cn("space-y-1 text-[11px]", className)} aria-label={t("backlog.why")}>
      {ordered.map((reason, i) => {
        const blocking = isBlockingSeverity(reason.severity);
        const Icon = blocking ? Lock : AlertTriangle;
        const links = conflictReasonLinks(reason);
        return (
          <li
            key={`${reason.conflict_type}-${reason.reason_code}-${i}`}
            className={cn(
              "flex items-start gap-1.5 rounded-md border px-1.5 py-1",
              blocking ? "border-status-danger/30 bg-status-danger/10" : mfTone.warning.panel,
            )}
            data-severity={blocking ? "blocking" : "warning"}
          >
            <Icon
              className={cn(
                "mt-0.5 h-3 w-3 shrink-0",
                blocking ? "text-status-danger" : mfTone.warning.text,
              )}
              role="img"
              aria-label={blocking ? t("reasonUi.blocking") : t("reasonUi.warning")}
            />
            <span className="min-w-0 flex-1 text-text-primary">
              {conflictReasonText(reason, t)}
              {links.map((link) => {
                if (link.kind === "windows") {
                  return onOpenWindows ? (
                    <button
                      key="windows"
                      type="button"
                      className="ml-1 underline underline-offset-2"
                      onClick={(e) => {
                        e.stopPropagation();
                        onOpenWindows();
                      }}
                    >
                      {t("reasonUi.openWindows")}
                    </button>
                  ) : null;
                }
                const label =
                  link.kind === "article"
                    ? t("reasonUi.openArticle", { code: link.code })
                    : link.kind === "permit"
                      ? t("reasonUi.openPermit")
                      : t("reasonUi.requestPermit");
                return (
                  <button
                    key={link.path}
                    type="button"
                    className="ml-1 underline underline-offset-2"
                    onClick={(e) => {
                      e.stopPropagation();
                      void navigate(link.path);
                    }}
                  >
                    {label}
                  </button>
                );
              })}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
