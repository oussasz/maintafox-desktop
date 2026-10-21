/**
 * Blocking screen from the pre-update backup (or the restore of the backup) until Maintafox
 * restarts (security review SF1): a change made then would be missing from the backup, or lost
 * with the restore. The rest of the app is inert (unreachable by mouse, keyboard and screen
 * readers), focus stays on this screen, no key leaves it and there is nothing to close. The Rust
 * side refuses session commands meanwhile (`updater::WriteBarrier`), so this screen is not the
 * only guard.
 *
 * The download runs before this phase while the app stays usable.
 */

import { Loader2 } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { isBlockingGateActive, setBlockingGateActive } from "@/lib/blocking-gate";
import type { UpdateInstallPhase } from "@/types";

import { UpdateProgress } from "./UpdateProgress";

const LOCKING_PHASES: readonly UpdateInstallPhase[] = [
  "backing_up",
  "restoring_backup",
  "installing",
  "restarting",
];

/** From the backup on, nothing in the app can be changed until it restarts. */
export function isLockingPhase(phase: UpdateInstallPhase): boolean {
  return LOCKING_PHASES.includes(phase);
}

interface UpdateInstallLockProps {
  phase: UpdateInstallPhase;
}

export function UpdateInstallLock({ phase }: UpdateInstallLockProps) {
  const locked = isLockingPhase(phase);
  if (!locked || phase === "idle") return null;
  return createPortal(<LockScreen phase={phase} />, document.body);
}

function LockScreen({ phase }: { phase: Exclude<UpdateInstallPhase, "idle"> }) {
  const { t } = useTranslation("shell");
  const screenRef = useRef<HTMLDivElement>(null);

  // Everything else on the page is inert while the screen shows, and comes back after.
  useEffect(() => {
    const screen = screenRef.current;
    if (!screen) return;
    const others = Array.from(document.body.children).filter(
      (el): el is HTMLElement =>
        el instanceof HTMLElement && el !== screen && !el.hasAttribute("inert"),
    );
    for (const el of others) {
      el.setAttribute("inert", "");
      el.setAttribute("aria-hidden", "true");
    }
    const gateWasActive = isBlockingGateActive();
    setBlockingGateActive(true);
    screen.focus();
    return () => {
      for (const el of others) {
        el.removeAttribute("inert");
        el.removeAttribute("aria-hidden");
      }
      setBlockingGateActive(gateWasActive);
    };
  }, []);

  /** No key leaves the screen (no shortcut, no Escape, no Tab out). */
  const holdKeys = (e: KeyboardEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.preventDefault();
  };

  return (
    <div
      ref={screenRef}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="update-install-lock-title"
      aria-describedby="update-install-lock-body"
      data-testid="update-install-lock"
      tabIndex={-1}
      onKeyDown={holdKeys}
      onBlur={(e) => {
        // Focus never leaves the screen.
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) e.currentTarget.focus();
      }}
      // Above every popover (z-200), dialog and the forced-removal screen (z-300).
      className="fixed inset-0 z-[400] flex items-center justify-center bg-surface-0/95 p-4 outline-none"
    >
      <div className="w-full max-w-md space-y-4 rounded-lg border border-surface-border bg-surface-1 p-5 shadow-lg">
        <div className="flex items-start gap-3">
          <Loader2 className="mt-0.5 h-6 w-6 shrink-0 animate-spin text-primary" aria-hidden />
          <div className="space-y-1">
            <h2 id="update-install-lock-title" className="text-lg font-semibold text-text-primary">
              {t("updates.lock.title")}
            </h2>
            <p id="update-install-lock-body" className="text-sm text-text-secondary">
              {t("updates.lock.body")}
            </p>
          </div>
        </div>
        <UpdateProgress phase={phase} downloadedBytes={0} totalBytes={null} />
      </div>
    </div>
  );
}
