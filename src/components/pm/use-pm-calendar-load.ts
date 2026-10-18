/**
 * Reading the PM calendar's visible dates, with the calendar's own error and loading state.
 *
 * Only the latest request speaks: when the user moves from one month to the next before the first
 * answer arrives, a late failure of the earlier month must not put an error over the month now
 * shown, and its end must not clear the loading state of the request still running.
 */
import { useCallback, useRef, useState } from "react";

import type { PmCalendarRange } from "@/stores/pm-store";

export interface PmCalendarLoad {
  error: unknown;
  loading: boolean;
  /** Read `range` (or, without one, the range last shown again, as a retry does). */
  loadRange: (range?: PmCalendarRange) => void;
}

export function usePmCalendarLoad(
  load: (range?: PmCalendarRange) => Promise<void>,
): PmCalendarLoad {
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const latest = useRef(0);

  const loadRange = useCallback(
    (range?: PmCalendarRange) => {
      const request = ++latest.current;
      setError(null);
      setLoading(true);
      load(range)
        .catch((err: unknown) => {
          if (request === latest.current) setError(err);
        })
        .finally(() => {
          if (request === latest.current) setLoading(false);
        });
    },
    [load],
  );

  return { error, loading, loadRange };
}
