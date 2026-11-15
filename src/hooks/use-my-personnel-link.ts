/**
 * Whether the signed-in account is linked to a personnel record (`user_accounts.personnel_id`).
 * Rounds, "My work" and other personal figures are keyed on that record: without it the account
 * is refused only at the end of the work (`GATE_CMD_USER_NO_PERSONNEL`) or sees only zeros. The
 * screens use this to say so up front.
 *
 * `linked` is `null` while loading and when the profile cannot be read (no notice then).
 */
import { useEffect, useState } from "react";

import { getMyProfile } from "@/services/user-service";

export interface MyPersonnelLink {
  loading: boolean;
  linked: boolean | null;
}

export function useMyPersonnelLink(): MyPersonnelLink {
  const [state, setState] = useState<MyPersonnelLink>({ loading: true, linked: null });

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const profile = await getMyProfile();
        if (!cancelled) setState({ loading: false, linked: profile.personnel_id != null });
      } catch {
        // The notice is an aid, never a blocker: an unreadable profile shows nothing.
        if (!cancelled) setState({ loading: false, linked: null });
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  return state;
}
