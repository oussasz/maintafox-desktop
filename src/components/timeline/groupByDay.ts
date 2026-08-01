/** Calendar day key (YYYY-MM-DD) or raw fallback for invalid ISO. */
export function dayKey(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toISOString().slice(0, 10);
}

/** Localized day header; uses todayLabel when the date is today. */
export function dayLabel(iso: string, locale: string, todayLabel: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return todayLabel;
  return d.toLocaleDateString(locale, {
    day: "2-digit",
    month: "short",
    year: "numeric",
  });
}

export interface DayGroup<T> {
  key: string;
  label: string;
  items: T[];
}

/** Group items by calendar day using a timestamp accessor. Preserves encounter order. */
export function groupItemsByDay<T>(
  items: T[],
  getTimestamp: (item: T) => string,
  locale: string,
  todayLabel: string,
): DayGroup<T>[] {
  const map = new Map<string, DayGroup<T>>();
  for (const item of items) {
    const iso = getTimestamp(item);
    const key = dayKey(iso);
    const existing = map.get(key);
    if (existing) {
      existing.items.push(item);
    } else {
      map.set(key, {
        key,
        label: dayLabel(iso, locale, todayLabel),
        items: [item],
      });
    }
  }
  return Array.from(map.values());
}
