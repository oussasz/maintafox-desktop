/**
 * AssetDetailDialog.tsx
 *
 * Asset record (UX-DW-001 detail dialog, OT/DI pattern): header with code, name, status,
 * criticality, location and warranty; actions to raise an OT or a DI, edit, decommission;
 * tabs for overview, structure, maintenance (costs, OT/DI/PM), meters, spare parts,
 * documents & photos, criticality and history.
 *
 * Opened by selecting an asset in the registry (`asset-search-store.selectedResultId`) or by
 * the `?assetId=` deep link.
 */

import {
  AlertTriangle,
  ClipboardList,
  FileText,
  Gauge,
  GitFork,
  History,
  Info,
  MoreHorizontal,
  Package,
  Pencil,
  Receipt,
  RotateCcw,
  ShieldAlert,
  Wrench,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetDecommissionModal } from "@/components/assets/AssetDecommissionModal";
import { AssetHealthBadge } from "@/components/assets/AssetHealthBadge";
import { AssetQrCode } from "@/components/assets/AssetQrCode";
import { AssetRecommissionDialog } from "@/components/assets/AssetRecommissionDialog";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { CriticalityReviewMark } from "@/components/assets/CriticalityReviewMark";
import { isRetiredStatus } from "@/components/assets/asset-tones";
import { AssetCommercialDialog } from "@/components/assets/detail/AssetCommercialDialog";
import { AssetCriticalityTab } from "@/components/assets/detail/AssetCriticalityTab";
import { AssetDocumentsTab } from "@/components/assets/detail/AssetDocumentsTab";
import { AssetMaintenanceTab } from "@/components/assets/detail/AssetMaintenanceTab";
import { AssetMetersTab } from "@/components/assets/detail/AssetMetersTab";
import { AssetOverviewTab } from "@/components/assets/detail/AssetOverviewTab";
import { AssetSparePartsTab } from "@/components/assets/detail/AssetSparePartsTab";
import { AssetStructureTab } from "@/components/assets/detail/AssetStructureTab";
import { useLoad } from "@/components/assets/detail/use-load";
import { WARRANTY_TONE, warrantyState } from "@/components/assets/detail/warranty";
import { AssetHistoryView } from "@/components/assets/history/AssetHistoryView";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { assetToSearchResult } from "@/lib/asset-to-search-result";
import { getAssetById } from "@/services/asset-service";
import { pushAppToast } from "@/store/app-toast-store";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import { useAssetStore } from "@/stores/asset-store";
import { useDiStore } from "@/stores/di-store";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";
import { useWoStore } from "@/stores/wo-store";
import type { Asset } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const TAB_IDS = [
  "overview",
  "structure",
  "maintenance",
  "meters",
  "spares",
  "documents",
  "criticality",
  "history",
] as const;
type AssetTab = (typeof TAB_IDS)[number];

const TAB_ICON: Record<AssetTab, typeof Info> = {
  overview: Info,
  structure: GitFork,
  maintenance: Wrench,
  meters: Gauge,
  spares: Package,
  documents: FileText,
  criticality: ShieldAlert,
  history: History,
};

