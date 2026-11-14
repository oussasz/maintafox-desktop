/**
 * Display text of positions: the name only; the code is added only to tell apart two positions
 * that share the same name.
 */
export function positionLabels(
  positions: { id: number; code: string; name: string }[],
): Map<number, string> {
  const counts = new Map<string, number>();
  for (const p of positions) counts.set(p.name, (counts.get(p.name) ?? 0) + 1);
  return new Map(
    positions.map((p) => [p.id, (counts.get(p.name) ?? 0) > 1 ? `${p.name} (${p.code})` : p.name]),
  );
}
