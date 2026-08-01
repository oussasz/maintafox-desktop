/**
 * ReferenceCreateModal — create a tenant-managed reference value without
 * leaving the current form. Uses create_operational_reference_value so the
 * new row is immediately available in the published catalog.
 */

import { Loader2 } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { resolveReferenceCreateModal } from "@/components/reference/ReferenceModalFactory";
import type { ReferenceTypeId } from "@/components/reference/reference-types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { createOperationalReferenceValue } from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type { ReferenceValue } from "@shared/ipc-types";

export interface ReferenceCreateModalProps {
  open: boolean;
  onClose: () => void;
  referenceType: ReferenceTypeId;
  /** Required when the registry declares a parent domain. */
  parentValueId?: number | null;
  parentLabel?: string | null;
  onCreated: (value: ReferenceValue) => void;
}

export function ReferenceCreateModal({
  open,
  onClose,
  referenceType,
  parentValueId = null,
  parentLabel = null,
  onCreated,
}: ReferenceCreateModalProps) {
  const { t, i18n } = useTranslation("reference");
  const modal = resolveReferenceCreateModal(referenceType, i18n.language);
  const [label, setLabel] = useState("");
  const [description, setDescription] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reset = useCallback(() => {
    setLabel("");
    setDescription("");
    setError(null);
    setSaving(false);
  }, []);

  const handleClose = useCallback(() => {
    if (saving) return;
    reset();
    onClose();
  }, [onClose, reset, saving]);

  const handleSubmit = useCallback(async () => {
    if (!label.trim()) {
      setError(t("combobox.labelRequired"));
      return;
    }
    if (modal.requireParent && (parentValueId == null || parentValueId <= 0)) {
      setError(t("combobox.parentRequired"));
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const created = await createOperationalReferenceValue({
        domain_code: modal.domainCode,
        label: label.trim(),
        description: description.trim() || null,
        parent_id: modal.requireParent ? parentValueId : null,
      });
      reset();
      onCreated(created);
      onClose();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }, [description, label, modal, onClose, onCreated, parentValueId, reset, t]);

  return (
    <Dialog open={open} onOpenChange={(v) => !v && handleClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{modal.title}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {modal.requireParent && (
            <div className="space-y-1.5">
              <Label>{t("combobox.parentLabel")}</Label>
              <Input value={parentLabel?.trim() || t("combobox.parentMissing")} disabled readOnly />
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="ref-create-label">{t("combobox.fieldLabel")}</Label>
            <Input
              id="ref-create-label"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder={t("combobox.fieldLabelPlaceholder")}
              disabled={saving}
              autoFocus
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="ref-create-desc">{t("combobox.fieldDescription")}</Label>
            <Textarea
              id="ref-create-desc"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("combobox.fieldDescriptionPlaceholder")}
              disabled={saving}
              rows={2}
            />
          </div>

          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={handleClose} disabled={saving}>
            {t("combobox.cancel")}
          </Button>
          <Button type="button" onClick={() => void handleSubmit()} disabled={saving}>
            {saving ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("combobox.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
