import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  fromDateTimeLocal,
  PLANNING_WINDOW_TYPES,
  toDateTimeLocal,
  type PlanningWindowType,
} from "@/components/planning/planning-model";
import { Badge } from "@/components/ui/badge";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatOrDash } from "@/lib/display";
import type { CreatePlanningWindowInput, PlanningWindow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ProductionWindowDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  teamId: number | null;
  windows: PlanningWindow[];
  saving: boolean;
  onSubmit: (input: CreatePlanningWindowInput) => Promise<void>;
};

export function ProductionWindowDialog({
  open,
  onOpenChange,
  teamId,
  windows,
  saving,
  onSubmit,
}: ProductionWindowDialogProps) {
  const { t } = useTranslation("planning");
  const [windowType, setWindowType] = useState<PlanningWindowType>("production");
  const [start, setStart] = useState(toDateTimeLocal(new Date().toISOString()));
  const [end, setEnd] = useState(
    toDateTimeLocal(new Date(Date.now() + 24 * 3600_000).toISOString()),
  );
  const [reason, setReason] = useState("");

  async function handleSubmit() {
    await onSubmit({
      entity_id: teamId,
      window_type: windowType,
      start_datetime: fromDateTimeLocal(start),
      end_datetime: fromDateTimeLocal(end),
      is_locked: true,
      lock_reason: reason.trim() || null,
    });
    setReason("");
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("windows.title")}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3 py-2">
          <div>
            <Label>{t("windows.type")}</Label>
            <Select
              value={windowType}
              onValueChange={(v) => setWindowType(v as PlanningWindowType)}
            >
              <SelectTrigger className="h-8 text-sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PLANNING_WINDOW_TYPES.map((type) => (
                  <SelectItem key={type} value={type}>
                    {t(`windows.types.${type}`, { defaultValue: type })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("windows.start")}</Label>
              <Input
                className="h-8 text-sm"
                type="datetime-local"
                value={start}
                onChange={(e) => setStart(e.target.value)}
              />
            </div>
            <div>
              <Label>{t("windows.end")}</Label>
              <Input
                className="h-8 text-sm"
                type="datetime-local"
                value={end}
                onChange={(e) => setEnd(e.target.value)}
              />
            </div>
          </div>
          <div>
            <Label>{t("windows.reason")}</Label>
            <Input
              className="h-8 text-sm"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </div>

          <div className="max-h-40 space-y-1 overflow-y-auto text-xs">
            {windows.map((window) => (
              <div
                key={window.id}
                className="rounded-lg border border-surface-border p-2 text-text-secondary"
              >
                <span className="font-medium text-text-primary">
                  {t(`windows.types.${window.window_type}`, {
                    defaultValue: window.window_type,
                  })}
                </span>{" "}
                {new Date(window.start_datetime).toLocaleString()} –{" "}
                {new Date(window.end_datetime).toLocaleString()}
                {window.is_locked === 1 && (
                  <Badge className="ml-2" variant="secondary">
                    {t("windows.locked")}
                  </Badge>
                )}
                {window.lock_reason ? (
                  <div className="text-text-muted">{formatOrDash(window.lock_reason)}</div>
                ) : null}
              </div>
            ))}
            {windows.length === 0 && <p className="text-text-muted">{t("windows.empty")}</p>}
          </div>
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("dialogs.cancel")}
          </Button>
          <PermissionGate permission={P.PLAN_WINDOWS}>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={saving}
              onClick={() => void handleSubmit()}
            >
              {t("windows.add")}
            </Button>
          </PermissionGate>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
