import { Scale, Star } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DetailSectionCard } from "@/components/detail";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  listSupplierArticleSources,
  upsertSupplierArticleSource,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { SupplierArticleSource } from "@shared/ipc-types";

import { computeCompositeRatings, effectiveUnitPrice, riskBadgeVariant } from "./supplier-sourcing";

const MAX_STARS = 5;

export interface SupplierComparisonCardProps {
  articleId: number;
  onPreferredChanged?: () => void;
  /** Bump to reload after a sibling view mutated the same sources. */
  refreshToken?: number;
}

/**
 * Side-by-side sourcing comparison for an article. Only meaningful when the
 * article has at least two active sources, so it renders nothing otherwise.
 */
export function SupplierComparisonCard({
  articleId,
  onPreferredChanged,
  refreshToken,
}: SupplierComparisonCardProps) {
  const { t } = useTranslation("inventory");
  const { formatDecimal, formatNumber } = useFormatters();

  const [sources, setSources] = useState<SupplierArticleSource[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const rows = await listSupplierArticleSources(null, articleId);
      setSources(rows.filter((row) => row.is_active === 1));
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [articleId]);

  useEffect(() => {
    void load();
  }, [load, refreshToken]);

  const ratings = useMemo(() => {
    const byId = new Map(computeCompositeRatings(sources).map((r) => [r.sourceId, r.stars]));
    return byId;
  }, [sources]);

  const setPreferred = async (source: SupplierArticleSource) => {
    setSaving(true);
    setError(null);
    try {
      await upsertSupplierArticleSource({
        supplier_id: source.supplier_id,
        article_id: source.article_id,
        is_preferred: true,
        priority: source.priority,
        lead_time_days: source.lead_time_days,
        unit_price_hint: source.unit_price_hint,
        min_order_qty: source.min_order_qty,
        supplier_article_code: source.supplier_article_code,
        is_active: true,
      });
      await load();
      onPreferredChanged?.();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  if (sources.length < 2) return null;

  return (
    <DetailSectionCard title={t("procurement.suppliers.comparison.title")} icon={Scale}>
      <p className="text-xs text-text-muted">{t("procurement.suppliers.comparison.hint")}</p>

      {error ? <p className="text-xs text-status-danger">{error}</p> : null}

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="h-8 px-2 text-xs">
              {t("procurement.suppliers.comparison.columns.supplier")}
            </TableHead>
            <TableHead className="h-8 px-2 text-right text-xs">
              {t("procurement.suppliers.comparison.columns.leadTime")}
            </TableHead>
            <TableHead className="h-8 px-2 text-right text-xs">
              {t("procurement.suppliers.comparison.columns.lastPrice")}
            </TableHead>
            <TableHead className="h-8 px-2 text-xs">
              {t("procurement.suppliers.comparison.columns.risk")}
            </TableHead>
            <TableHead className="h-8 px-2 text-xs">
              {t("procurement.suppliers.comparison.columns.rating")}
            </TableHead>
            <TableHead className="h-8 px-2 text-right text-xs">
              <span className="sr-only">
                {t("procurement.suppliers.comparison.columns.actions")}
              </span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sources.map((source) => {
            const stars = ratings.get(source.id) ?? 0;
            const price = effectiveUnitPrice(source);
            return (
              <TableRow key={source.id}>
                <TableCell className="px-2 py-1.5 text-xs">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium">
                      {formatAssetLabel(source.supplier_code, source.supplier_name)}
                    </span>
                    {source.is_preferred === 1 ? (
                      <Badge className="h-4 text-[9px]">
                        {t("procurement.suppliers.sources.preferred")}
                      </Badge>
                    ) : null}
                  </div>
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.lead_time_days != null ? formatNumber(source.lead_time_days) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {price != null ? formatDecimal(price, 2) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-xs">
                  {source.risk_level ? (
                    <Badge variant={riskBadgeVariant(source.risk_level)} className="h-4 text-[9px]">
                      {t(`procurement.suppliers.risk.${source.risk_level}`, {
                        defaultValue: source.risk_level,
                      })}
                    </Badge>
                  ) : (
                    "—"
                  )}
                </TableCell>
                <TableCell className="px-2 py-1.5">
                  <span
                    className="flex items-center gap-0.5"
                    aria-label={t("procurement.suppliers.comparison.ratingValue", {
                      stars,
                      max: MAX_STARS,
                    })}
                  >
                    {Array.from({ length: MAX_STARS }, (_, index) => (
                      <Star
                        key={index}
                        aria-hidden
                        className={cn(
                          "h-3 w-3",
                          index < stars
                            ? "fill-status-warning text-status-warning"
                            : "text-text-muted",
                        )}
                      />
                    ))}
                  </span>
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right">
                  <PermissionGate permission="inv.manage">
                    {source.is_preferred === 0 ? (
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-6 px-1.5 text-[11px]"
                        disabled={saving}
                        onClick={() => void setPreferred(source)}
                      >
                        {t("procurement.suppliers.sources.setPreferred")}
                      </Button>
                    ) : null}
                  </PermissionGate>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </DetailSectionCard>
  );
}
