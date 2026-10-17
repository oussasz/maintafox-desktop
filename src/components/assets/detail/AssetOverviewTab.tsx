/**
 * Overview tab of the asset record: identification, technical data, location and cost centre,
 * commercial / warranty data, reliability profile and the records linked to the asset.
 */

import { Activity, Building2, Cog, Receipt, Tag } from "lucide-react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetBindingSummary } from "@/components/assets/AssetBindingSummary";
import {
  WARRANTY_TONE,
  warrantyDaysLeft,
  warrantyState,
} from "@/components/assets/detail/warranty";
import { DetailFieldRow, DetailSectionCard } from "@/components/detail";
import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import type { Asset } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface AssetOverviewTabProps {
  asset: Asset;
  onEditCommercial: () => void;
}

export function AssetOverviewTab({ asset, onEditCommercial }: AssetOverviewTabProps) {
  const { t } = useTranslation("equipment");
  const { formatDate, formatCurrency, formatDecimal } = useFormatters();
  const today = new Date();
  const warranty = warrantyState(asset.warranty_expiry_date, today);
  const daysLeft = warrantyDaysLeft(asset.warranty_expiry_date, today);
  const ownCostCenter = asset.cost_center_code ?? null;
  const effectiveCostCenter = ownCostCenter ?? asset.org_cost_center_code ?? null;

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <DetailSectionCard title={t("form.classification.title")} icon={Tag}>
        <DetailFieldRow label={t("detail.fields.code")} value={asset.asset_code} mono />
        <DetailFieldRow label={t("detail.fields.class")} value={asset.class_name} />
        <DetailFieldRow label={t("detail.fields.family")} value={asset.family_name} />
        <DetailFieldRow label={t("detail.fields.subfamily")} value={asset.subfamily_name} />
        <DetailFieldRow
          label={t("detail.fields.maintainableBoundary")}
          value={asset.maintainable_boundary ? t("detail.yes") : t("detail.no")}
        />
      </DetailSectionCard>

      <DetailSectionCard title={t("detail.sections.technical")} icon={Cog}>
        <DetailFieldRow label={t("detail.fields.manufacturer")} value={asset.manufacturer} />
        <DetailFieldRow label={t("detail.fields.model")} value={asset.model} />
        <DetailFieldRow label={t("detail.fields.serialNumber")} value={asset.serial_number} mono />
        <DetailFieldRow
          label={t("detail.fields.commissioningDate")}
          value={asset.commissioned_at ? formatDate(asset.commissioned_at) : null}
        />
        {asset.decommissioned_at ? (
          <DetailFieldRow
            label={t("detail.fields.decommissionedAt")}
            value={formatDate(asset.decommissioned_at)}
          />
        ) : null}
      </DetailSectionCard>

      <DetailSectionCard title={t("detail.sections.location")} icon={Building2}>
        <DetailFieldRow label={t("detail.fields.site")} value={asset.org_node_name} />
        <DetailFieldRow label={t("commercial.costCenter")}>
          {effectiveCostCenter ? (
            <span className="text-right">
              <span className="font-mono text-xs">{effectiveCostCenter}</span>
              <span className="block text-[11px] text-text-muted">
                {ownCostCenter
                  ? t("commercial.costCenterOwn")
                  : t("commercial.costCenterInherited")}
              </span>
            </span>
          ) : (
            <span className="text-text-muted">{t("commercial.costCenterNone")}</span>
          )}
        </DetailFieldRow>
      </DetailSectionCard>

      <DetailSectionCard title={t("commercial.title")} icon={Receipt}>
        <DetailFieldRow
          label={t("commercial.purchaseDate")}
          value={asset.purchase_date ? formatDate(asset.purchase_date) : null}
        />
        <DetailFieldRow label={t("commercial.warrantyExpiry")}>
          {warranty === "none" ? (
            <span>—</span>
          ) : (
            <span className="flex flex-col items-end gap-0.5">
              <span>{formatDate(asset.warranty_expiry_date ?? null)}</span>
              <StatusChip
                tone={WARRANTY_TONE[warranty]}
                label={
                  warranty === "expired"
                    ? t("warranty.expiredShort")
                    : t("warranty.daysLeft", { count: daysLeft ?? 0 })
                }
              />
            </span>
          )}
        </DetailFieldRow>
        <DetailFieldRow
          label={t("commercial.replacementValue")}
          value={asset.replacement_value != null ? formatCurrency(asset.replacement_value) : null}
        />
        {asset.notes ? (
          <p className="whitespace-pre-wrap rounded-md bg-surface-2 p-2 text-xs">{asset.notes}</p>
        ) : null}
        <PermissionGate permission={P.EQ_MANAGE}>
          <div className="flex justify-end pt-1">
            <Button variant="outline" className="h-9 gap-1.5" onClick={onEditCommercial}>
              <Receipt className="h-4 w-4" aria-hidden />
              {t("commercial.edit")}
            </Button>
          </div>
        </PermissionGate>
      </DetailSectionCard>

      <DetailSectionCard title={t("rams.title")} icon={Activity}>
        <DetailFieldRow
          label={t("rams.schedule")}
          value={asset.rams_schedule_reference_value_name ?? t("rams.scheduleNone")}
        />
        <DetailFieldRow
          label={t("rams.utilization")}
          value={
            Number.isFinite(asset.rams_utilization_factor)
              ? formatDecimal(asset.rams_utilization_factor, 2)
              : null
          }
        />
        <p className="text-xs text-text-muted">{t("rams.hint")}</p>
      </DetailSectionCard>

      <div className="lg:col-span-2">
        <AssetBindingSummary assetId={asset.id} />
      </div>
    </div>
  );
}
