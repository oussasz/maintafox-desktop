/**
 * CriticalityBadge.tsx
 *
 * Standardized badge for asset criticality grades A–D.
 * Used in AssetResultTable, AssetDetailPanel, and AssetTreeNavigator.
 */

import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

type CriticalityGrade = "A" | "B" | "C" | "D";

interface CriticalityBadgeProps {
  criticality: CriticalityGrade | string | null;
  compact?: boolean;
}

const GRADE_STYLES: Record<CriticalityGrade, string> = {
  A: "bg-red-500 text-white hover:bg-red-500/90",
  B: "bg-orange-400 text-white hover:bg-orange-400/90",
  C: "bg-blue-400 text-white hover:bg-blue-400/90",
  D: "bg-gray-300 text-gray-800 hover:bg-gray-300/90",
};

const GRADE_LABELS: Record<CriticalityGrade, string> = {
  A: "criticality.critical",
  B: "criticality.important",
  C: "criticality.standard",
  D: "criticality.low",
};

function isGrade(value: string): value is CriticalityGrade {
  return value === "A" || value === "B" || value === "C" || value === "D";
}

export function CriticalityBadge({ criticality, compact }: CriticalityBadgeProps) {
  const { t } = useTranslation("equipment");

  if (!criticality || !isGrade(criticality)) {
    return (
      <Badge variant="secondary" className="text-xs">
        —
      </Badge>
    );
  }

  const style = GRADE_STYLES[criticality];
  const label = t(GRADE_LABELS[criticality] as never);

  return (
    <Badge className={cn("text-xs border-0", style)}>
      {compact ? criticality : `${criticality} — ${label}`}
    </Badge>
  );
}
