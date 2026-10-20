/** "Transférer" action of a stock line: only offered with the inventory-manage right. */
import { ArrowRightLeft } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { StockTransferDialog } from "@/components/inventory/StockTransferDialog";
import { Button } from "@/components/ui/button";
import type { InventoryArticleRow } from "@/services/inventory-service";
import type { InventoryStockBalance } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export interface StockTransferActionProps {
  article: InventoryArticleRow;
  balance: InventoryStockBalance;
  onTransferred: () => void;
}

export function StockTransferAction({ article, balance, onTransferred }: StockTransferActionProps) {
  const { t } = useTranslation("inventory");
  const [open, setOpen] = useState(false);
  return (
    <PermissionGate permission={P.INV_MANAGE}>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="gap-1.5"
        title={balance.available_qty <= 0 ? t("transfer.noneAvailable") : t("transfer.actionHint")}
        disabled={balance.available_qty <= 0}
        onClick={() => setOpen(true)}
      >
        <ArrowRightLeft className="h-3.5 w-3.5" aria-hidden />
        {t("transfer.action")}
      </Button>
      <StockTransferDialog
        open={open}
        onOpenChange={setOpen}
        article={article}
        source={balance}
        onTransferred={onTransferred}
      />
    </PermissionGate>
  );
}
