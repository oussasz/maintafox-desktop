import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { listActiveTeams } from "@/services/personnel-service";
import type { PmPlanRow as PmPlan } from "@/services/pm-service";
import type { AssetSearchResult, CreatePmPlanInput, UpdatePmPlanInput } from "@shared/ipc-types";

import { formatPmAsset } from "./pm-cells";
import { pmUnreportedErrorText } from "./pm-errors";
import { PM_SCOPE_TYPES, PM_STRATEGY_TYPES } from "./pm-labels";

interface PmPlanFormDialogProps {
  open: boolean;
  mode: "create" | "edit";
  plan?: PmPlan | null;
  saving?: boolean;
  onClose: () => void;
  onCreate: (input: CreatePmPlanInput) => Promise<void>;
  onUpdate: (planId: number, rowVersion: number, input: UpdatePmPlanInput) => Promise<void>;
}

export function PmPlanFormDialog({
  open,
  mode,
  plan,
  saving,
  onClose,
  onCreate,
  onUpdate,
}: PmPlanFormDialogProps) {
  const { t } = useTranslation("pm");
  const [code, setCode] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [strategy, setStrategy] = useState<string>("fixed");
  const [scopeType, setScopeType] = useState<string>("equipment");
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [crewId, setCrewId] = useState<string>("");
  const [criticalityId, setCriticalityId] = useState<string>("");
  const [requiresShutdown, setRequiresShutdown] = useState(false);
  const [requiresPermit, setRequiresPermit] = useState(false);
  const [isActive, setIsActive] = useState(true);
  const [teams, setTeams] = useState<Array<{ id: number; code: string; name: string }>>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    void listActiveTeams()
      .then((rows) => setTeams(rows.map((r) => ({ id: r.id, code: r.code, name: r.name }))))
      .catch(() => {
        // The global IPC toast reports the failure; the crew list stays empty.
        setTeams([]);
      });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    if (mode === "edit" && plan) {
      setCode(plan.code);
      setTitle(plan.title);
      setDescription(plan.description ?? "");
      setStrategy(plan.strategy_type);
      setScopeType(plan.asset_scope_type);
      setAsset(null);
      setCrewId(plan.assigned_team_id != null ? String(plan.assigned_team_id) : "");
      setCriticalityId(plan.criticality_value_id != null ? String(plan.criticality_value_id) : "");
      setRequiresShutdown(plan.requires_shutdown === 1);
      setRequiresPermit(plan.requires_permit === 1);
      setIsActive(plan.is_active === 1);
    } else {
      setCode("");
      setTitle("");
      setDescription("");
      setStrategy("fixed");
      setScopeType("equipment");
      setAsset(null);
      setCrewId("");
      setCriticalityId("");
      setRequiresShutdown(false);
      setRequiresPermit(false);
      setIsActive(true);
    }
    setError(null);
  }, [open, mode, plan]);

  async function handleSubmit() {
    setError(null);
    if (scopeType === "equipment" && mode === "create" && !asset) {
      setError(t("errors.assetRequired"));
      return;
    }
    const assignedTeamId = crewId ? Number(crewId) : null;
    const criticalityValueId = criticalityId ? Number(criticalityId) : null;
    const assetScopeId =
      scopeType === "equipment"
        ? (asset?.id ?? (mode === "edit" ? (plan?.asset_scope_id ?? null) : null))
        : null;

    try {
      if (mode === "create") {
        await onCreate({
          code: code.trim(),
          title: title.trim(),
          description: description.trim() || null,
          asset_scope_type: scopeType,
          asset_scope_id: assetScopeId,
          strategy_type: strategy,
          criticality_value_id: criticalityValueId,
          assigned_team_id: assignedTeamId,
          requires_shutdown: requiresShutdown,
          requires_permit: requiresPermit,
          is_active: isActive,
        });
      } else if (plan) {
        await onUpdate(plan.id, plan.row_version, {
          title: title.trim(),
          description: description.trim() || null,
          asset_scope_type: scopeType,
          asset_scope_id: assetScopeId,
          strategy_type: strategy,
          criticality_value_id: criticalityValueId,
          assigned_team_id: assignedTeamId,
          requires_shutdown: requiresShutdown,
          requires_permit: requiresPermit,
          is_active: isActive,
        });
      }
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {mode === "create" ? t("dialogs.newPlan.title") : t("dialogs.editPlan.title")}
          </DialogTitle>
          <DialogDescription>
            {mode === "create"
              ? t("dialogs.newPlan.description")
              : t("dialogs.editPlan.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 py-2">
          {mode === "create" ? (
            <div className="grid gap-1">
              <Label htmlFor="pm-plan-code">{t("fields.code")}</Label>
              <Input id="pm-plan-code" value={code} onChange={(e) => setCode(e.target.value)} />
            </div>
          ) : null}
          <div className="grid gap-1">
            <Label htmlFor="pm-plan-title">{t("fields.title")}</Label>
            <Input id="pm-plan-title" value={title} onChange={(e) => setTitle(e.target.value)} />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-plan-description">{t("fields.description")}</Label>
            <Textarea
              id="pm-plan-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
            />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.strategy")}</Label>
            <Select value={strategy} onValueChange={setStrategy}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PM_STRATEGY_TYPES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {t(`strategy.${s}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.scopeType")}</Label>
            <Select value={scopeType} onValueChange={setScopeType}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PM_SCOPE_TYPES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {t(`scope.${s}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {scopeType === "equipment" ? (
            <div className="grid gap-1">
              <Label>{t("fields.asset")}</Label>
              <AssetPicker value={asset} onChange={setAsset} />
              {mode === "edit" && plan?.asset_scope_id != null && !asset ? (
                <p className="text-xs text-text-muted">
                  {formatPmAsset(plan.asset_code, plan.asset_name)
                    ? `${t("fields.currentAsset", {
                        asset: formatPmAsset(plan.asset_code, plan.asset_name),
                      })} — `
                    : ""}
                  {t("fields.currentAssetHint")}
                </p>
              ) : null}
            </div>
          ) : null}
          <div className="grid gap-1">
            <Label>{t("fields.crew")}</Label>
            <Select
              value={crewId || "__none"}
              onValueChange={(v) => setCrewId(v === "__none" ? "" : v)}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("labels.none")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none">{t("labels.none")}</SelectItem>
                {teams.map((team) => (
                  <SelectItem key={team.id} value={String(team.id)}>
                    {team.code} — {team.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.criticality")}</Label>
            <ReferenceCombobox
              referenceType="equipment.criticality"
              value={criticalityId || null}
              onChange={(v) => setCriticalityId(v ?? "")}
              valueMode="id"
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={requiresShutdown}
              onCheckedChange={(v) => setRequiresShutdown(v === true)}
            />
            {t("fields.requiresShutdown")}
          </label>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={requiresPermit}
              onCheckedChange={(v) => setRequiresPermit(v === true)}
            />
            {t("fields.requiresPermit")}
          </label>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={isActive} onCheckedChange={(v) => setIsActive(v === true)} />
            {t("fields.isActive")}
          </label>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button
            disabled={saving || !title.trim() || (mode === "create" && !code.trim())}
            onClick={() => void handleSubmit()}
          >
            {mode === "create" ? t("actions.create") : t("actions.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
