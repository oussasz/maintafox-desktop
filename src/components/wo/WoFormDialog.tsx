/**
 * WoFormDialog.tsx
 *
 * Dialog wrapper for WoCreateForm (UX-DW-001 pattern).
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { useTranslation } from "react-i18next";

import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { WoCreateForm } from "@/components/wo/WoCreateForm";
import { pushAppToast } from "@/store/app-toast-store";
import { useWoStore } from "@/stores/wo-store";

export function WoFormDialog() {
  const { t } = useTranslation("ot");
  const showCreateForm = useWoStore((s) => s.showCreateForm);
  const editingWo = useWoStore((s) => s.editingWo);
  const createPrefillAsset = useWoStore((s) => s.createPrefillAsset);
  const createPrefillEquipmentId = useWoStore((s) => s.createPrefillEquipmentId);
  const closeCreateForm = useWoStore((s) => s.closeCreateForm);
  const openWo = useWoStore((s) => s.openWo);

  const isEdit = editingWo !== null;
  const formKey = showCreateForm
    ? editingWo
      ? `e-${editingWo.id}`
      : `new-${createPrefillEquipmentId ?? "blank"}`
    : "closed";

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
        {showCreateForm ? (
          <WoCreateForm
            key={formKey}
            initial={editingWo}
            prefillAsset={createPrefillAsset}
            prefillEquipmentId={createPrefillEquipmentId}
            onSubmitted={(wo) => {
              closeCreateForm();
              // A visible confirmation with the work order number (the list may be filtered).
              pushAppToast({
                title: t(isEdit ? "form.savedToast" : "form.createdToast", { code: wo.code }),
                variant: "success",
                ...(isEdit
                  ? {}
                  : {
                      action: { label: t("form.openCreated"), onClick: () => void openWo(wo.id) },
                    }),
              });
            }}
            onCancel={() => closeCreateForm()}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
