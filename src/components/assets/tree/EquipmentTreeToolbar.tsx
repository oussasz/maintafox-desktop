/**
 * Expand All / Collapse All controls for the equipment list tree.
 */

import { ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

type EquipmentTreeToolbarProps = {
  onExpandAll: () => void;
  onCollapseAll: () => void;
  disabled?: boolean;
};

export function EquipmentTreeToolbar({
  onExpandAll,
  onCollapseAll,
  disabled = false,
}: EquipmentTreeToolbarProps) {
  const { t } = useTranslation("equipment");

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 border-b border-surface-border bg-surface-0">
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 text-xs"
        disabled={disabled}
        onClick={onExpandAll}
      >
        <ChevronsUpDown className="h-3.5 w-3.5" />
        {t("tree.expandAll")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 text-xs"
        disabled={disabled}
        onClick={onCollapseAll}
      >
        <ChevronsDownUp className="h-3.5 w-3.5" />
        {t("tree.collapseAll")}
      </Button>
    </div>
  );
}
