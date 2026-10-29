import { createContext, useContext } from "react";

/**
 * What the enrolment screen must tell after a phone erased itself. A phone that was activated
 * with the licence key (kind `unbound`) keeps its old licence slot until the administrator removes
 * it from the Phones list.
 */
export interface RemovalNotice {
  oldSlotToRemove: boolean;
}

export const RemovalNoticeContext = createContext<RemovalNotice>({ oldSlotToRemove: false });

export function useRemovalNotice(): RemovalNotice {
  return useContext(RemovalNoticeContext);
}
