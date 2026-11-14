/**
 * WoSubcontractingCard.tsx
 *
 * Contractor work on a work order (L3-09, L3-10): the external company doing the job, its order or
 * contract reference, the hours it declares and the cost it invoices. The cost is the work order's
 * service cost (the close-out panel edits the same value), so the budget and the contractor's
 * consumed contract amount read one figure. A contractor whose contract is expiring or expired is
 * flagged next to the pick.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useInRouterContext } from "react-router-dom";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { DecimalField } from "@/components/ui/decimal-field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type WoContractorOption,
  type WoSubcontracting,
  getWoSubcontracting,
  listWoContractorOptions,
  setWoSubcontracting,
} from "@/services/wo-closeout-service";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

const NO_CONTRACTOR = "__none__";

const CONTRACT_TONE: Record<string, Tone> = {
  none: "neutral",
  active: "success",
  expiring: "attention",
  expired: "danger",
};

/** Statuses after which the contractor facts are frozen (same rule as the service cost). */
const FROZEN_STATUSES = new Set(["closed", "cancelled"]);

interface WoSubcontractingCardProps {
  wo: WorkOrder;
  canEdit: boolean;
}

export function WoSubcontractingCard({ wo, canEdit }: WoSubcontractingCardProps) {
  const { t } = useTranslation("ot");
  const { currencyLabel, formatDate } = useFormatters();
  const inRouter = useInRouterContext();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const [saved, setSaved] = useState<WoSubcontracting | null>(null);
  const [options, setOptions] = useState<WoContractorOption[]>([]);
  const [loadError, setLoadError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [companyId, setCompanyId] = useState<string>(NO_CONTRACTOR);
  const [orderRef, setOrderRef] = useState("");
  const [hours, setHours] = useState<number | null>(null);
  const [cost, setCost] = useState<number | null>(null);
  const [underWarranty, setUnderWarranty] = useState(false);
  const [warrantyNote, setWarrantyNote] = useState("");

  const apply = useCallback((s: WoSubcontracting) => {
    setSaved(s);
    setCompanyId(s.external_company_id != null ? String(s.external_company_id) : NO_CONTRACTOR);
    setOrderRef(s.service_order_ref ?? "");
    setHours(s.subcontractor_hours);
    setCost(s.service_cost > 0 ? s.service_cost : null);
    setUnderWarranty(s.under_warranty);
    setWarrantyNote(s.warranty_claim_note ?? "");
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoadError(false);
    void Promise.all([getWoSubcontracting(wo.id), listWoContractorOptions()])
      .then(([s, list]) => {
        if (cancelled) return;
        apply(s);
        setOptions(list);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id, wo.row_version, apply]);

  const editable = canEdit && !FROZEN_STATUSES.has(wo.status_code ?? "");
  const picked = options.find((o) => String(o.id) === companyId) ?? null;
  // The company already on the work order stays selectable even when it became inactive.
  const savedMissing =
    saved?.external_company_id != null && !options.some((o) => o.id === saved.external_company_id);
  const dirty =
    saved != null &&
    ((saved.external_company_id != null ? String(saved.external_company_id) : NO_CONTRACTOR) !==
      companyId ||
      (saved.service_order_ref ?? "") !== orderRef.trim() ||
      saved.subcontractor_hours !== hours ||
      (saved.service_cost > 0 ? saved.service_cost : null) !== cost ||
      saved.under_warranty !== underWarranty ||
      (saved.warranty_claim_note ?? "") !== (underWarranty ? warrantyNote.trim() : ""));

  const save = async () => {
    if (!saved) return;
    setSaving(true);
    try {
      const next = await setWoSubcontracting({
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        external_company_id: companyId === NO_CONTRACTOR ? null : Number(companyId),
        service_order_ref: orderRef.trim() === "" ? null : orderRef.trim(),
        subcontractor_hours: hours,
        service_cost: cost,
        under_warranty: underWarranty,
        warranty_claim_note:
          underWarranty && warrantyNote.trim() !== "" ? warrantyNote.trim() : null,
      });
      apply(next);
      await refreshActiveWo();
    } catch {
      // The global IPC toast reports the reason; the card keeps what was typed.
    } finally {
      setSaving(false);
    }
  };

  if (loadError) {
    return <p className="text-sm text-text-danger">{t("subcontracting.loadError")}</p>;
  }
  if (!saved) return null;

  const contract = picked ?? saved;

  return (
    <section
      className="space-y-3 rounded-lg border border-surface-border bg-surface-1 p-4"
      aria-label={t("subcontracting.title")}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("subcontracting.title")}</h3>
        {inRouter ? (
          <Link className="text-xs underline" to="/personnel?tab=companies">
            {t("subcontracting.manageContractors")}
          </Link>
        ) : null}
      </div>
      <p className="text-xs text-text-muted">{t("subcontracting.hint")}</p>
      <div className="grid gap-3 sm:grid-cols-2">
        <FormField name="wo-sub-company" label={t("subcontracting.company")}>
          <Select value={companyId} onValueChange={setCompanyId} disabled={!editable}>
            <SelectTrigger id="wo-sub-company">
              <SelectValue placeholder={t("subcontracting.noContractor")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_CONTRACTOR}>{t("subcontracting.noContractor")}</SelectItem>
              {savedMissing && saved.external_company_id != null ? (
                <SelectItem value={String(saved.external_company_id)}>
                  {saved.company_name ?? String(saved.external_company_id)}
                </SelectItem>
              ) : null}
              {options.map((o) => (
                <SelectItem key={o.id} value={String(o.id)}>
                  {o.contract_number ? `${o.name} (${o.contract_number})` : o.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField name="wo-sub-ref" label={t("subcontracting.orderRef")}>
          <Input
            id="wo-sub-ref"
            value={orderRef}
            onChange={(e) => setOrderRef(e.target.value)}
            disabled={!editable}
          />
        </FormField>
        <FormField name="wo-sub-hours" label={t("subcontracting.hours")}>
          <DecimalField
            id="wo-sub-hours"
            decimals={2}
            min={0}
            placeholder="0"
            value={hours}
            onValueChange={setHours}
            disabled={!editable}
          />
        </FormField>
        <FormField name="wo-sub-cost" label={t("subcontracting.cost", { currency: currencyLabel })}>
          <DecimalField
            id="wo-sub-cost"
            decimals={2}
            min={0}
            placeholder="0.00"
            value={cost}
            onValueChange={setCost}
            disabled={!editable}
          />
        </FormField>
      </div>
      {contract.contract_state !== "none" ? (
        <div className="flex flex-wrap items-center gap-2 text-xs text-text-muted">
          <StatusChip
            label={t(`subcontracting.contractState.${contract.contract_state}`)}
            tone={CONTRACT_TONE[contract.contract_state] ?? "neutral"}
            dot
          />
          {contract.contract_end ? (
            <span>
              {t("subcontracting.contractEnd", { date: formatDate(contract.contract_end) })}
            </span>
          ) : null}
        </div>
      ) : null}
      <div className="space-y-2 border-t border-surface-border pt-3">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="h-5 w-5 accent-primary"
            checked={underWarranty}
            disabled={!editable}
            onChange={(e) => setUnderWarranty(e.target.checked)}
          />
          {t("subcontracting.warranty.label")}
        </label>
        {saved.asset_warranty_expiry ? (
          <p className="text-xs text-text-muted">
            {t("subcontracting.warranty.assetUntil", {
              date: formatDate(saved.asset_warranty_expiry),
            })}
          </p>
        ) : null}
        {underWarranty ? (
          <FormField
            name="wo-sub-warranty-note"
            label={t("subcontracting.warranty.note")}
            description={t("subcontracting.warranty.noteHint")}
          >
            <Input
              id="wo-sub-warranty-note"
              value={warrantyNote}
              onChange={(e) => setWarrantyNote(e.target.value)}
              disabled={!editable}
            />
          </FormField>
        ) : null}
      </div>
      {editable ? (
        <Button className="h-11" onClick={() => void save()} disabled={!dirty || saving}>
          {t("subcontracting.save")}
        </Button>
      ) : null}
    </section>
  );
}
