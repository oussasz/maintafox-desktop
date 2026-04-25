/**
 * WoFormDialog.tsx
 *
 * Dialog wrapper for WoCreateForm (UX-DW-001 pattern).
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { useTranslation } from "react-i18next";

import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { WoCreateForm } from "@/components/wo/WoCreateForm";
import { useWoStore } from "@/stores/wo-store";

export function WoFormDialog() {
  const { t } = useTranslation("ot");
  const showCreateForm = useWoStore((s) => s.showCreateForm);
  const editingWo = useWoStore((s) => s.editingWo);
  const closeCreateForm = useWoStore((s) => s.closeCreateForm);

  const isEdit = editingWo !== null;

  return (
    <Dialog
      open={showCreateForm}
      onOpenChange={(open) => {
        if (!open) closeCreateForm();
      }}
    >
      <DialogContent
        className="max-w-2xl max-h-[90vh] overflow-hidden flex flex-col"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{isEdit ? t("page.titleEdit") : t("page.titleNew")}</DialogTitle>
        </DialogHeader>
        <WoCreateForm
          key={showCreateForm ? (editingWo ? `e-${editingWo.id}` : "new") : "closed"}
          initial={editingWo}
          onSubmitted={() => closeCreateForm()}
          onCancel={() => closeCreateForm()}
        />
      </DialogContent>
    </Dialog>
  );
}
