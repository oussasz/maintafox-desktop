/**
 * Day grouping for the shared timeline. Days are the user's LOCAL calendar days — the same day
 * the header shows and the same rule as the planning / personnel calendars — so an event at
 * 23:30 local time is listed under that evening, not under the next UTC day.
 */

function localYmd(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Local calendar day key (YYYY-MM-DD) or the raw value when it is not a valid timestamp. */
export function dayKey(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return localYmd(d);
}

/** Localized day header of the local day; `todayLabel` when it is today. */
export function dayLabel(
  iso: string,
  locale: string,
  todayLabel: string,
  now = new Date(),
): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  if (localYmd(d) === localYmd(now)) return todayLabel;
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

/** Group items by local calendar day using a timestamp accessor. Preserves encounter order. */
export function groupItemsByDay<T>(
  items: T[],
  getTimestamp: (item: T) => string,
  locale: string,
  todayLabel: string,
  now = new Date(),
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
        label: dayLabel(iso, locale, todayLabel, now),
        items: [item],
      });
    }
  }
  return Array.from(map.values());
}