export function AssetDetailDialog() {
  const { t } = useTranslation("equipment");
  const { formatDate } = useFormatters();
  const assetId = useAssetSearchStore((s) => s.selectedResultId);
  const selectAsset = useAssetSearchStore((s) => s.selectAsset);
  const runSearch = useAssetSearchStore((s) => s.runSearch);
  const openEditForm = useAssetStore((s) => s.openEditForm);
  const showEditForm = useAssetStore((s) => s.showEditForm);
  const openCreateWo = useWoStore((s) => s.openCreateForm);
  const openCreateDi = useDiStore((s) => s.openCreateForm);

  const [tab, setTab] = useState<AssetTab>("overview");
  const [showDecommission, setShowDecommission] = useState(false);
  const [showCommercial, setShowCommercial] = useState(false);
  const [showRecommission, setShowRecommission] = useState(false);

  const load = useMemo(() => (assetId == null ? null : () => getAssetById(assetId)), [assetId]);
  const {
    data: asset,
    error,
    loading,
    reload,
    setData,
  } = useLoad<Asset>(load, String(assetId ?? ""));

  // Another asset opens on its overview.
  useEffect(() => {
    setTab("overview");
  }, [assetId]);

  // The identity form is a separate dialog: refresh the record once it closes.
  const editWasOpen = useRef(false);
  useEffect(() => {
    if (editWasOpen.current && !showEditForm) void reload();
    editWasOpen.current = showEditForm;
  }, [showEditForm, reload]);

  const close = useCallback(() => selectAsset(null), [selectAsset]);

  const onAssetChanged = useCallback(
    (next: Asset) => {
      setData(next);
      void runSearch();
    },
    [setData, runSearch],
  );

  const { catalog: taxonomy } = useEquipmentTaxonomy();
  const warranty = warrantyState(asset?.warranty_expiry_date, new Date());
  // Retired = the status carries the `retired` tag in EQUIPMENT.STATUS.
  const retired = isRetiredStatus(taxonomy, asset?.status_code);

  return (
    <Dialog
      open={assetId != null}
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      <DialogContent
        className="flex max-h-[90vh] w-full max-w-5xl flex-col gap-0 p-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        {loading && !asset ? (
          <>
            <DialogHeader className="px-6 pb-3 pt-5">
              <DialogTitle>{t("detail.loadingTitle")}</DialogTitle>
              <DialogDescription className="sr-only">{t("detail.loadingTitle")}</DialogDescription>
            </DialogHeader>
            <LoadingState className="py-16" />
          </>
        ) : error || !asset ? (
          <>
            <DialogHeader className="px-6 pb-3 pt-5">
              <DialogTitle>{t("registry.detail.loadError")}</DialogTitle>
              <DialogDescription className="sr-only">
                {t("registry.detail.loadError")}
              </DialogDescription>
            </DialogHeader>
            <div className="p-6">
              <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />
            </div>
          </>
        ) : (
          <>
            <DialogHeader className="border-b border-surface-border px-6 pb-4 pt-5">
              <div className="flex items-start justify-between gap-4 pr-10">
                <div className="min-w-0 space-y-1.5">
                  <DialogTitle className="flex items-center gap-2 text-lg font-bold">
                    <span className="font-mono text-base text-text-muted">{asset.asset_code}</span>
                    <span className="truncate">{asset.asset_name}</span>
                  </DialogTitle>
                  <DialogDescription className="text-sm text-text-muted">
                    {[asset.class_name, asset.org_node_name].filter(Boolean).join(" · ") ||
                      t("detail.noClassification")}
                  </DialogDescription>
                  <div className="flex flex-wrap items-center gap-1.5 pt-1">
                    <AssetStatusBadge code={asset.status_code} size="md" />
                    <CriticalityBadge criticality={asset.criticality_code} />
                    {asset.criticality_to_review ? <CriticalityReviewMark /> : null}
                    {warranty !== "none" ? (
                      <StatusChip
                        tone={WARRANTY_TONE[warranty]}
                        label={t(`warranty.state.${warranty}`, {
                          date: formatDate(asset.warranty_expiry_date ?? null),
                        })}
                      />
                    ) : null}
                    <AssetHealthBadge assetId={asset.id} />
                  </div>
                </div>
                <div className="hidden shrink-0 sm:block">
                  <AssetQrCode asset={asset} />
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-2 pt-3">
                <PermissionGate permission={P.OT_CREATE}>
                  <Button
                    className={mfLayout.primaryAction}
                    disabled={retired}
                    onClick={() => openCreateWo(undefined, assetToSearchResult(asset))}
                  >
                    <Wrench className="h-4 w-4" aria-hidden />
                    {t("actions.createWo")}
                  </Button>
                </PermissionGate>
                <PermissionGate permission={P.DI_CREATE}>
                  <Button
                    variant="outline"
                    className="h-11 gap-1.5"
                    disabled={retired}
                    onClick={() => openCreateDi(undefined, assetToSearchResult(asset))}
                  >
                    <ClipboardList className="h-4 w-4" aria-hidden />
                    {t("actions.createDi")}
                  </Button>
                </PermissionGate>
                {retired ? (
                  <span className="text-xs text-text-muted">{t("detail.retiredHint")}</span>
                ) : null}
                {retired ? (
                  <PermissionGate permission={P.EQ_MANAGE}>
                    <Button
                      variant="outline"
                      className="h-11 gap-1.5"
                      onClick={() => setShowRecommission(true)}
                    >
                      <RotateCcw className="h-4 w-4" aria-hidden />
                      {t("recommission.action")}
                    </Button>
                  </PermissionGate>
                ) : null}
                <PermissionGate permission={P.EQ_MANAGE}>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="outline"
                        className={mfLayout.toolbarIconButton}
                        aria-label={t("actions.more")}
                      >
                        <MoreHorizontal className="h-4 w-4" aria-hidden />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="start" className="w-56">
                      <DropdownMenuItem className="gap-2" onSelect={() => openEditForm(asset)}>
                        <Pencil className="h-4 w-4" aria-hidden />
                        {t("editForm.button")}
                      </DropdownMenuItem>
                      <DropdownMenuItem className="gap-2" onSelect={() => setShowCommercial(true)}>
                        <Receipt className="h-4 w-4" aria-hidden />
                        {t("commercial.edit")}
                      </DropdownMenuItem>
                      {!retired ? (
                        <>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className="gap-2 text-status-danger focus:text-status-danger"
                            onSelect={() => setShowDecommission(true)}
                          >
                            <AlertTriangle className="h-4 w-4" aria-hidden />
                            {t("decommission.action")}
                          </DropdownMenuItem>
                        </>
                      ) : null}
                    </DropdownMenuContent>
                  </DropdownMenu>
                </PermissionGate>
              </div>
            </DialogHeader>

            <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
              <Tabs value={tab} onValueChange={(v) => setTab(v as AssetTab)}>
                <TabsList className="h-auto w-full flex-wrap justify-start">
                  {TAB_IDS.map((id) => {
                    const Icon = TAB_ICON[id];
                    return (
                      <TabsTrigger key={id} value={id} className="min-h-[36px] gap-1.5 text-xs">
                        <Icon className="h-3.5 w-3.5" aria-hidden />
                        {t(`detail.tabs.${id}`)}
                      </TabsTrigger>
                    );
                  })}
                </TabsList>
                <TabsContent value="overview" className="pt-3">
                  <AssetOverviewTab
                    asset={asset}
                    onEditCommercial={() => setShowCommercial(true)}
                  />
                </TabsContent>
                <TabsContent value="structure" className="pt-3">
                  <AssetStructureTab asset={asset} onOpenAsset={selectAsset} />
                </TabsContent>
                <TabsContent value="maintenance" className="pt-3">
                  <AssetMaintenanceTab asset={asset} />
                </TabsContent>
                <TabsContent value="meters" className="pt-3">
                  <AssetMetersTab asset={asset} />
                </TabsContent>
                <TabsContent value="spares" className="pt-3">
                  <AssetSparePartsTab asset={asset} />
                </TabsContent>
                <TabsContent value="documents" className="pt-3">
                  <AssetDocumentsTab asset={asset} />
                </TabsContent>
                <TabsContent value="criticality" className="pt-3">
                  <AssetCriticalityTab asset={asset} onAssetChanged={onAssetChanged} />
                </TabsContent>
                <TabsContent value="history" className="pt-3">
                  <AssetHistoryView assetId={asset.id} />
                </TabsContent>
              </Tabs>
            </div>

            <div className="flex justify-end border-t border-surface-border px-6 py-3">
              <Button variant="outline" className="h-11 gap-1.5" onClick={close}>
                <X className="h-4 w-4" aria-hidden />
                {t("detail.close")}
              </Button>
            </div>

            <AssetDecommissionModal
              open={showDecommission}
              asset={asset}
              onClose={() => setShowDecommission(false)}
              onDecommissioned={(updated) => {
                setShowDecommission(false);
                onAssetChanged(updated);
              }}
            />
            <AssetRecommissionDialog
              open={showRecommission}
              asset={asset}
              onClose={() => setShowRecommission(false)}
              onRecommissioned={async (statusLabel) => {
                setShowRecommission(false);
                pushAppToast({
                  title: t("recommission.done", { code: asset.asset_code, status: statusLabel }),
                  variant: "success",
                });
                await reload();
                void runSearch();
              }}
            />
            <AssetCommercialDialog
              open={showCommercial}
              asset={asset}
              onClose={() => setShowCommercial(false)}
              onSaved={(updated) => {
                setShowCommercial(false);
                onAssetChanged(updated);
              }}
            />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
