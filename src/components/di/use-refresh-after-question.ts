import { useEffect, useRef } from "react";

import { useDiReviewStore } from "@/stores/di-review-store";

/**
 * The return-for-clarification and rejection dialogs open over the request sheet (L6-18), so
 * Annuler leaves the sheet as it was. Once such a dialog is gone, whether the user cancelled or
 * saved, `reload` re-reads the request so the sheet shows its current state.
 */
export function useRefreshAfterQuestion(sheetOpen: boolean, reload: () => void): boolean {
  const questionOpen = useDiReviewStore((s) => s.returnDi_ !== null || s.rejectionDi !== null);
  const wasOpen = useRef(false);
  const reloadRef = useRef(reload);
  reloadRef.current = reload;
  useEffect(() => {
    if (wasOpen.current && !questionOpen && sheetOpen) reloadRef.current();
    wasOpen.current = questionOpen;
  }, [questionOpen, sheetOpen]);
  return questionOpen;
}
