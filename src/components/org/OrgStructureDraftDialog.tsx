/**
 * Explicit creation of an org structure draft: either the first (bootstrap) draft
 * or a fork from the published model. No implicit defaults — the user confirms intent.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  archiveOrgStructureModel,
  createOrgStructureModel,
  forkOrgDraftFromPublished,
} from "@/services/org-service";
import { toErrorMessage } from "@/utils/errors";

export type OrgStructureDraftDialogVariant = "bootstrap" | "fork";

interface OrgStructureDraftDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  variant: OrgStructureDraftDialogVariant;
  onSuccess: () => void;
}

export function OrgStructureDraftDialog({
  open,
  onOpenChange,
  variant,
  onSuccess,
}: OrgStructureDraftDialogProps) {
  const { t } = useTranslation("org");
  const [description, setDescription] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleOpenChange = (next: boolean) => {
    if (!next) {
      setError(null);
      setDescription("");
    }
    onOpenChange(next);
  };

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    const desc = description.trim() || null;
    try {
      if (variant === "bootstrap") {
        await createOrgStructureModel(desc);
      } else {
        await forkOrgDraftFromPublished(desc);
      }
      setDescription("");
      onOpenChange(false);
      onSuccess();
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {variant === "bootstrap"
              ? t("lifecycle.bootstrapDialogTitle")
              : t("lifecycle.forkDialogTitle")}
          </DialogTitle>
          <DialogDescription className="text-left pt-1 space-y-2">
            {variant === "bootstrap" ? (
              <span>{t("lifecycle.bootstrapDialogBody")}</span>
            ) : (
              <span>{t("lifecycle.forkDialogBody")}</span>
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-2 py-1">
          <Label htmlFor="org-draft-desc" className="text-xs text-text-muted">
            {t("lifecycle.draftDescriptionLabel")}
          </Label>
          <Input
            id="org-draft-desc"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t("lifecycle.draftDescriptionPlaceholder")}
            disabled={submitting}
            autoComplete="off"
          />
        </div>
        {error && (
          <p className="text-sm text-status-danger" role="alert">
            {error}
          </p>
        )}
        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            type="button"
            variant="outline"
            onClick={() => handleOpenChange(false)}
            disabled={submitting}
          >
            {t("lifecycle.cancel")}
          </Button>
          <Button type="button" onClick={() => void submit()} disabled={submitting}>
            {submitting
              ? t("lifecycle.working")
              : variant === "bootstrap"
                ? t("lifecycle.confirmBootstrap")
                : t("lifecycle.confirmFork")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface AbandonOrgDraftDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  draftModelId: number;
  onSuccess: () => void;
}

export function AbandonOrgDraftDialog({
  open,
  onOpenChange,
  draftModelId,
  onSuccess,
}: AbandonOrgDraftDialogProps) {
  const { t } = useTranslation("org");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      await archiveOrgStructureModel(draftModelId);
      onOpenChange(false);
      onSuccess();
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("lifecycle.abandonDialogTitle")}</DialogTitle>
          <DialogDescription className="text-left">
            {t("lifecycle.abandonDialogBody")}
          </DialogDescription>
        </DialogHeader>
        {error && (
          <p className="text-sm text-status-danger" role="alert">
            {error}
          </p>
        )}
        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={submitting}
          >
            {t("lifecycle.cancel")}
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={() => void submit()}
            disabled={submitting}
          >
            {submitting ? t("lifecycle.working") : t("lifecycle.confirmAbandon")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
