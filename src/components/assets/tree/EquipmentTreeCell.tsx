/**
 * First-column tree chrome for the equipment list: guides, chevron, code, child count.
 * Chevron click expands/collapses only (does not select the row).
 */

import { ChevronDown, ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { VisibleTreeRow } from "@/components/assets/tree/equipment-tree-model";
import { cn } from "@/lib/utils";

const GUIDE_WIDTH_PX = 16;
const CHEVRON_SLOT_PX = 20;

type EquipmentTreeCellProps = {
  row: VisibleTreeRow;
  isExpanded: boolean;
  onToggle: (id: number) => void;
};

export function EquipmentTreeCell({ row, isExpanded, onToggle }: EquipmentTreeCellProps) {
  const { t } = useTranslation("equipment");
  const { depth, isLeaf, childCount, ancestorIsLast, isLastSibling, data } = row;

  return (
    <div className="flex items-center min-w-0">
      <div className="flex items-stretch shrink-0 self-stretch" aria-hidden>
        {ancestorIsLast.map((ancestorLast, i) => (
          <span key={i} className="relative shrink-0" style={{ width: GUIDE_WIDTH_PX }}>
            {!ancestorLast && (
              <span className="absolute left-1/2 top-0 bottom-0 w-px -translate-x-1/2 bg-surface-border" />
            )}
          </span>
        ))}
        {depth > 0 && (
          <span className="relative shrink-0" style={{ width: GUIDE_WIDTH_PX }}>
            {/* vertical stub from parent line */}
            <span
              className={cn(
                "absolute left-1/2 w-px -translate-x-1/2 bg-surface-border",
                isLastSibling ? "top-0 h-1/2" : "top-0 bottom-0",
              )}
            />
            {/* horizontal stub to chevron */}
            <span className="absolute left-1/2 top-1/2 h-px w-1/2 bg-surface-border" />
          </span>
        )}
      </div>

      <span
        className="inline-flex items-center justify-center shrink-0"
        style={{ width: CHEVRON_SLOT_PX }}
      >
        {isLeaf ? (
          <span className="w-3.5 h-3.5" aria-hidden />
        ) : (
          <button
            type="button"
            className="p-0.5 rounded hover:bg-accent text-text-muted"
            aria-expanded={isExpanded}
            aria-label={isExpanded ? t("tree.collapse") : t("tree.expand")}
            onClick={(e) => {
              e.stopPropagation();
              onToggle(row.id);
            }}
          >
            {isExpanded ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )}
          </button>
        )}
      </span>

      <span className="font-mono text-xs truncate min-w-0">
        {data.asset_code}
        {childCount > 0 && (
          <span
            className="ml-1 text-text-muted font-sans"
            title={t("tree.childCount", { count: childCount })}
          >
            ({childCount})
          </span>
        )}
      </span>
    </div>
  );
}
