/**
 * Spare parts tab (equipment BOM): the stock articles the asset needs, the quantity installed,
 * where they sit, which ones are critical spares, and the stock on hand right now.
 */

import { Package, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { useLoad } from "@/components/assets/detail/use-load";
import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import {
  addAssetSparePart,
  listAssetSpareParts,
  removeAssetSparePart,
  updateAssetSparePart,
} from "@/services/asset-registry-service";
import { listInventoryArticles } from "@/services/inventory-service";
import type { Asset, AssetSparePart, InventoryArticle, SparePartInput } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Backend limits (`spare_parts::POSITION_LABEL_MAX_CHARS`, `NOTES_MAX_CHARS`). */
const POSITION_MAX_CHARS = 120;
const NOTES_MAX_CHARS = 1000;
/** Articles offered per search in the picker. */
const ARTICLE_RESULTS = 20;
const ROW_ACTION = "h-9 w-9 p-0";

type StockState = "ok" | "short" | "out";

function stockState(line: AssetSparePart): StockState {
  if (line.available_qty <= 0) return "out";
  if (line.available_qty < line.quantity) return "short";
  return "ok";
}

function stockTone(state: StockState, critical: boolean): Tone {
  if (state === "ok") return "success";
  if (state === "out" && critical) return "danger";
  return "warning";
}

export function AssetSparePartsTab({ asset }: { asset: Asset }) {
  const { t } = useTranslation("equipment");
  const { formatDecimal } = useFormatters();
  const { can } = usePermissions();
  const load = useMemo(() => () => listAssetSpareParts(asset.id), [asset.id]);
  const { data, error, loading, reload } = useLoad(load, String(asset.id));
  const [editing, setEditing] = useState<AssetSparePart | "new" | null>(null);
  const [removing, setRemoving] = useState<AssetSparePart | null>(null);
  const canPickArticles = can(P.INV_VIEW);
  const navigate = useNavigate();

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  const criticalShort = data.filter((l) => l.is_critical_spare && stockState(l) !== "ok").length;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-text-muted">
          {criticalShort > 0
            ? t("spares.criticalShort", { count: criticalShort })
            : t("spares.summary", { count: data.length })}
        </p>
        <PermissionGate permission={P.EQ_MANAGE}>
          <div className="flex items-center gap-2">
            {!canPickArticles ? (
              <span className="text-xs text-text-muted">{t("spares.noInventoryPermission")}</span>
            ) : null}
            <Button
              variant="outline"
              className="h-11 gap-1.5"
              disabled={!canPickArticles}
              onClick={() => setEditing("new")}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("spares.add")}
            </Button>
          </div>
        </PermissionGate>
      </div>

      {data.length === 0 ? (
        <EmptyState
          icon={<Package className="h-8 w-8" aria-hidden />}
          title={t("spares.empty")}
          description={t("spares.emptyHint")}
        />
      ) : (
        <div className="overflow-x-auto rounded-md border border-surface-border">
          <table className="w-full text-sm">
            <caption className="sr-only">{t("spares.caption", { code: asset.asset_code })}</caption>
            <thead>
              <tr className="bg-surface-1 text-left text-xs text-text-muted">
                <th className="px-3 py-2 font-medium">{t("spares.article")}</th>
                <th className="px-3 py-2 font-medium">{t("spares.position")}</th>
                <th className="px-3 py-2 text-right font-medium">{t("spares.quantity")}</th>
                <th className="px-3 py-2 font-medium">{t("spares.stock")}</th>
                <th className="px-3 py-2">
                  <span className="sr-only">{t("spares.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {data.map((line) => {
                const state = stockState(line);
                return (
                  <tr key={line.id} className="border-t border-surface-border">
                    <td className="px-3 py-2">
                      {canPickArticles ? (
                        <Button
                          variant="link"
                          className="h-auto min-h-[32px] p-0 font-mono text-xs"
                          aria-label={t("spares.openArticle", { code: line.article_code })}
                          onClick={() => void navigate(`/inventory?articleId=${line.article_id}`)}
                        >
                          {line.article_code}
                        </Button>
                      ) : (
                        <span className="block font-mono text-xs text-text-muted">
                          {line.article_code}
                        </span>
                      )}
                      <span className="block">{line.article_name}</span>
                      <span className="flex flex-wrap gap-1 pt-0.5">
                        {line.is_critical_spare ? (
                          <StatusChip tone="attention" label={t("spares.critical")} />
                        ) : null}
                        {!line.article_is_active ? (
                          <StatusChip tone="neutral" label={t("spares.articleInactive")} />
                        ) : null}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-xs">{line.position_label ?? "—"}</td>
                    <td className="px-3 py-2 text-right tabular-nums">
                      {formatDecimal(line.quantity, 2)} {line.unit_label ?? ""}
                    </td>
                    <td className="px-3 py-2">
                      <StatusChip
                        tone={stockTone(state, line.is_critical_spare)}
                        label={t(`spares.stockState.${state}`, {
                          available: formatDecimal(line.available_qty, 2),
                        })}
                      />
                      <span className="block pt-0.5 text-[11px] text-text-muted">
                        {t("spares.onHand", { qty: formatDecimal(line.on_hand_qty, 2) })}
                      </span>
                    </td>
                    <td className="px-3 py-2">
                      <PermissionGate permission={P.EQ_MANAGE}>
                        <div className="flex justify-end gap-1">
                          <Button
                            variant="ghost"
                            className={ROW_ACTION}
                            aria-label={t("spares.editLine", { code: line.article_code })}
                            onClick={() => setEditing(line)}
                          >
                            <Pencil className="h-4 w-4" aria-hidden />
                          </Button>
                          <Button
                            variant="ghost"
                            className={ROW_ACTION}
                            aria-label={t("spares.removeLine", { code: line.article_code })}
                            onClick={() => setRemoving(line)}
                          >
                            <Trash2 className="h-4 w-4" aria-hidden />
                          </Button>
                        </div>
                      </PermissionGate>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {editing ? (
        <SparePartDialog
          asset={asset}
          line={editing === "new" ? null : editing}
          existingArticleIds={data.map((l) => l.article_id)}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void reload();
          }}
        />
      ) : null}
      <ConfirmDialog
        open={removing != null}
        onOpenChange={(open) => {
          if (!open) setRemoving(null);
        }}
        title={t("spares.removeTitle")}
        description={t("spares.removeDescription", {
          code: removing?.article_code ?? "",
          asset: asset.asset_code,
        })}
        confirmLabel={t("spares.remove")}
        destructive
        onConfirm={async () => {
          if (!removing) return;
          await removeAssetSparePart(removing.id, removing.row_version);
          setRemoving(null);
          await reload();
        }}
      />
    </div>
  );
}

function SparePartDialog({
  asset,
  line,
  existingArticleIds,
  onClose,
  onSaved,
}: {
  asset: Asset;
  line: AssetSparePart | null;
  existingArticleIds: number[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("equipment");
  const formId = useId();
  const [search, setSearch] = useState("");
  const debounced = useDebouncedValue(search, 300);
  const [articles, setArticles] = useState<InventoryArticle[]>([]);
  const [article, setArticle] = useState<InventoryArticle | null>(null);
  const [quantity, setQuantity] = useState(line ? String(line.quantity) : "1");
  const [position, setPosition] = useState(line?.position_label ?? "");
  const [critical, setCritical] = useState(line?.is_critical_spare ?? false);
  const [notes, setNotes] = useState(line?.notes ?? "");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (line) return;
    let cancelled = false;
    listInventoryArticles({ search: debounced.trim() || null })
      .then((rows) => {
        if (!cancelled) setArticles(rows.slice(0, ARTICLE_RESULTS));
      })
      .catch(() => {
        // Reported by the global IPC toast.
        if (!cancelled) setArticles([]);
      });
    return () => {
      cancelled = true;
    };
  }, [debounced, line]);

  const qty = Number(quantity.replace(",", "."));
  const qtyError =
    quantity.trim() !== "" && (!Number.isFinite(qty) || qty <= 0)
      ? t("spares.errors.quantity")
      : undefined;
  const positionError =
    position.length > POSITION_MAX_CHARS
      ? t("spares.errors.tooLong", { max: POSITION_MAX_CHARS })
      : undefined;
  const notesError =
    notes.length > NOTES_MAX_CHARS
      ? t("spares.errors.tooLong", { max: NOTES_MAX_CHARS })
      : undefined;
  const invalid =
    (!line && !article) || quantity.trim() === "" || !!qtyError || !!positionError || !!notesError;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid) return;
    const input: SparePartInput = {
      quantity: qty,
      position_label: position.trim() || null,
      is_critical_spare: critical,
      notes: notes.trim() || null,
    };
    setSaving(true);
    try {
      if (line) await updateAssetSparePart(line.id, input, line.row_version);
      else if (article) await addAssetSparePart(asset.id, article.id, input);
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <EntityFormDialog
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={
        line
          ? t("spares.editTitle", { code: line.article_code })
          : t("spares.addTitle", { code: asset.asset_code })
      }
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={line ? t("commercial.save") : t("spares.add")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4 sm:grid-cols-2">
        {!line ? (
          <div className="space-y-2 sm:col-span-2">
            <FormField name={`${formId}-search`} label={t("spares.article")} required>
              <div className="relative">
                <Search
                  className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
                  aria-hidden
                />
                <Input
                  id={`${formId}-search`}
                  className="pl-8"
                  placeholder={t("spares.searchPlaceholder")}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </div>
            </FormField>
            <ul
              className="max-h-56 overflow-y-auto rounded-md border border-surface-border"
              aria-label={t("spares.article")}
            >
              {articles.length === 0 ? (
                <li className="px-3 py-2 text-sm text-text-muted">{t("spares.noArticle")}</li>
              ) : (
                articles.map((a) => {
                  const taken = existingArticleIds.includes(a.id);
                  const selected = article?.id === a.id;
                  return (
                    <li key={a.id}>
                      <button
                        type="button"
                        aria-pressed={selected}
                        disabled={taken || a.is_active === 0}
                        onClick={() => setArticle(a)}
                        className={`flex min-h-[40px] w-full items-center gap-3 px-3 py-1.5 text-left text-sm hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60 ${selected ? "bg-surface-2 font-medium" : ""}`}
                      >
                        <span className="font-mono text-xs text-text-muted">{a.article_code}</span>
                        <span className="min-w-0 flex-1 truncate">{a.article_name}</span>
                        {taken ? (
                          <span className="text-[11px] text-text-muted">
                            {t("spares.alreadyListed")}
                          </span>
                        ) : null}
                      </button>
                    </li>
                  );
                })
              )}
            </ul>
          </div>
        ) : null}
        <FormField name={`${formId}-qty`} label={t("spares.quantity")} required error={qtyError}>
          <Input
            id={`${formId}-qty`}
            inputMode="decimal"
            value={quantity}
            onChange={(e) => setQuantity(e.target.value)}
          />
        </FormField>
        <FormField
          name={`${formId}-pos`}
          label={t("spares.position")}
          description={t("spares.positionHint")}
          error={positionError}
        >
          <Input
            id={`${formId}-pos`}
            value={position}
            onChange={(e) => setPosition(e.target.value)}
          />
        </FormField>
        <label className="flex min-h-[44px] items-center gap-2 text-sm sm:col-span-2">
          <Checkbox checked={critical} onCheckedChange={(v) => setCritical(v === true)} />
          {t("spares.criticalLabel")}
        </label>
        <FormField
          name={`${formId}-notes`}
          label={t("commercial.notes")}
          className="sm:col-span-2"
          error={notesError}
        >
          <Input id={`${formId}-notes`} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </FormField>
      </form>
    </EntityFormDialog>
  );
}
