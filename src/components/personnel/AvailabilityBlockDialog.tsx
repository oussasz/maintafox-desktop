import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  createAvailabilityBlock,
  isAvailabilityOverlapConfirmError,
  updateAvailabilityBlock,
} from "@/services/personnel-service";
import type { PersonnelAvailabilityBlock } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export interface AvailabilityBlockPersonOption {
  id: number;
  label: string;
}

interface AvailabilityBlockDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: "create" | "edit";
  personOptions: AvailabilityBlockPersonOption[];
  initialPersonnelId?: number | null;
  initialStartLocal?: string;
  initialEndLocal?: string;
  editingBlock?: PersonnelAvailabilityBlock | null;
  onSaved: () => void;
}

function toDatetimeLocalValue(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const FORCED_CRITICAL_TYPES = new Set(["medical", "restriction"]);

export function AvailabilityBlockDialog({
  open,
  onOpenChange,
  mode,
  personOptions,
  initialPersonnelId,
  initialStartLocal,
  initialEndLocal,
  editingBlock,
  onSaved,
}: AvailabilityBlockDialogProps) {
  const { t } = useTranslation("personnel");
  const [personnelId, setPersonnelId] = useState<number | null>(null);
  const [blockType, setBlockType] = useState("leave");
  const [startAt, setStartAt] = useState("");
  const [endAt, setEndAt] = useState("");
  const [reason, setReason] = useState("");
  const [isCritical, setIsCritical] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overlapConfirmPending, setOverlapConfirmPending] = useState(false);
  const [saving, setSaving] = useState(false);

  const forcedCritical = FORCED_CRITICAL_TYPES.has(blockType);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setOverlapConfirmPending(false);
    if (mode === "edit" && editingBlock) {
      setPersonnelId(editingBlock.personnel_id);
      setBlockType(editingBlock.block_type);
      setStartAt(toDatetimeLocalValue(editingBlock.start_at));
      setEndAt(toDatetimeLocalValue(editingBlock.end_at));
      setReason(editingBlock.reason_note ?? "");
      setIsCritical(editingBlock.is_critical);
      return;
    }
    setPersonnelId(initialPersonnelId ?? personOptions[0]?.id ?? null);
    setBlockType("leave");
    setStartAt(initialStartLocal ?? "");
    setEndAt(initialEndLocal ?? "");
    setReason("");
    setIsCritical(false);
  }, [
    open,
    mode,
    editingBlock,
    initialPersonnelId,
    initialStartLocal,
    initialEndLocal,
    personOptions,
  ]);

  useEffect(() => {
    if (forcedCritical) setIsCritical(true);
  }, [forcedCritical]);

  const title = useMemo(
    () => (mode === "edit" ? t("availability.form.editTitle") : t("availability.form.title")),
    [mode, t],
  );

  const submit = async (confirmOverlap: boolean) => {
    if (mode === "create" && personnelId == null) {
      setError(t("availability.form.errors.personnelRequired"));
      return;
    }
    if (!startAt || !endAt) {
      setError(t("availability.form.errors.timeRequired"));
      return;
    }
    const startIso = new Date(startAt).toISOString();
    const endIso = new Date(endAt).toISOString();
    if (startIso >= endIso) {
      setError(t("availability.form.errors.invalidRange"));
      return;
    }

    setSaving(true);
    setError(null);
    try {
      if (mode === "edit" && editingBlock) {
        const updateInput = {
          id: editingBlock.id,
          block_type: blockType,
          start_at: startIso,
          end_at: endIso,
          reason_note: reason.trim() || null,
          is_critical: forcedCritical || isCritical,
          ...(confirmOverlap ? { confirm_overlap: true as const } : {}),
        };
        await updateAvailabilityBlock(updateInput);
      } else if (personnelId != null) {
        const createInput = {
          personnel_id: personnelId,
          block_type: blockType,
          start_at: startIso,
          end_at: endIso,
          reason_note: reason.trim() || null,
          is_critical: forcedCritical || isCritical,
          ...(confirmOverlap ? { confirm_overlap: true as const } : {}),
        };
        await createAvailabilityBlock(createInput);
      }
      setOverlapConfirmPending(false);
      onSaved();
      onOpenChange(false);
    } catch (err) {
      if (isAvailabilityOverlapConfirmError(err)) {
        setOverlapConfirmPending(true);
        setError(t("availability.form.errors.overlapConfirm"));
      } else {
        setOverlapConfirmPending(false);
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3 py-2">
          {mode === "create" ? (
            <label className="text-sm">
              <span className="mb-1 block text-text-muted">{t("availability.form.personnel")}</span>
              <select
                className="h-9 w-full rounded border bg-background px-2"
                value={personnelId ?? ""}
                onChange={(e) => setPersonnelId(Number(e.target.value))}
              >
                {personOptions.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          ) : null}

          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.form.blockType")}</span>
            <select
              className="h-9 w-full rounded border bg-background px-2"
              value={blockType}
              onChange={(e) => setBlockType(e.target.value)}
            >
              <option value="medical">{t("availability.form.types.medical")}</option>
              <option value="restriction">{t("availability.form.types.restriction")}</option>
              <option value="leave">{t("availability.form.types.leave")}</option>
              <option value="training">{t("availability.form.types.training")}</option>
            </select>
          </label>

          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.form.startAt")}</span>
            <Input
              type="datetime-local"
              value={startAt}
              onChange={(e) => setStartAt(e.target.value)}
            />
          </label>

          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.form.endAt")}</span>
            <Input type="datetime-local" value={endAt} onChange={(e) => setEndAt(e.target.value)} />
          </label>

          <label className="text-sm">
            <span className="mb-1 block text-text-muted">{t("availability.form.reason")}</span>
            <Input value={reason} onChange={(e) => setReason(e.target.value)} />
          </label>

          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={forcedCritical || isCritical}
              disabled={forcedCritical}
              onChange={(e) => setIsCritical(e.target.checked)}
            />
            {t("availability.form.isCritical")}
            {forcedCritical ? (
              <span className="text-xs text-text-muted">
                ({t("availability.form.criticalForced")})
              </span>
            ) : null}
          </label>

          {error ? <div className="text-sm text-destructive">{error}</div> : null}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t("availability.form.cancel")}
          </Button>
          <PermissionGate permission={P.PER_MANAGE}>
            {overlapConfirmPending ? (
              <Button
                type="button"
                variant="destructive"
                onClick={() => void submit(true)}
                disabled={saving}
              >
                {saving ? t("common.loading") : t("availability.form.confirmOverlap")}
              </Button>
            ) : (
              <Button type="button" onClick={() => void submit(false)} disabled={saving}>
                {saving
                  ? t("common.loading")
                  : mode === "edit"
                    ? t("availability.form.save")
                    : t("availability.form.submit")}
              </Button>
            )}
          </PermissionGate>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
