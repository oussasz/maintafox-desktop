/**
 * AssetBindingSummary.tsx
 *
 * Renders cross-module binding summary cards for an asset.
 * Each domain shows either a live count or a "not implemented" placeholder.
 */

import {
  AlertTriangle,
  ClipboardList,
  FileText,
  HardHat,
  Loader2,
  Radio,
  RefreshCw,
  Server,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { getAssetBindingSummary } from "@/services/asset-lifecycle-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  AssetBindingSummary as AssetBindingSummaryType,
  DomainBindingEntry,
} from "@shared/ipc-types";

interface AssetBindingSummaryProps {
  assetId: number;
}

/** Domain descriptor for rendering. */
interface DomainDescriptor {
  key: keyof Omit<AssetBindingSummaryType, "asset_id">;
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
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (id: number) => {
    setLoading(true);
    setError(null);
    try {
      const data = await getAssetBindingSummary(id);
      setSummary(data);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(assetId);
  }, [assetId, load]);

  if (loading) {
    return (
      <Card>
        <CardContent className="flex items-center justify-center py-6">
          <Loader2 className="h-4 w-4 animate-spin text-text-muted" />
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <Card>
        <CardContent className="py-4">
          <p className="text-xs text-status-danger">{error}</p>
        </CardContent>
      </Card>
    );
  }

  if (!summary) return null;

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center gap-2">
          <HardHat className="h-4 w-4 text-text-muted" />
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
}: {
  entry: DomainBindingEntry;
  label: string;
  icon: React.ReactNode;
  unavailableLabel: string;
}) {
  const isAvailable = entry.status === "available";

  return (
    <div className="flex items-center gap-2 rounded-md border border-border/50 px-3 py-2">
      {icon}
      <div className="flex flex-1 flex-col min-w-0">
        <span className="text-xs font-medium truncate">{label}</span>
        {isAvailable ? (
          <span className="text-xs text-text-muted">{entry.count ?? 0}</span>
        ) : (
          <Badge variant="outline" className="w-fit text-[10px] text-text-muted mt-0.5">
            {unavailableLabel}
          </Badge>
        )}
      </div>
    </div>
  );
}
