/**
 * WoAddExecutionPartDialog.tsx
 *
 * Records a part used during execution that was not in the plan. By default the part is
 * picked from the inventory article master and a stock location that holds it; the backend
 * then reserves and issues the quantity (stock movement + WO parts cost). Free text is kept
 * only as an explicit "non-stock item" choice (e.g. a part bought on the spot).
 */

import { Loader2, PackagePlus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
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
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type InventoryArticleRow,
  listInventoryArticles,
  listInventoryStockBalances,
} from "@/services/inventory-service";
import { addPart } from "@/services/wo-execution-service";
import type { InventoryStockBalance } from "@shared/ipc-types";

import { woActionFailureText } from "./wo-action-error";

type PartMode = "stock" | "non_stock";

/** Matches shown in the article list at once; the search narrows the rest. */
const ARTICLE_RESULT_LIMIT = 50;

interface WoAddExecutionPartDialogProps {
  woId: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAdded: () => Promise<void> | void;
}

export function WoAddExecutionPartDialog({
  woId,
  open,
  onOpenChange,
  onAdded,
}: WoAddExecutionPartDialogProps) {
  const { t } = useTranslation("ot");
  const { formatNumber } = useFormatters();
  const [mode, setMode] = useState<PartMode>("stock");
  const [articles, setArticles] = useState<InventoryArticleRow[]>([]);
  const [articlesState, setArticlesState] = useState<"loading" | "ready" | "error">("loading");
  const [search, setSearch] = useState("");
  const [articleId, setArticleId] = useState("");
  const [balances, setBalances] = useState<InventoryStockBalance[]>([]);
  const [balancesLoading, setBalancesLoading] = useState(false);
  const [locationId, setLocationId] = useState("");
  const [label, setLabel] = useState("");
  const [qty, setQty] = useState("1");
  const [formError, setFormError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!open) return;
    setMode("stock");
    setSearch("");
    setArticleId("");
    setBalances([]);
    setLocationId("");
    setLabel("");
    setQty("1");
    setFormError(null);
    let cancelled = false;
    setArticlesState("loading");
    listInventoryArticles({ search: null })
      .then((rows) => {
        if (cancelled) return;
        setArticles(rows.filter((row) => row.is_active === 1));
        setArticlesState("ready");
      })
      .catch(() => {
        if (!cancelled) setArticlesState("error");
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const rows = q
      ? articles.filter(
          (a) =>
            a.article_code.toLowerCase().includes(q) || a.article_name.toLowerCase().includes(q),
        )
      : articles;
    return rows.slice(0, ARTICLE_RESULT_LIMIT);
  }, [articles, search]);

  const selectedArticle = useMemo(
    () => articles.find((a) => String(a.id) === articleId) ?? null,
    [articles, articleId],
  );

  // Stock locations that hold the chosen article; preselect the one with most available.
  useEffect(() => {
    if (!selectedArticle) {
      setBalances([]);
      setLocationId("");
      return;
    }
    let cancelled = false;
    setBalancesLoading(true);
    listInventoryStockBalances({ article_id: selectedArticle.id })
      .then((rows) => {
        if (cancelled) return;
        const withStock = rows
          .filter((b) => b.available_qty > 0)
          .sort((a, b) => b.available_qty - a.available_qty);
        setBalances(withStock);
        const preferred =
          withStock.find((b) => b.location_id === selectedArticle.preferred_location_id) ??
          withStock[0];
        setLocationId(preferred ? String(preferred.location_id) : "");
      })
      .catch(() => {
        if (!cancelled) {
          setBalances([]);
          setLocationId("");
        }
      })
      .finally(() => {
        if (!cancelled) setBalancesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedArticle]);

  const selectedBalance = balances.find((b) => String(b.location_id) === locationId) ?? null;

  const submit = async () => {
    const quantity = Number(qty);
    if (!Number.isFinite(quantity) || quantity <= 0) {
      setFormError(t("execution.error.invalidQuantity"));
      return;
    }
    if (mode === "stock") {
      if (!selectedArticle) {
        setFormError(t("execution.parts.add.articleRequired"));
        return;
      }
      if (!selectedBalance) {
        setFormError(t("execution.parts.add.locationRequired"));
        return;
      }
      if (quantity > selectedBalance.available_qty) {
        setFormError(
          t("execution.parts.add.notEnoughStock", {
            available: formatNumber(selectedBalance.available_qty),
            unit: selectedArticle.unit_label,
          }),
        );
        return;
      }
    } else if (!label.trim()) {
      setFormError(t("execution.parts.add.labelRequired"));
      return;
    }

    setPending(true);
    setFormError(null);
    try {
      if (mode === "stock" && selectedArticle && selectedBalance) {
        await addPart({
          wo_id: woId,
          article_id: selectedArticle.id,
          stock_location_id: selectedBalance.location_id,
          quantity_planned: quantity,
          auto_reserve: true,
        });
      } else {
        await addPart({
          wo_id: woId,
          article_ref: label.trim(),
          quantity_planned: quantity,
          auto_reserve: false,
        });
      }
      onOpenChange(false);
      await onAdded();
    } catch (e) {
      setFormError(woActionFailureText(t("execution.parts.add.error"), e));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("execution.parts.add.title")}</DialogTitle>
          <DialogDescription>{t("execution.parts.add.stockHint")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div
            role="radiogroup"
            aria-label={t("execution.parts.add.modeLabel")}
            className="grid grid-cols-2 gap-2"
          >
            {(["stock", "non_stock"] as const).map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={mode === m}
                onClick={() => {
                  setMode(m);
                  setFormError(null);
                }}
                className={cn(
                  "min-h-[44px] rounded-md border px-3 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  mode === m
                    ? "border-primary bg-primary/10 text-text-primary"
                    : "border-surface-border text-text-secondary hover:bg-surface-2",
                )}
              >
                {m === "stock"
                  ? t("execution.parts.add.modeStock")
                  : t("execution.parts.add.modeNonStock")}
              </button>
            ))}
          </div>

          {mode === "stock" ? (
            <>
              <div className="space-y-1">
                <Label htmlFor="wo-exec-part-search">
                  {t("execution.parts.add.searchArticle")}
                </Label>
                <Input
                  id="wo-exec-part-search"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t("execution.parts.add.searchPlaceholder")}
                  disabled={articlesState !== "ready"}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="wo-exec-part-article">{t("execution.parts.add.article")}</Label>
                <Select
                  value={articleId || "__none"}
                  onValueChange={(v) => setArticleId(v === "__none" ? "" : v)}
                  disabled={articlesState !== "ready"}
                >
                  <SelectTrigger id="wo-exec-part-article">
                    <SelectValue
                      placeholder={
                        articlesState === "loading"
                          ? t("execution.parts.add.articlesLoading")
                          : t("execution.parts.add.articlePlaceholder")
                      }
                    />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none">
                      {t("execution.parts.add.articlePlaceholder")}
                    </SelectItem>
                    {filtered.map((a) => (
                      <SelectItem key={a.id} value={String(a.id)}>
                        {a.article_code} — {a.article_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {articlesState === "error" ? (
                  <p className="text-xs text-text-muted">
                    {t("execution.parts.add.articlesFailed")}
                  </p>
                ) : articlesState === "ready" && filtered.length === 0 ? (
                  <p className="text-xs text-text-muted">
                    {t("execution.parts.add.noArticleMatch")}
                  </p>
                ) : null}
              </div>
              {selectedArticle ? (
                <div className="space-y-1">
                  <Label htmlFor="wo-exec-part-location">{t("execution.parts.add.location")}</Label>
                  <Select
                    value={locationId || "__none"}
                    onValueChange={(v) => setLocationId(v === "__none" ? "" : v)}
                    disabled={balancesLoading || balances.length === 0}
                  >
                    <SelectTrigger id="wo-exec-part-location">
                      <SelectValue placeholder={t("execution.parts.add.locationPlaceholder")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none">
                        {t("execution.parts.add.locationPlaceholder")}
                      </SelectItem>
                      {balances.map((b) => (
                        <SelectItem key={b.location_id} value={String(b.location_id)}>
                          {t("execution.parts.add.locationOption", {
                            warehouse: b.warehouse_name ?? b.warehouse_code,
                            location: b.location_name ?? b.location_code,
                            available: formatNumber(b.available_qty),
                            unit: selectedArticle.unit_label,
                          })}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {!balancesLoading && balances.length === 0 ? (
                    <p className="text-xs text-text-muted">{t("execution.parts.add.noStock")}</p>
                  ) : null}
                </div>
              ) : null}
            </>
          ) : (
            <div className="space-y-1">
              <Label htmlFor="wo-exec-part-label">{t("execution.parts.add.label")}</Label>
              <Input
                id="wo-exec-part-label"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                placeholder={t("execution.parts.add.labelPlaceholder")}
              />
              <p className="text-xs text-text-muted">{t("execution.parts.add.nonStockHint")}</p>
            </div>
          )}

          <div className="space-y-1">
            <Label htmlFor="wo-exec-part-qty">
              {t("execution.parts.add.qty")}
              {mode === "stock" && selectedArticle ? ` (${selectedArticle.unit_label})` : ""}
            </Label>
            <Input
              id="wo-exec-part-qty"
              type="number"
              min="0.01"
              step="0.01"
              value={qty}
              onChange={(e) => setQty(e.target.value)}
            />
          </div>

          {formError ? (
            <p role="alert" className="text-sm text-status-danger">
              {formError}
            </p>
          ) : null}
        </div>

        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            disabled={pending}
            onClick={() => onOpenChange(false)}
          >
            {t("execution.cancel")}
          </Button>
          <Button className="h-11 gap-1.5" disabled={pending} onClick={() => void submit()}>
            {pending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <PackagePlus className="h-4 w-4" aria-hidden />
            )}
            {mode === "stock" ? t("execution.parts.add.saveIssue") : t("execution.parts.add.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
