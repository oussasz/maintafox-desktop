/**
 * AssetDetailPanel.tsx
 *
 * Right-pane detail panel for a selected asset.
 * Clear hierarchy: summary (status + health), classification, technical,
 * hierarchy (parent + children), lifecycle, meters, documents.
 */

import {
  Activity,
  AlertTriangle,
  ArrowUpRight,
  FileText,
  Gauge,
  GitFork,
  Loader2,
  Pencil,
  Tag,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetBindingSummary } from "@/components/assets/AssetBindingSummary";
import { AssetDecommissionModal } from "@/components/assets/AssetDecommissionModal";
import { AssetHealthBadge } from "@/components/assets/AssetHealthBadge";
import { AssetPhotoGallery } from "@/components/assets/AssetPhotoGallery";
import { AssetQrCode } from "@/components/assets/AssetQrCode";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  listAssetDocumentLinks,
  listAssetLifecycleEvents,
  listAssetMeters,
} from "@/services/asset-lifecycle-service";
import { getAssetById, listAssetChildren, listAssetParents } from "@/services/asset-service";
import { useAssetStore } from "@/stores/asset-store";
import { toErrorMessage } from "@/utils/errors";
import type { Asset, AssetHierarchyRow, AssetLifecycleEvent, AssetMeter } from "@shared/ipc-types";

interface AssetDetailPanelProps {
  assetId: number;
  onToast?: (msg: string, variant?: "default" | "destructive") => void;
}

interface ChildLink {
  row: AssetHierarchyRow;
  asset: Asset | null;
}

