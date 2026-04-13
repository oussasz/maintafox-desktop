/**
 * DiFormDialog.tsx
 *
 * Dialog wrapper that hosts DiCreateForm with open/close controls.
 * Follows UX-DW-001 centered-dialog pattern.
 *
 * Phase 2 – Sub-phase 04 – File 01 – Sprint S4.
 */

import { useTranslation } from "react-i18next";

import { DiCreateForm } from "@/components/di/DiCreateForm";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useDiStore } from "@/stores/di-store";

export function DiFormDialog() {
  const { t } = useTranslation("di");

  const open = useDiStore((s) => s.showCreateForm);
  const editingDi = useDiStore((s) => s.editingDi);
  const closeCreateForm = useDiStore((s) => s.closeCreateForm);

  const isEdit = editingDi !== null;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && closeCreateForm()}>
      <DialogContent
        className="max-w-2xl max-h-[90vh]"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{isEdit ? t("page.titleEdit") : t("page.titleNew")}</DialogTitle>
        </DialogHeader>
        <DiCreateForm
          initial={editingDi}
          onSubmitted={() => closeCreateForm()}
          onCancel={closeCreateForm}
        />
      </DialogContent>
    </Dialog>
  );
}
