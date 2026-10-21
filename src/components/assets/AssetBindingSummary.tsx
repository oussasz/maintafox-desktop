/**
 * AssetBindingSummary.tsx
 *
 * Renders cross-module binding summary cards for an asset.
 * Each domain shows its count ("—" when not reported) or that the module does not report one.
 * A failed load is an error with retry, never a card of zeros.
 */

import {
  AlertTriangle,
  ClipboardList,
  FileText,
  HardHat,
  Radio,
  RefreshCw,
  Server,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { useFormatters } from "@/hooks/use-formatters";
import { listLinks } from "@/lib/list-links";
import { getAssetBindingSummary } from "@/services/asset-lifecycle-service";
import type {
  AssetBindingSummary as AssetBindingSummaryType,
  DomainBindingEntry,
} from "@shared/ipc-types";

interface AssetBindingSummaryProps {
  assetId: number;
}

/** Domain descriptor for rendering. */
interface DomainDescriptor {
  key: keyof Omit<AssetBindingSummaryType, "asset_id" | "open_wo_rule">;
  labelKey:
    | "binding.domains.di"
    | "binding.domains.wo"
    | "binding.domains.pm"
    | "binding.domains.failure"
    | "binding.domains.document"
    | "binding.domains.iot"
    | "binding.domains.erp";
  icon: React.ReactNode;
}

const ICON_CLASS = "h-3.5 w-3.5 text-text-muted";

const DOMAINS: DomainDescriptor[] = [
  {
    key: "linked_di_count",
    labelKey: "binding.domains.di",
    icon: <ClipboardList className={ICON_CLASS} />,
  },
  {
    key: "linked_wo_count",
    labelKey: "binding.domains.wo",
    icon: <Wrench className={ICON_CLASS} />,
  },
  {
    key: "linked_pm_plan_count",
    labelKey: "binding.domains.pm",
    icon: <RefreshCw className={ICON_CLASS} />,
  },
  {
    key: "linked_failure_event_count",
    labelKey: "binding.domains.failure",
    icon: <AlertTriangle className={ICON_CLASS} />,
  },
  {
    key: "linked_document_count",
    labelKey: "binding.domains.document",
    icon: <FileText className={ICON_CLASS} />,
  },
  {
    key: "linked_iot_signal_count",
    labelKey: "binding.domains.iot",
    icon: <Radio className={ICON_CLASS} />,
  },
  {
    key: "linked_erp_mapping_count",
    labelKey: "binding.domains.erp",
    icon: <Server className={ICON_CLASS} />,
  },
];

export function AssetBindingSummary({ assetId }: AssetBindingSummaryProps) {
  const { t } = useTranslation("equipment");
  const [summary, setSummary] = useState<AssetBindingSummaryType | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  // Monotonic request id: when the user switches assets quickly, a slower response for a
  // previously selected asset must not overwrite the summary of the asset now on screen.
  const latestRequest = useRef(0);

  const load = useCallback(async (id: number) => {
    const requestId = ++latestRequest.current;
    setLoading(true);
    setError(null);
    try {
      const data = await getAssetBindingSummary(id);
      if (requestId !== latestRequest.current) return;
      setSummary(data);
    } catch (err) {
      if (requestId !== latestRequest.current) return;
      setError(err);
    } finally {
      if (requestId === latestRequest.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(assetId);
  }, [assetId, load]);

  if (loading) {
    return (
      <Card>
        <CardContent className="py-2">
          <LoadingState />
        </CardContent>
      </Card>
    );
  }

  if (error != null) {
    return (
      <Card>
        <CardContent className="py-4">
          <ErrorState
            error={error}
            title={t("binding.loadError")}
            onRetry={() => void load(assetId)}
          />
        </CardContent>
      </Card>
    );
  }

  if (!summary) return null;

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center gap-2">
          <HardHat className="h-4 w-4 text-text-muted" aria-hidden />
          <CardTitle className="text-base">{t("binding.title")}</CardTitle>
        </div>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 gap-2">
          {DOMAINS.map((d) => (
            <DomainCard
              key={d.key}
              entry={summary[d.key]}
              label={t(d.labelKey)}
              icon={d.icon}
              unavailableLabel={t("binding.notImplemented")}
              {...(d.key === "linked_wo_count"
                ? { to: listLinks.workOrders({ equipmentId: assetId }) }
                : d.key === "linked_di_count"
                  ? { to: listLinks.requests({ equipmentId: assetId }) }
                  : {})}
            />
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

function DomainCard({
  entry,
  label,
  icon,
  unavailableLabel,
  to,
}: {
  entry: DomainBindingEntry;
  label: string;
  icon: React.ReactNode;
  unavailableLabel: string;
  /** List of these records for the asset (shown as a link on the count). */
  to?: string;
}) {
  const { formatNumber } = useFormatters();
  const isAvailable = entry.status === "available";

  return (
    <div className="flex items-center gap-2 rounded-md border border-border/50 px-3 py-2">
      {icon}
      <div className="flex flex-1 flex-col min-w-0">
        <span className="text-xs font-medium truncate">{label}</span>
        {isAvailable ? (
          to && entry.count != null ? (
            <Link
              to={to}
              aria-label={`${label} — ${formatNumber(entry.count)}`}
              className="w-fit rounded text-xs text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {formatNumber(entry.count)}
            </Link>
          ) : (
            <span className="text-xs text-text-muted">
              {entry.count == null ? "—" : formatNumber(entry.count)}
            </span>
          )
        ) : (
          <Badge variant="outline" className="mt-0.5 w-fit text-xs text-text-muted">
            {unavailableLabel}
          </Badge>
        )}
      </div>
    </div>
  );
}
