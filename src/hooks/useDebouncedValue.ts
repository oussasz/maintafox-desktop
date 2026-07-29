/**
 * useDebouncedValue — returns a value that updates only after `delayMs` of stability.
 * Default delay matches the WO/DI search toolbar convention (300ms).
 */

import { useEffect, useState } from "react";

export function useDebouncedValue<T>(value: T, delayMs = 300): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);

  return debounced;
}
