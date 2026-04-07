/**
 * AssetDetailPanel.tsx
 *
 * Right-pane detail panel for a selected asset.
 * Shows identity block, hierarchy block, latest lifecycle events,
 * latest meter values, and document link count.
 */

import { Activity, ArrowUpRight, FileText, Gauge, GitFork, Loader2, Tag } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetBindingSummary } from "@/components/assets/AssetBindingSummary";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  listAssetLifecycleEvents,
  listAssetMeters,
  listAssetDocumentLinks,
} from "@/services/asset-lifecycle-service";
import { getAssetById, listAssetChildren } from "@/services/asset-service";
import { toErrorMessage } from "@/utils/errors";
import type { Asset, AssetHierarchyRow, AssetLifecycleEvent, AssetMeter } from "@shared/ipc-types";

interface AssetDetailPanelProps {
  assetId: number;
}

export function AssetDetailPanel({ assetId }: AssetDetailPanelProps) {
  const { t } = useTranslation("equipment");

  const [asset, setAsset] = useState<Asset | null>(null);
  const [children, setChildren] = useState<AssetHierarchyRow[]>([]);
  const [events, setEvents] = useState<AssetLifecycleEvent[]>([]);
  const [meters, setMeters] = useState<AssetMeter[]>([]);
  const [documentCount, setDocumentCount] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadDetail = useCallback(async (id: number) => {
    setLoading(true);
    setError(null);
    try {
      const [assetData, hierarchyData, eventData, meterData, docData] = await Promise.all([
        getAssetById(id),
        listAssetChildren(id),
        listAssetLifecycleEvents(id, 5),
        listAssetMeters(id),
        listAssetDocumentLinks(id),
      ]);
      setAsset(assetData);
      setChildren(hierarchyData);
      setEvents(eventData);
      setMeters(meterData);
      setDocumentCount(docData.length);
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
      {/* ── Identity block ─────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Tag className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("detail.sections.identity")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <Row label={t("detail.fields.code")} value={asset.asset_code} mono />
          <Row label={t("detail.fields.name")} value={asset.asset_name} />
          <Row label={t("detail.fields.class")} value={asset.class_name} />
          <Row label={t("detail.fields.family")} value={asset.family_name} />
          <Row label={t("detail.fields.criticality")} value={asset.criticality_code} />
          <Row label={t("detail.fields.status")}>
            <Badge variant="outline" className="text-xs">
              {asset.status_code}
            </Badge>
          </Row>
          <Row label={t("detail.fields.site")} value={asset.org_node_name} />
          <Row label={t("detail.fields.manufacturer")} value={asset.manufacturer} />
          <Row label={t("detail.fields.model")} value={asset.model} />
          <Row label={t("detail.fields.serialNumber")} value={asset.serial_number} />
        </CardContent>
      </Card>

      {/* ── Hierarchy block ────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <GitFork className="h-4 w-4 text-text-muted" />
            <CardTitle className="text-base">{t("registry.detail.hierarchy")}</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="text-sm">
          {children.length === 0 ? (
            <p className="text-xs text-text-muted">{t("registry.detail.noChildren")}</p>
          ) : (
            <ul className="space-y-1">
              {children.map((c) => (
                <li key={c.relation_id} className="flex items-center gap-2 text-xs">
                  <ArrowUpRight className="h-3 w-3 text-text-muted" />
                  <span className="font-mono">{c.child_asset_id}</span>
                  <span className="text-text-muted">({c.relation_type})</span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      {/* ── Latest lifecycle events ────────────────────────────────── */}
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
                  <Badge variant="outline" className="shrink-0 text-[10px]">
                    {ev.event_type}
                  </Badge>
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

      {/* ── Meter values ───────────────────────────────────────────── */}
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
                      <Badge variant="secondary" className="text-[10px]">
                        {t("registry.detail.primary")}
                      </Badge>
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

      {/* ── Documents count ────────────────────────────────────────── */}
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

      {/* ── Cross-module binding summary ───────────────────────────── */}
      <AssetBindingSummary assetId={assetId} />
    </div>
  );
}

// ── Row helper ────────────────────────────────────────────────────────────────

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
    <div className="flex items-center justify-between py-0.5">
      <span className="text-text-muted">{label}</span>
      {children ?? <span className={mono ? "font-mono text-xs" : ""}>{value ?? "—"}</span>}
    </div>
  );
}
