/**
 * Shared crew crest catalog + color swatches (SSOT with backend TEAM_ICON_KEYS).
 */

import type { LucideIcon } from "lucide-react";
import {
  Anchor,
  Cog,
  Cpu,
  Factory,
  Flame,
  Hammer,
  HardHat,
  Shield,
  ShieldAlert,
  Users,
  Wrench,
  Zap,
} from "lucide-react";

export type CrewIconKey =
  | "shield"
  | "shield-alert"
  | "zap"
  | "wrench"
  | "users"
  | "flame"
  | "anchor"
  | "cpu"
  | "hard-hat"
  | "hammer"
  | "cog"
  | "factory";

export const CREW_ICON_OPTIONS: { key: CrewIconKey; Icon: LucideIcon }[] = [
  { key: "shield", Icon: Shield },
  { key: "shield-alert", Icon: ShieldAlert },
  { key: "zap", Icon: Zap },
  { key: "wrench", Icon: Wrench },
  { key: "users", Icon: Users },
  { key: "flame", Icon: Flame },
  { key: "anchor", Icon: Anchor },
  { key: "cpu", Icon: Cpu },
  { key: "hard-hat", Icon: HardHat },
  { key: "hammer", Icon: Hammer },
  { key: "cog", Icon: Cog },
  { key: "factory", Icon: Factory },
];

/** Shared with org node type swatches — single palette, not a second hardcoded set. */
export const CREW_COLOR_SWATCHES = [
  "#3b82f6",
  "#22c55e",
  "#eab308",
  "#a855f7",
  "#ef4444",
  "#f97316",
  "#06b6d4",
  "#ec4899",
  "#14b8a6",
  "#6366f1",
  "#64748b",
  "#78716c",
] as const;

export function resolveCrewIcon(key: string | null | undefined): LucideIcon | null {
  if (!key) return null;
  return CREW_ICON_OPTIONS.find((o) => o.key === key)?.Icon ?? null;
}

/** First two alphanumeric chars from code (prefer) or name. */
export function crewMonogram(code: string, name: string): string {
  const source = (code.trim() || name.trim()).toUpperCase();
  const chars = source.replace(/[^A-Z0-9]/g, "");
  if (chars.length >= 2) return chars.slice(0, 2);
  if (chars.length === 1) return `${chars}·`;
  return "??";
}
