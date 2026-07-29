import { Truck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DetailSectionCard } from "@/components/detail";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel, formatOrDash } from "@/lib/display";
import {
  listSupplierArticleSources,
  upsertSupplierArticleSource,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { SupplierArticleSource, SupplierArticleSourceInput } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { riskBadgeVariant } from "./supplier-sourcing";

/** Minimal article shape needed to pick a sourcing counterpart. */
export interface SourceArticleOption {
  id: number;
  article_code: string;
  article_name: string;
}

/** Minimal supplier shape needed to pick a sourcing counterpart. */
export interface SourceSupplierOption {
  id: number;
  code: string;
  name: string;
}

export interface SupplierArticleSourcesSectionProps {
  /** Supplier-side view: lists every article this supplier can provide. */
  supplierId?: number | null;
  /** Article-side view: lists every supplier that can provide this article. */
  articleId?: number | null;
  /** Selectable articles for the article-side of the add form (supplier view). */
  articles?: SourceArticleOption[];
  /** Selectable suppliers for the supplier-side of the add form (article view). */
  suppliers?: SourceSupplierOption[];
  onChanged?: () => void;
  /** Bump to reload after a sibling view mutated the same sources. */
  refreshToken?: number;
}

interface SourceFormState {
  /** Existing row being edited, or `null` when adding. */
  editing: SupplierArticleSource | null;
  counterpartId: number;
  priority: string;
  leadTimeDays: string;
  unitPriceHint: string;
  minOrderQty: string;
  supplierArticleCode: string;
  isPreferred: boolean;
  isActive: boolean;
}

const EMPTY_FORM: SourceFormState = {
  editing: null,
  counterpartId: 0,
  priority: "",
  leadTimeDays: "",
  unitPriceHint: "",
  minOrderQty: "",
  supplierArticleCode: "",
  isPreferred: false,
  isActive: true,
};

function parseOptionalNumber(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

/** Full input payload for a row, so partial mutations never drop stored values. */
function toInput(source: SupplierArticleSource): SupplierArticleSourceInput {
  return {
    supplier_id: source.supplier_id,
    article_id: source.article_id,
    is_preferred: source.is_preferred === 1,
    priority: source.priority,
    lead_time_days: source.lead_time_days,
    unit_price_hint: source.unit_price_hint,
    min_order_qty: source.min_order_qty,
    supplier_article_code: source.supplier_article_code,
    is_active: source.is_active === 1,
  };
}

export function SupplierArticleSourcesSection({
  supplierId,
  articleId,
  articles,
  suppliers,
  onChanged,
  refreshToken,
}: SupplierArticleSourcesSectionProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatDate, formatDecimal, formatNumber } = useFormatters();

  const mode: "article" | "supplier" = articleId != null ? "article" : "supplier";

  const [sources, setSources] = useState<SupplierArticleSource[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [formOpen, setFormOpen] = useState(false);
  const [form, setForm] = useState<SourceFormState>(EMPTY_FORM);

  const load = useCallback(async () => {
    if (supplierId == null && articleId == null) {
      setSources([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setSources(await listSupplierArticleSources(supplierId ?? null, articleId ?? null));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [articleId, supplierId]);

  useEffect(() => {
    void load();
  }, [load, refreshToken]);

  /**
   * Counterparts available to add. Already-linked pairs are excluded because the
   * backend upsert would silently overwrite the existing row — those are edited
   * (including reactivation) through the row actions instead.
   */
  const counterpartOptions = useMemo(() => {
    const linked = new Set(sources.map((s) => (mode === "article" ? s.supplier_id : s.article_id)));
    if (mode === "article") {
      return (suppliers ?? [])
        .filter((s) => !linked.has(s.id))
        .map((s) => ({ id: s.id, label: formatAssetLabel(s.code, s.name) }));
    }
    return (articles ?? [])
      .filter((a) => !linked.has(a.id))
      .map((a) => ({ id: a.id, label: formatAssetLabel(a.article_code, a.article_name) }));
  }, [articles, mode, sources, suppliers]);

  const openAddForm = () => {
    setForm(EMPTY_FORM);
    setFormOpen(true);
  };

  const openEditForm = (source: SupplierArticleSource) => {
    setForm({
      editing: source,
      counterpartId: mode === "article" ? source.supplier_id : source.article_id,
      priority: String(source.priority),
      leadTimeDays: source.lead_time_days != null ? String(source.lead_time_days) : "",
      unitPriceHint: source.unit_price_hint != null ? String(source.unit_price_hint) : "",
      minOrderQty: source.min_order_qty != null ? String(source.min_order_qty) : "",
      supplierArticleCode: source.supplier_article_code ?? "",
      isPreferred: source.is_preferred === 1,
      isActive: source.is_active === 1,
    });
    setFormOpen(true);
  };

  const runMutation = async (mutate: () => Promise<unknown>) => {
    setSaving(true);
    setError(null);
    try {
      await mutate();
      await load();
      onChanged?.();
      return true;
    } catch (err) {
      setError(toErrorMessage(err));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const saveForm = async () => {
    const resolvedSupplierId = mode === "article" ? form.counterpartId : (supplierId ?? 0);
    const resolvedArticleId = mode === "article" ? (articleId ?? 0) : form.counterpartId;
    if (resolvedSupplierId <= 0 || resolvedArticleId <= 0) return;

    const ok = await runMutation(() =>
      upsertSupplierArticleSource({
        supplier_id: resolvedSupplierId,
        article_id: resolvedArticleId,
        // An inactive source must never stay flagged as preferred.
        is_preferred: form.isActive && form.isPreferred,
        priority: parseOptionalNumber(form.priority),
        lead_time_days: parseOptionalNumber(form.leadTimeDays),
        unit_price_hint: parseOptionalNumber(form.unitPriceHint),
        min_order_qty: parseOptionalNumber(form.minOrderQty),
        supplier_article_code: form.supplierArticleCode.trim() || null,
        is_active: form.isActive,
      }),
    );
    if (ok) setFormOpen(false);
  };

  const setPreferred = (source: SupplierArticleSource) =>
    void runMutation(() => upsertSupplierArticleSource({ ...toInput(source), is_preferred: true }));

  const deactivate = (source: SupplierArticleSource) =>
    void runMutation(() =>
      upsertSupplierArticleSource({ ...toInput(source), is_active: false, is_preferred: false }),
    );

  const counterpartHeader =
    mode === "article"
      ? t("procurement.suppliers.sources.columns.supplier")
      : t("procurement.suppliers.sources.columns.article");

  const canAdd = counterpartOptions.length > 0;

  return (
    <DetailSectionCard title={t("procurement.suppliers.sources.title")} icon={Truck}>
      <div className="flex items-center justify-between">
        <p className="text-xs text-text-muted">
          {t("procurement.suppliers.sources.resultCount", { count: sources.length })}
        </p>
        <PermissionGate permission={P.INV_MANAGE}>
          <Button size="sm" variant="outline" disabled={!canAdd} onClick={openAddForm}>
            {t("procurement.suppliers.sources.add")}
          </Button>
        </PermissionGate>
      </div>

      {error ? <p className="text-xs text-status-danger">{error}</p> : null}

      {loading ? (
        <p className="text-xs text-text-muted">{tc("app.loading")}</p>
      ) : sources.length === 0 ? (
        <p className="text-xs text-text-muted">{t("procurement.suppliers.sources.empty")}</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="h-8 px-2 text-xs">{counterpartHeader}</TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                {t("procurement.suppliers.sources.columns.priceHint")}
              </TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                {t("procurement.suppliers.sources.columns.lastPrice")}
              </TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                {t("procurement.suppliers.sources.columns.avgPrice")}
              </TableHead>
              <TableHead className="h-8 px-2 text-xs">
                {t("procurement.suppliers.sources.columns.currency")}
              </TableHead>
              <TableHead className="h-8 px-2 text-xs">
                {t("procurement.suppliers.sources.columns.lastPurchase")}
              </TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                {t("procurement.suppliers.sources.columns.moq")}
              </TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                {t("procurement.suppliers.sources.columns.leadTime")}
              </TableHead>
              <TableHead className="h-8 px-2 text-right text-xs">
                <span className="sr-only">
                  {t("procurement.suppliers.sources.columns.actions")}
                </span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sources.map((source) => (
              <TableRow key={source.id} className={source.is_active === 1 ? "" : "opacity-60"}>
                <TableCell className="px-2 py-1.5 text-xs">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium">
                      {mode === "article"
                        ? formatAssetLabel(source.supplier_code, source.supplier_name)
                        : formatAssetLabel(source.article_code, source.article_name)}
                    </span>
                    {source.is_preferred === 1 ? (
                      <Badge className="h-4 text-[9px]">
                        {t("procurement.suppliers.sources.preferred")}
                      </Badge>
                    ) : null}
                    {source.risk_level ? (
                      <Badge
                        variant={riskBadgeVariant(source.risk_level)}
                        className="h-4 text-[9px]"
                      >
                        {t(`procurement.suppliers.risk.${source.risk_level}`, {
                          defaultValue: source.risk_level,
                        })}
                      </Badge>
                    ) : null}
                    {source.is_active === 0 ? (
                      <Badge variant="outline" className="h-4 text-[9px]">
                        {tc("status.inactive")}
                      </Badge>
                    ) : null}
                  </div>
                  {source.supplier_article_code ? (
                    <span className="text-[10px] text-text-muted">
                      {source.supplier_article_code}
                    </span>
                  ) : null}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.unit_price_hint != null ? formatDecimal(source.unit_price_hint, 2) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.last_price != null ? formatDecimal(source.last_price, 2) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.avg_price != null ? formatDecimal(source.avg_price, 2) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-xs">
                  {formatOrDash(source.currency_label)}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-xs">
                  {formatDate(source.last_purchase_at)}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.min_order_qty != null ? formatNumber(source.min_order_qty) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                  {source.lead_time_days != null ? formatNumber(source.lead_time_days) : "—"}
                </TableCell>
                <TableCell className="px-2 py-1.5 text-right">
                  <PermissionGate permission={P.INV_MANAGE}>
                    <div className="flex justify-end gap-1">
                      {source.is_active === 1 && source.is_preferred === 0 ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-1.5 text-[11px]"
                          disabled={saving}
                          onClick={() => setPreferred(source)}
                        >
                          {t("procurement.suppliers.sources.setPreferred")}
                        </Button>
                      ) : null}
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 px-1.5 text-[11px]"
                        disabled={saving}
                        onClick={() => openEditForm(source)}
                      >
                        {tc("action.edit")}
                      </Button>
                      {source.is_active === 1 ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-1.5 text-[11px] text-status-danger hover:text-status-danger"
                          disabled={saving}
                          onClick={() => deactivate(source)}
                        >
                          {t("procurement.suppliers.sources.deactivate")}
                        </Button>
                      ) : null}
                    </div>
                  </PermissionGate>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      <Dialog open={formOpen} onOpenChange={setFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>
              {form.editing
                ? t("procurement.suppliers.sources.edit")
                : t("procurement.suppliers.sources.add")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div>
              <Label htmlFor="source-counterpart">{counterpartHeader}</Label>
              {form.editing ? (
                <p id="source-counterpart" className="mt-1 text-sm font-medium">
                  {mode === "article"
                    ? formatAssetLabel(form.editing.supplier_code, form.editing.supplier_name)
                    : formatAssetLabel(form.editing.article_code, form.editing.article_name)}
                </p>
              ) : (
                <Select
                  value={String(form.counterpartId)}
                  onValueChange={(v) => setForm((prev) => ({ ...prev, counterpartId: Number(v) }))}
                >
                  <SelectTrigger id="source-counterpart" className="h-8 text-sm">
                    <SelectValue placeholder={counterpartHeader} />
                  </SelectTrigger>
                  <SelectContent>
                    {counterpartOptions.map((option) => (
                      <SelectItem key={option.id} value={String(option.id)}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <Label htmlFor="source-priority">
                  {t("procurement.suppliers.sources.fields.priority")}
                </Label>
                <Input
                  id="source-priority"
                  type="number"
                  min={0}
                  value={form.priority}
                  onChange={(e) => setForm((prev) => ({ ...prev, priority: e.target.value }))}
                />
              </div>
              <div>
                <Label htmlFor="source-lead-time">
                  {t("procurement.suppliers.sources.fields.leadTime")}
                </Label>
                <Input
                  id="source-lead-time"
                  type="number"
                  min={0}
                  value={form.leadTimeDays}
                  onChange={(e) => setForm((prev) => ({ ...prev, leadTimeDays: e.target.value }))}
                />
              </div>
              <div>
                <Label htmlFor="source-price-hint">
                  {t("procurement.suppliers.sources.fields.priceHint")}
                </Label>
                <Input
                  id="source-price-hint"
                  type="number"
                  min={0}
                  step="0.01"
                  value={form.unitPriceHint}
                  onChange={(e) => setForm((prev) => ({ ...prev, unitPriceHint: e.target.value }))}
                />
              </div>
              <div>
                <Label htmlFor="source-moq">{t("procurement.suppliers.sources.fields.moq")}</Label>
                <Input
                  id="source-moq"
                  type="number"
                  min={0}
                  step="0.01"
                  value={form.minOrderQty}
                  onChange={(e) => setForm((prev) => ({ ...prev, minOrderQty: e.target.value }))}
                />
              </div>
            </div>
            <div>
              <Label htmlFor="source-supplier-article-code">
                {t("procurement.suppliers.sources.fields.supplierArticleCode")}
              </Label>
              <Input
                id="source-supplier-article-code"
                value={form.supplierArticleCode}
                onChange={(e) =>
                  setForm((prev) => ({ ...prev, supplierArticleCode: e.target.value }))
                }
              />
            </div>
            <div className="flex items-center gap-2">
              <Checkbox
                id="source-preferred"
                checked={form.isPreferred}
                disabled={!form.isActive}
                onCheckedChange={(checked) =>
                  setForm((prev) => ({ ...prev, isPreferred: checked }))
                }
              />
              <Label htmlFor="source-preferred">
                {t("procurement.suppliers.sources.fields.preferred")}
              </Label>
            </div>
            {form.editing ? (
              <div className="flex items-center gap-2">
                <Checkbox
                  id="source-active"
                  checked={form.isActive}
                  onCheckedChange={(checked) => setForm((prev) => ({ ...prev, isActive: checked }))}
                />
                <Label htmlFor="source-active">{tc("status.active")}</Label>
              </div>
            ) : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setFormOpen(false)}>
              {tc("action.cancel")}
            </Button>
            <Button
              disabled={saving || (!form.editing && form.counterpartId <= 0)}
              onClick={() => void saveForm()}
            >
              {tc("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </DetailSectionCard>
  );
}
