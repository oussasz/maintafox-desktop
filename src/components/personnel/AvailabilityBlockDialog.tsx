import { useEffect, useMemo, useRef, useState } from "react";
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
import { mfInput } from "@/design-system/tokens";
import { describeAppError } from "@/lib/app-error";
import {
  createAvailabilityBlock,
  isAvailabilityOverlapConfirmError,
  updateAvailabilityBlock,
} from "@/services/personnel-service";
import type { PersonnelAvailabilityBlock } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { PersonnelListSelect } from "./PersonnelListSelect";
import { usePersonnelLists } from "./use-personnel-lists";

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
  // Absence types come from the plant's list (Données de référence); a type tagged "critical"
  // always makes the absence critical (the server applies the same rule).
  const lists = usePersonnelLists();
  const [personnelId, setPersonnelId] = useState<number | null>(null);
  const [blockType, setBlockType] = useState("");
  const [startAt, setStartAt] = useState("");
  const [endAt, setEndAt] = useState("");
  const [reason, setReason] = useState("");
  const [isCritical, setIsCritical] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overlapConfirmPending, setOverlapConfirmPending] = useState(false);
  const [saving, setSaving] = useState(false);

  const forcedCritical = lists.option("absence_types", blockType)?.tag === "critical";

  // Latest props for the reset below, read without making them reset triggers: the parent
  // rebuilds `personOptions` on every render, which used to wipe the form while typing.
  const initialRef = useRef({
    editingBlock,
    initialPersonnelId,
    initialStartLocal,
    initialEndLocal,
    personOptions,
  });
  initialRef.current = {
    editingBlock,
    initialPersonnelId,
    initialStartLocal,
    initialEndLocal,
    personOptions,
  };
  /** Changes only when the dialog opens, or opens on another block. */
  const resetKey = open ? `${mode}:${editingBlock?.id ?? "new"}` : null;

  useEffect(() => {
    if (resetKey == null) return;
    const init = initialRef.current;
    setError(null);
    setOverlapConfirmPending(false);
    if (mode === "edit" && init.editingBlock) {
      const b = init.editingBlock;
      setPersonnelId(b.personnel_id);
      setBlockType(b.block_type);
      setStartAt(toDatetimeLocalValue(b.start_at));
      setEndAt(toDatetimeLocalValue(b.end_at));
      setReason(b.reason_note ?? "");
      setIsCritical(b.is_critical);
      return;
    }
    // Opened from a person, the absence is theirs; otherwise the planner chooses the person.
    setPersonnelId(init.initialPersonnelId ?? null);
    setBlockType("");
    setStartAt(init.initialStartLocal ?? "");
    setEndAt(init.initialEndLocal ?? "");
    setReason("");
    setIsCritical(false);
  }, [resetKey, mode]);

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
    if (!blockType) {
      setError(t("availability.form.errors.typeRequired"));
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
        setError(describeAppError(err).title);
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
                className={mfInput.nativeSelect}
                value={personnelId ?? ""}
                onChange={(e) =>
                  setPersonnelId(e.target.value === "" ? null : Number(e.target.value))
                }
              >
                <option value="" disabled>
                  {t("availability.form.personnelPlaceholder")}
                </option>
                {personOptions.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          ) : null}

          <div className="text-sm">
            <span id="availability-block-type" className="mb-1 block text-text-muted">
              {t("availability.form.blockType")}
            </span>
            <PersonnelListSelect
              list="absence_types"
              lists={lists}
              value={blockType}
              onChange={setBlockType}
              ariaLabel={t("availability.form.blockType")}
            />
          </div>

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
