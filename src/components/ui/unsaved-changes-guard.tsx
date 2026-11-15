/**
 * UnsavedChangesGuard — asks before work in progress is dropped.
 *
 * While `when` is true:
 * - closing or reloading the window asks the browser's own question;
 * - inside the app router, any navigation (another page, or a change of the query string such as
 *   a study step tab) stops on a ConfirmDialog: "Discard" goes on, "Cancel" stays.
 *
 * Rendered outside a data router (component tests, embedded previews) only the window guard
 * applies — the router guard needs the data router to block a navigation.
 */
import { useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { UNSAFE_DataRouterContext, useBlocker } from "react-router-dom";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";

export interface UnsavedChangesGuardProps {
  when: boolean;
  title: string;
  /** What is lost, in one plain sentence. */
  description: string;
  /** Verb naming the action, e.g. "Discard changes". */
  confirmLabel: string;
}

export function UnsavedChangesGuard(props: UnsavedChangesGuardProps) {
  const dataRouter = useContext(UNSAFE_DataRouterContext);

  useEffect(() => {
    if (!props.when) return;
    const warn = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [props.when]);

  return dataRouter != null ? <RouterLeaveGuard {...props} /> : null;
}

function RouterLeaveGuard({ when, title, description, confirmLabel }: UnsavedChangesGuardProps) {
  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      when &&
      (currentLocation.pathname !== nextLocation.pathname ||
        currentLocation.search !== nextLocation.search),
  );

  return (
    <ConfirmDialog
      open={blocker.state === "blocked"}
      onOpenChange={(open) => {
        if (!open && blocker.state === "blocked") blocker.reset();
      }}
      title={title}
      description={description}
      confirmLabel={confirmLabel}
      destructive
      onConfirm={() => {
        if (blocker.state === "blocked") blocker.proceed();
      }}
    />
  );
}

/**
 * Guard for a create/edit form that lives in a dialog: asks before the route changes (Back, a
 * shortcut) and before the form's own Cancel button drops what was typed. Closing the dialog
 * with Escape or the cross is already asked by `DialogContent`.
 *
 * `dirty` is the form's real "differs from what it opened with" flag. Call `markSaved()` right
 * after a successful save so saving and then leaving never asks.
 */
export function useUnsavedFormGuard({
  dirty,
  onCancel,
}: {
  dirty: boolean;
  onCancel: () => void;
}): {
  /** Wire to the Cancel button: closes at once when clean, asks when something was typed. */
  requestCancel: () => void;
  markSaved: () => void;
  /** Render once inside the form. */
  guard: ReactNode;
} {
  const { t } = useTranslation("common");
  const [saved, setSaved] = useState(false);
  const [asking, setAsking] = useState(false);
  const active = dirty && !saved;
  // A form kept mounted between openings is reset when it opens again: a clean form is unsaved
  // no more, so the next opening starts guarded again.
  useEffect(() => {
    if (!dirty) setSaved(false);
  }, [dirty]);

  const requestCancel = useCallback(() => {
    if (active) setAsking(true);
    else onCancel();
  }, [active, onCancel]);
  const markSaved = useCallback(() => setSaved(true), []);

  const guard = (
    <>
      <UnsavedChangesGuard
        when={active}
        title={t("discardGuard.title")}
        description={t("discardGuard.description")}
        confirmLabel={t("discardGuard.leave")}
      />
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title={t("discardGuard.title")}
        description={t("discardGuard.description")}
        confirmLabel={t("discardGuard.leave")}
        dismissLabel={t("discardGuard.keep")}
        destructive
        onConfirm={onCancel}
      />
    </>
  );
  return { requestCancel, markSaved, guard };
}