export function AssetDetailPanel({ assetId, onToast }: AssetDetailPanelProps) {
  const { t } = useTranslation("equipment");

  const [asset, setAsset] = useState<Asset | null>(null);
  const [parentAsset, setParentAsset] = useState<Asset | null>(null);
  const [childLinks, setChildLinks] = useState<ChildLink[]>([]);
  const [events, setEvents] = useState<AssetLifecycleEvent[]>([]);
  const [meters, setMeters] = useState<AssetMeter[]>([]);
  const [documentCount, setDocumentCount] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showDecommission, setShowDecommission] = useState(false);
  const openEditForm = useAssetStore((s) => s.openEditForm);

  const loadDetail = useCallback(async (id: number) => {
    setLoading(true);
    setError(null);
    try {
      const [assetData, hierarchyData, parentRows, eventData, meterData, docData] =
        await Promise.all([
          getAssetById(id),
          listAssetChildren(id),
          listAssetParents(id),
          listAssetLifecycleEvents(id, 5),
          listAssetMeters(id),
          listAssetDocumentLinks(id),
        ]);

      setAsset(assetData);
      setEvents(eventData);
      setMeters(meterData);
      setDocumentCount(docData.length);

      const parentId =
        parentRows.find((r) => r.relation_type === "PARENT_CHILD")?.parent_asset_id ??
        parentRows[0]?.parent_asset_id ??
        null;
      if (parentId != null) {
        try {
          setParentAsset(await getAssetById(parentId));
        } catch {
          setParentAsset(null);
        }
      } else {
        setParentAsset(null);
      }

      const enriched: ChildLink[] = await Promise.all(
        hierarchyData.map(async (row) => ({
          row,
          asset: await getAssetById(row.child_asset_id).catch(() => null),
        })),
      );
      setChildLinks(enriched);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDetail(assetId);
  }, [assetId, loadDetail]);

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
      </div>
    );
  }

  if (error || !asset) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <p className="text-sm text-status-danger">{error ?? t("registry.detail.loadError")}</p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-auto p-4 space-y-4">
      {/* Summary — status & health prominent */}
      <Card className="border-surface-border shadow-sm">
        <CardContent className="space-y-4 pt-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0 space-y-1">
              <p className="font-mono text-xs text-text-muted">{asset.asset_code}</p>
              <h2 className="text-lg font-semibold leading-tight tracking-tight">
                {asset.asset_name}
              </h2>
            </div>
            <div className="flex flex-shrink-0 flex-wrap items-center gap-2">
              <PermissionGate permission="eq.manage">
                <Button variant="outline" size="sm" onClick={() => openEditForm(asset)}>
                  <Pencil className="mr-1.5 h-3.5 w-3.5" />
                  {t("editForm.button")}
                </Button>
              </PermissionGate>
              <AssetQrCode asset={asset} />
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 border-t border-surface-border/80 pt-3">
            <div className="flex flex-col gap-1">
              <span className="text-[10px] font-medium uppercase tracking-wide text-text-muted">
                {t("detail.fields.status")}
              </span>
              <AssetStatusBadge code={asset.status_code} size="md" />
            </div>
            <div className="h-8 w-px bg-surface-border/80" aria-hidden />
            <div className="flex flex-col gap-1">
              <span className="text-[10px] font-medium uppercase tracking-wide text-text-muted">
                {t("health.title")}
              </span>
              <AssetHealthBadge assetId={assetId} />
            </div>
          </div>

          {asset.status_code !== "DECOMMISSIONED" && asset.status_code !== "SCRAPPED" && (
            <PermissionGate permission="eq.manage">
              <Button
                variant="outline"
                size="sm"
                className="text-status-danger border-status-danger/30 hover:bg-status-danger/5"
                onClick={() => setShowDecommission(true)}
              >
                <AlertTriangle className="mr-1.5 h-3.5 w-3.5" />
                {t("decommission.action")}
              </Button>
            </PermissionGate>
          )}
        </CardContent>
      </Card>

      {/* Classification */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Tag className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("form.classification.title")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <Row label={t("detail.fields.class")} value={asset.class_name ?? asset.class_code} />
          <Row label={t("detail.fields.family")} value={asset.family_name ?? asset.family_code} />
          <Row label={t("detail.fields.criticality")}>
            <CriticalityBadge criticality={asset.criticality_code} />
          </Row>
          <Row label={t("detail.fields.site")} value={asset.org_node_name} />
        </CardContent>
      </Card>

      {/* Technical */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Tag className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("detail.sections.technical")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <Row label={t("detail.fields.manufacturer")} value={asset.manufacturer} />
          <Row label={t("detail.fields.model")} value={asset.model} />
          <Row label={t("detail.fields.serialNumber")} value={asset.serial_number} />
          <Row
            label={t("detail.fields.commissioningDate")}
            value={
              asset.commissioned_at ? new Date(asset.commissioned_at).toLocaleDateString() : null
            }
          />
        </CardContent>
      </Card>

      {/* Hierarchy */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <GitFork className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("registry.detail.hierarchy")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div>
            <p className="text-[10px] font-medium uppercase tracking-wide text-text-muted mb-1">
              {t("detail.fields.parentEquipment")}
            </p>
            {parentAsset ? (
              <p className="text-sm">
                <span className="font-mono text-xs">{parentAsset.asset_code}</span>
                <span className="text-text-muted"> — </span>
                <span>{parentAsset.asset_name}</span>
              </p>
            ) : (
              <p className="text-xs text-text-muted">{t("registry.detail.noParent")}</p>
            )}
          </div>
          <div>
            <p className="text-[10px] font-medium uppercase tracking-wide text-text-muted mb-1">
              {t("registry.detail.childAssets")}
            </p>
            {childLinks.length === 0 ? (
              <p className="text-xs text-text-muted">{t("registry.detail.noChildren")}</p>
            ) : (
              <ul className="space-y-2">
                {childLinks.map(({ row, asset: child }) => (
                  <li key={row.relation_id} className="flex items-start gap-2 text-xs">
                    <ArrowUpRight className="mt-0.5 h-3.5 w-3.5 shrink-0 text-text-muted" />
                    <div>
                      <span className="font-mono">{child?.asset_code ?? row.child_asset_id}</span>
                      {child && <span className="block text-text-muted">{child.asset_name}</span>}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Latest lifecycle events */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Activity className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("detail.sections.lifecycle")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="text-sm">
          {events.length === 0 ? (
            <p className="text-xs text-text-muted">{t("registry.detail.noEvents")}</p>
          ) : (
            <ul className="space-y-2">
              {events.map((ev) => (
                <li key={ev.id} className="flex items-start gap-2 text-xs">
                  <span className="rounded border border-surface-border px-1.5 py-0.5 font-medium text-[10px]">
                    {ev.event_type}
                  </span>
                  <span className="text-text-muted">
                    {new Date(ev.event_at).toLocaleDateString()}
                  </span>
                  {ev.notes && <span className="text-text-secondary truncate">{ev.notes}</span>}
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      {/* Meter values */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Gauge className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("detail.sections.meters")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="text-sm">
          {meters.length === 0 ? (
            <p className="text-xs text-text-muted">{t("empty.noMeters")}</p>
          ) : (
            <ul className="space-y-2">
              {meters.map((m) => (
                <li key={m.id} className="flex items-center justify-between text-xs">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{m.name}</span>
                    {m.is_primary && (
                      <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-text-secondary">
                        {t("registry.detail.primary")}
                      </span>
                    )}
                  </div>
                  <span className="text-text-muted">
                    {m.current_reading} {m.unit ?? ""}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      {/* Documents count */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <FileText className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("detail.sections.documents")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-text-muted">
            {documentCount > 0
              ? t("registry.detail.documentCount", { count: documentCount })
              : t("empty.noDocuments")}
          </p>
        </CardContent>
      </Card>

      <AssetPhotoGallery assetId={assetId} {...(onToast ? { onToast } : {})} />

      <AssetBindingSummary assetId={assetId} />

      {asset && (
        <AssetDecommissionModal
          open={showDecommission}
          asset={asset}
          onClose={() => setShowDecommission(false)}
          onDecommissioned={(updated) => {
            setShowDecommission(false);
            setAsset(updated);
            onToast?.(t("decommission.success"));
          }}
        />
      )}
    </div>
  );
}

function Row({
  label,
  value,
  mono,
  children,
}: {
  label: string;
  value?: string | null;
  mono?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-0.5">
      <span className="text-text-muted">{label}</span>
      {children ?? (
        <span className={mono ? "font-mono text-xs" : "text-right"}>{value ?? "—"}</span>
      )}
    </div>
  );
}
