/**
 * WoPlanningPanel.tsx
 *
 * Planning sub-panel for a work order: timing fields (planned dates, shift,
 * duration) and assignment. Renders inside WoDetailDialog "Plan" tab.
 * Editable only when WO is in draft/planned/ready_to_schedule status.
 *
 * Phase 2 – Sub-phase 05 – File 02 – Sprint S4.
 */

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { useWoStore } from "@/stores/wo-store";
import type { WoShift, WorkOrder } from "@shared/ipc-types";

// ── Editable statuses ────────────────────────────────────────────────────────

const EDITABLE_STATUSES = new Set(["draft", "planned", "ready_to_schedule"]);

const SHIFT_OPTIONS: { value: WoShift; labelKey: string }[] = [
  { value: "morning", labelKey: "shift.morning" },
  { value: "afternoon", labelKey: "shift.afternoon" },
  { value: "night", labelKey: "shift.night" },
  { value: "full_day", labelKey: "shift.fullDay" },
];

// ── Component ───────────────────────────────────────────────────────────────

interface WoPlanningPanelProps {
  wo: WorkOrder;
}

export function WoPlanningPanel({ wo }: WoPlanningPanelProps) {
  const { t } = useTranslation("ot");
  const planWorkOrder = useWoStore((s) => s.planWorkOrder);
  const saving = useWoStore((s) => s.saving);

  const editable = EDITABLE_STATUSES.has(wo.status);

  const [plannedStart, setPlannedStart] = useState(wo.planned_start ?? "");
  const [plannedEnd, setPlannedEnd] = useState(wo.planned_end ?? "");
  const [duration, setDuration] = useState(
    wo.expected_duration_hours != null ? String(wo.expected_duration_hours) : "",
  );
  const [shift, setShift] = useState<string>(wo.shift ?? "");
  const [dirty, setDirty] = useState(false);

  const markDirty = useCallback(() => setDirty(true), []);

  const handleSave = useCallback(async () => {
    await planWorkOrder({
      id: wo.id,
      expected_row_version: wo.row_version,
      planned_start: plannedStart || null,
      planned_end: plannedEnd || null,
      expected_duration_hours: duration ? Number(duration) : null,
      shift: (shift as WoShift) || null,
    });
    setDirty(false);
  }, [wo, plannedStart, plannedEnd, duration, shift, planWorkOrder]);

  return (
    <div className="space-y-4">
      {/* ── General info (read-only) ───────────────────────────────── */}
      <div className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
        <div>
          <span className="text-muted-foreground">{t("detail.fields.type")}:</span>{" "}
          <span className="font-medium">{wo.type_label ?? "—"}</span>
        </div>
        <div>
          <span className="text-muted-foreground">{t("detail.fields.equipment")}:</span>{" "}
          <span className="font-medium">{wo.equipment_name ?? "—"}</span>
        </div>
        <div>
          <span className="text-muted-foreground">{t("detail.fields.priority")}:</span>{" "}
          <span className="font-medium">{wo.urgency_label ?? "—"}</span>
        </div>
        <div>
          <span className="text-muted-foreground">{t("detail.fields.assignedTo")}:</span>{" "}
          <span className="font-medium">{wo.assigned_to_name ?? "—"}</span>
        </div>
      </div>

      <Separator />

      {/* ── Timing section ─────────────────────────────────────────── */}
      <h4 className="text-sm font-semibold">{t("planning.timing")}</h4>

      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <Label className="text-xs">{t("form.plannedStart.label")}</Label>
          <Input
            type="datetime-local"
            value={plannedStart}
            onChange={(e) => {
              setPlannedStart(e.target.value);
              markDirty();
            }}
            disabled={!editable}
            className="h-8 text-sm"
          />
        </div>

        <div className="space-y-1.5">
          <Label className="text-xs">{t("form.plannedEnd.label")}</Label>
          <Input
            type="datetime-local"
            value={plannedEnd}
            onChange={(e) => {
              setPlannedEnd(e.target.value);
              markDirty();
            }}
            disabled={!editable}
            className="h-8 text-sm"
          />
          {plannedStart && plannedEnd && plannedEnd < plannedStart && (
            <p className="text-xs text-destructive">{t("planning.endBeforeStart")}</p>
          )}
        </div>

        <div className="space-y-1.5">
          <Label className="text-xs">{t("planning.shift")}</Label>
          <Select
            value={shift}
            onValueChange={(v) => {
              setShift(v);
              markDirty();
            }}
            disabled={!editable}
          >
            <SelectTrigger className="h-8 text-sm">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {SHIFT_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {t(opt.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-1.5">
          <Label className="text-xs">{t("form.duration.label")}</Label>
          <Input
            type="number"
            min={0}
            step={0.5}
            value={duration}
            onChange={(e) => {
              setDuration(e.target.value);
              markDirty();
            }}
            disabled={!editable}
            className="h-8 text-sm"
          />
        </div>
      </div>

      {/* ── Save button ──────────────────────────────────────────── */}
      {editable && dirty && (
        <div className="flex justify-end pt-2">
          <Button size="sm" onClick={() => void handleSave()} disabled={saving}>
            {t("planning.save")}
          </Button>
        </div>
      )}
    </div>
  );
}
