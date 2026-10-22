/**
 * Planning windows: when and where work may (not) be scheduled. A window belongs to a site of the
 * organisation (with its sub-nodes) or to the whole plant, and optionally to one equipment of the
 * site. Its type is a value of the SYSTEM list PLANNING.WINDOW_TYPES (production, maintenance
 * window, planned shutdown, schedule freeze); the frontend keeps no list of its own.
 *
 * - A locked window refuses commitments on the jobs it applies to.
 * - An open maintenance / shutdown window is where work needing the equipment stopped goes.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetPicker } from "@/components/assets/AssetPicker";
import { natureValueLabel } from "@/components/lookups/MaintenanceNaturePicker";
import { fromDateTimeLocal, toDateTimeLocal } from "@/components/planning/planning-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ListAccessNote } from "@/components/ui/list-access-note";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { formatOrDash } from "@/lib/display";
import { useCanReadList } from "@/lib/list-access";
import { cn } from "@/lib/utils";
import { listOrgTree } from "@/services/org-node-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type {
  AssetSearchResult,
  CreatePlanningWindowInput,
  OrgTreeRow,
  PlanningWindow,
  ReferenceValue,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export const PLANNING_WINDOW_TYPES_DOMAIN = "PLANNING.WINDOW_TYPES";

const PLANT_WIDE = "__plant__";

type ProductionWindowDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  windows: PlanningWindow[];
  saving: boolean;
  onSubmit: (input: CreatePlanningWindowInput) => Promise<void>;
};

/** Where a window applies, in words: its equipment, its site, or the whole plant. */
export function windowScopeLabel(
  window: Pick<PlanningWindow, "entity_name" | "equipment_code" | "equipment_name">,
  plantWide: string,
): string {
  const equipment = [window.equipment_code, window.equipment_name].filter(Boolean).join(" — ");
  const site = window.entity_name ?? plantWide;
  return equipment ? `${site} · ${equipment}` : site;
}

export function ProductionWindowDialog({
  open,
  onOpenChange,
  windows,
  saving,
  onSubmit,
}: ProductionWindowDialogProps) {
  const { t, i18n } = useTranslation("planning");
  const { formatDateTime } = useFormatters();
  const [types, setTypes] = useState<ReferenceValue[]>([]);
  const [sites, setSites] = useState<OrgTreeRow[]>([]);
  const [windowType, setWindowType] = useState<string>("");
  const [siteId, setSiteId] = useState<number | null>(null);
  // Sites come from the organisation: without its view right only "plant-wide" is offered (and the
  // window types still load — they used to fail together with the refused site list).
  const canReadOrg = useCanReadList("orgNodes");
  const [equipment, setEquipment] = useState<AssetSearchResult | null>(null);
  const [locked, setLocked] = useState(false);
  const [start, setStart] = useState(toDateTimeLocal(new Date().toISOString()));
  const [end, setEnd] = useState(
    toDateTimeLocal(new Date(Date.now() + 24 * 3600_000).toISOString()),
  );
  const [reason, setReason] = useState("");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void Promise.all([
      listPublishedReferenceValuesByDomainCode(PLANNING_WINDOW_TYPES_DOMAIN),
      canReadOrg ? listOrgTree() : Promise.resolve([]),
    ])
      .then(([typeRows, orgRows]) => {
        if (cancelled) return;
        const active = typeRows.filter((v) => v.is_active);
        setTypes(active);
        setWindowType((current) => current || (active[0]?.code ?? ""));
        setSites(orgRows.filter((r) => r.node.status === "active"));
      })
      .catch(() => {
        // The global toast reports the failure; the dialog then offers no choice.
        if (!cancelled) {
          setTypes([]);
          setSites([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, canReadOrg]);

  const typeLabel = (code: string) => {
    const value = types.find((v) => v.code === code);
    return value
      ? natureValueLabel(value, i18n.language)
      : t(`windows.types.${code}`, { defaultValue: code });
  };

  function pickEquipment(asset: AssetSearchResult | null) {
    setEquipment(asset);
    // The equipment's location is the window's site unless a site is already chosen.
    if (asset?.org_node_id != null && siteId == null) setSiteId(asset.org_node_id);
  }

  async function handleSubmit() {
    await onSubmit({
      entity_id: siteId,
      equipment_id: equipment?.id ?? null,
      window_type: windowType,
      start_datetime: fromDateTimeLocal(start),
      end_datetime: fromDateTimeLocal(end),
      is_locked: locked,
      lock_reason: reason.trim() || null,
    });
    setReason("");
    setEquipment(null);
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
            <Select value={windowType} onValueChange={setWindowType}>
              <SelectTrigger className="h-8 text-sm" aria-label={t("windows.type")}>
                <SelectValue placeholder={t("windows.typePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {types.map((type) => (
                  <SelectItem key={type.code} value={type.code}>
                    {natureValueLabel(type, i18n.language)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("windows.site")}</Label>
              <Select
                value={siteId == null ? PLANT_WIDE : String(siteId)}
                onValueChange={(v) => setSiteId(v === PLANT_WIDE ? null : Number(v))}
              >
                <SelectTrigger className="h-8 text-sm" aria-label={t("windows.site")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={PLANT_WIDE}>{t("windows.plantWide")}</SelectItem>
                  {sites.map((row) => (
                    <SelectItem key={row.node.id} value={String(row.node.id)}>
                      {`${"  ".repeat(Math.max(0, row.node.depth))}${row.node.name}`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {!canReadOrg ? <ListAccessNote list="orgNodes" className="mt-1" /> : null}
            </div>
            <div>
              <Label>{t("windows.equipment")}</Label>
              <AssetPicker value={equipment} onChange={pickEquipment} />
              <p className="mt-1 text-xs text-text-muted">{t("windows.equipmentHint")}</p>
            </div>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("windows.start")}</Label>
              <DateTimeField className="h-8 text-sm" value={start} onChange={(v) => setStart(v)} />
            </div>
            <div>
              <Label>{t("windows.end")}</Label>
              <DateTimeField className="h-8 text-sm" value={end} onChange={(v) => setEnd(v)} />
            </div>
          </div>
          <div className="flex items-start gap-2">
            <Switch
              id="planning-window-locked"
              checked={locked}
              onCheckedChange={setLocked}
              aria-label={t("windows.lock")}
            />
            <div>
              <Label htmlFor="planning-window-locked">{t("windows.lock")}</Label>
              <p className="text-xs text-text-muted">
                {locked ? t("windows.lockedHint") : t("windows.openHint")}
              </p>
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
                  {typeLabel(window.window_type)}
                </span>{" "}
                · {windowScopeLabel(window, t("windows.plantWide"))} ·{" "}
                {formatDateTime(window.start_datetime)} – {formatDateTime(window.end_datetime)}
                {window.is_locked === 1 && (
                  <Badge className="ml-2" variant="secondary">
                    {t("windows.locked")}
                  </Badge>
                )}
                {window.scope_review_required === 1 && (
                  <Badge className={cn("ml-2", mfTone.warning.chip)} variant="outline">
                    {t("windows.scopeReview")}
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
              disabled={saving || windowType === ""}
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
