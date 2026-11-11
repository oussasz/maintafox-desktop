/**
 * WoDepannageDialog.tsx
 *
 * Dépannage (L1-05): creates a corrective work order and starts it in one step
 * (`start_depannage_wo`). The breakdown time defaults to now; "machine arrêtée" has no default.
 * Before launching, the open work orders already on the chosen equipment are listed so the user can
 * open one instead of writing a duplicate. When a permit is needed the work order stays ready and
 * waits for it (never refused for the permit).
 */

import { AlertTriangle, CheckCircle2, Loader2, Siren } from "lucide-react";
import { type ChangeEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { UrgencySelect } from "@/components/common/urgency/UrgencySelect";
import { fromDateTimeLocal, toDateTimeLocal } from "@/components/planning/planning-model";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StickyActionBar } from "@/components/ui/sticky-action-bar";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { describeAppError } from "@/lib/app-error";
import { listWos, startDepannageWo } from "@/services/wo-service";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { statusToI18nKey } from "@/utils/wo-status";
import type {
  AssetSearchResult,
  UrgencyCode,
  WoDepannageStartResult,
  WorkOrder,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { WoPermitRequirementField } from "./WoPermitRequirementField";
import { useWoPeopleOptions } from "./use-wo-people";
import { woTypeLabel } from "./use-wo-type-labels";

const TITLE_MAX = 200;
/** Select value for "the plant's type for immediate corrective work" (backend default). */
const DEFAULT_TYPE = "__default";
/** Select value for "me" as the assignee (the backend's default). */
const ME = "__me";
const OPEN_WO_LIMIT = 5;

interface WoDepannageDialogProps {
  open: boolean;
  onClose: () => void;
  /** Opens an existing or the new work order. */
  onOpenWo: (woId: number) => void;
  /** Called after a dépannage was written (reload lists). */
  onCreated?: (wo: WorkOrder) => void;
}

export function WoDepannageDialog({ open, onClose, onOpenWo, onCreated }: WoDepannageDialogProps) {
  const { t } = useTranslation("ot");
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent
        className="max-w-2xl max-h-[90vh] overflow-hidden flex flex-col"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Siren className="h-4 w-4 text-status-danger" aria-hidden />
            {t("depannage.title")}
          </DialogTitle>
        </DialogHeader>
        {open ? (
          <WoDepannageForm
            onClose={onClose}
            onOpenWo={onOpenWo}
            {...(onCreated ? { onCreated } : {})}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

type FormProps = Omit<WoDepannageDialogProps, "open">;

export function WoDepannageForm({ onClose, onOpenWo, onCreated }: FormProps) {
  const { t, i18n } = useTranslation("ot");
  const { info } = useSession();
  const { can } = usePermissions();
  const mayAssignOther = can(P.OT_EMERGENCY_ASSIGN);

  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [breakdownAt, setBreakdownAt] = useState(() => toDateTimeLocal(new Date().toISOString()));
  const [machineStopped, setMachineStopped] = useState<boolean | null>(null);
  const [typeCode, setTypeCode] = useState(DEFAULT_TYPE);
  const [urgency, setUrgency] = useState<string>("");
  const [assignee, setAssignee] = useState(ME);
  const [requiresPermit, setRequiresPermit] = useState(false);
  const [triedSubmit, setTriedSubmit] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<WoDepannageStartResult | null>(null);

  // Corrective OT types only (EN 13306 natures); the backend refuses any other.
  const typesCatalog = useWorkOrderTypesCatalog((s) => s.types);
  const loadTypes = useWorkOrderTypesCatalog((s) => s.load);
  useEffect(() => {
    void loadTypes();
  }, [loadTypes]);
  const correctiveTypes = useMemo(
    () =>
      typesCatalog.filter(
        (ty) =>
          ty.is_active &&
          (ty.nature === "corrective_immediate" || ty.nature === "corrective_deferred"),
      ),
    [typesCatalog],
  );

  const { options: people } = useWoPeopleOptions();
  const otherPeople = useMemo(
    () => people.filter((p) => p.value !== String(info?.user_id ?? "")),
    [people, info?.user_id],
  );

  // Open work orders already on the equipment: open one instead of writing a duplicate.
  const [openWos, setOpenWos] = useState<WorkOrder[]>([]);
  const [openWosTotal, setOpenWosTotal] = useState(0);
  const equipmentId = asset != null && asset.id > 0 ? asset.id : null;
  useEffect(() => {
    setOpenWos([]);
    setOpenWosTotal(0);
    if (equipmentId == null) return;
    let cancelled = false;
    listWos({ equipment_id: equipmentId, not_closed_only: true, limit: OPEN_WO_LIMIT, offset: 0 })
      .then((page) => {
        if (cancelled) return;
        setOpenWos(page.items);
        setOpenWosTotal(page.total);
      })
      .catch(() => {
        // Only a hint: the global IPC toast reports the failure, the dépannage stays possible.
      });
    return () => {
      cancelled = true;
    };
  }, [equipmentId]);

  const breakdownFuture =
    breakdownAt !== "" && new Date(fromDateTimeLocal(breakdownAt)).getTime() > Date.now() + 60_000;
  const missing: string[] = [];
  if (equipmentId == null) missing.push(t("form.equipment.label"));
  if (!breakdownAt) missing.push(t("depannage.breakdownAt.label"));
  if (machineStopped === null) missing.push(t("depannage.machineStopped.label"));
  if (!title.trim()) missing.push(t("form.title.label"));
  const valid = missing.length === 0 && !breakdownFuture;

  const submit = async () => {
    setTriedSubmit(true);
    setError(null);
    if (!valid || equipmentId == null || machineStopped === null) return;
    setSaving(true);
    try {
      const res = await startDepannageWo({
        equipment_id: equipmentId,
        type_code: typeCode === DEFAULT_TYPE ? null : typeCode,
        urgency_code: urgency ? (urgency as UrgencyCode) : null,
        title: title.trim(),
        description: description.trim() || null,
        breakdown_at: fromDateTimeLocal(breakdownAt),
        machine_stopped: machineStopped,
        assignee_id: assignee === ME ? null : Number(assignee),
        requires_permit: requiresPermit,
      });
      setResult(res);
      onCreated?.(res.wo);
    } catch (err) {
      const d = describeAppError(err);
      setError(d.description ? `${d.title} — ${d.description}` : d.title);
    } finally {
      setSaving(false);
    }
  };

  if (result) {
    const waiting = result.outcome === "waiting_for_permit";
    return (
      <div className="flex flex-col gap-4 px-1 py-2" data-testid="depannage-result">
        <div
          role="status"
          className={
            waiting
              ? "flex gap-2 rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg"
              : "flex gap-2 rounded-md border border-tone-success-border bg-tone-success-bg px-3 py-2 text-sm text-tone-success-fg"
          }
        >
          {waiting ? (
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          ) : (
            <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          )}
          <div className="space-y-1">
            <p className="font-medium">
              {waiting
                ? t("depannage.result.waiting", { code: result.wo.code })
                : t("depannage.result.started", { code: result.wo.code })}
            </p>
            {waiting && result.required_permit_types.length > 0 ? (
              <p>
                {t("depannage.result.permitTypes", {
                  types: result.required_permit_types.join(", "),
                })}
              </p>
            ) : null}
            {waiting && result.permit_draft_ids.length > 0 ? (
              <p>
                {t("depannage.result.draftsWritten", { count: result.permit_draft_ids.length })}
              </p>
            ) : null}
            {waiting ? <p>{t("depannage.result.issuersNotified")}</p> : null}
          </div>
        </div>
        {result.breakdown_old_warning ? (
          <p className="text-xs text-status-warning" role="note">
            {t("depannage.result.oldBreakdown")}
          </p>
        ) : null}
        <StickyActionBar>
          <Button variant="outline" size="sm" onClick={onClose}>
            {t("depannage.close")}
          </Button>
          <Button size="sm" onClick={() => onOpenWo(result.wo.id)}>
            {t("depannage.openWo", { code: result.wo.code })}
          </Button>
        </StickyActionBar>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 overflow-y-auto max-h-[70vh] px-1 py-2">
      <p className="text-xs text-text-muted">{t("depannage.intro")}</p>

      <FormField
        name="depannage-equipment"
        label={t("form.equipment.label")}
        required
        error={
          triedSubmit && equipmentId == null ? t("form.validation.equipmentRequired") : undefined
        }
      >
        <AssetPicker id="depannage-equipment" value={asset} onChange={setAsset} error={null} />
      </FormField>

      {openWos.length > 0 ? (
        <div
          role="note"
          data-testid="depannage-open-wos"
          className="space-y-2 rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg"
        >
          <p className="font-medium">{t("depannage.openWos.title", { count: openWosTotal })}</p>
          <ul className="space-y-1">
            {openWos.map((wo) => (
              <li key={wo.id} className="flex items-center justify-between gap-2">
                <span className="min-w-0 truncate">
                  {wo.code} — {wo.title}
                  {wo.status_code ? ` (${t(`status.${statusToI18nKey(wo.status_code)}`)})` : ""}
                </span>
                <Button variant="outline" size="sm" onClick={() => onOpenWo(wo.id)}>
                  {t("depannage.openWos.open")}
                </Button>
              </li>
            ))}
          </ul>
          <p className="text-xs">{t("depannage.openWos.hint")}</p>
        </div>
      ) : null}

      <div className="grid grid-cols-2 gap-4">
        <FormField
          name="depannage-breakdown-at"
          label={t("depannage.breakdownAt.label")}
          required
          error={
            breakdownFuture
              ? t("depannage.breakdownAt.future")
              : triedSubmit && !breakdownAt
                ? t("depannage.breakdownAt.required")
                : undefined
          }
        >
          <DateTimeField
            id="depannage-breakdown-at"
            value={breakdownAt}
            onChange={setBreakdownAt}
          />
        </FormField>

        <FormField
          name="depannage-machine-stopped"
          label={t("depannage.machineStopped.label")}
          required
          error={
            triedSubmit && machineStopped === null
              ? t("depannage.machineStopped.required")
              : undefined
          }
        >
          <div className="flex gap-2" role="group" aria-label={t("depannage.machineStopped.label")}>
            <Button
              type="button"
              size="sm"
              variant={machineStopped === true ? "default" : "outline"}
              aria-pressed={machineStopped === true}
              onClick={() => setMachineStopped(true)}
            >
              {t("depannage.machineStopped.yes")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant={machineStopped === false ? "default" : "outline"}
              aria-pressed={machineStopped === false}
              onClick={() => setMachineStopped(false)}
            >
              {t("depannage.machineStopped.no")}
            </Button>
          </div>
        </FormField>
      </div>

      <FormField
        name="depannage-title"
        label={t("form.title.label")}
        required
        error={triedSubmit && !title.trim() ? t("form.validation.titleRequired") : undefined}
      >
        <Input
          id="depannage-title"
          value={title}
          maxLength={TITLE_MAX}
          placeholder={t("depannage.titlePlaceholder")}
          onChange={(e: ChangeEvent<HTMLInputElement>) => setTitle(e.target.value)}
        />
      </FormField>

      <FormField name="depannage-description" label={t("form.description.label")}>
        <Textarea
          id="depannage-description"
          value={description}
          rows={2}
          onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setDescription(e.target.value)}
        />
      </FormField>

      <div className="grid grid-cols-2 gap-4">
        <FormField name="depannage-type" label={t("form.type.label")}>
          <Select value={typeCode} onValueChange={setTypeCode}>
            <SelectTrigger id="depannage-type" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT_TYPE}>{t("depannage.type.default")}</SelectItem>
              {correctiveTypes.map((ty) => (
                <SelectItem key={ty.id} value={ty.code}>
                  {woTypeLabel(ty, i18n.language)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        <FormField name="depannage-urgency" label={t("form.urgency.label")}>
          <UrgencySelect
            id="depannage-urgency"
            value={urgency || null}
            onChange={setUrgency}
            placeholder={t("form.urgency.placeholder")}
            className="w-full"
          />
        </FormField>
      </div>

      <FormField name="depannage-assignee" label={t("depannage.assignee.label")}>
        <Select value={assignee} onValueChange={setAssignee} disabled={!mayAssignOther}>
          <SelectTrigger id="depannage-assignee" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ME}>{t("depannage.assignee.me")}</SelectItem>
            {mayAssignOther
              ? otherPeople.map((p) => (
                  <SelectItem key={p.value} value={p.value}>
                    {p.label}
                  </SelectItem>
                ))
              : null}
          </SelectContent>
        </Select>
        {!mayAssignOther ? (
          <p className="mt-1 text-xs text-text-muted">{t("depannage.assignee.onlyMe")}</p>
        ) : null}
      </FormField>

      <WoPermitRequirementField checked={requiresPermit} onCheckedChange={setRequiresPermit} />

      {error ? (
        <div
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {error}
        </div>
      ) : null}

      {missing.length > 0 ? (
        <p className="text-right text-xs text-text-muted" role="status">
          {t("form.missingFields", { fields: missing.join(", ") })}
        </p>
      ) : null}

      <StickyActionBar>
        <Button variant="outline" size="sm" onClick={onClose} disabled={saving}>
          {t("form.cancel")}
        </Button>
        <Button
          size="sm"
          variant="destructive"
          onClick={() => void submit()}
          disabled={saving || !valid}
          data-testid="depannage-submit"
        >
          {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" aria-hidden /> : null}
          {t("depannage.submit")}
        </Button>
      </StickyActionBar>
    </div>
  );
}
