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
import { useContext, useEffect } from "react";
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
