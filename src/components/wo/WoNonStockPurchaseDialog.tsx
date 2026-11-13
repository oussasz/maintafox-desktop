/**
 * Buy a hors-stock part for this work order (L3-04, CMMS Advisor option B).
 *
 * The work-order editor first looks for an existing hors-stock article (reused as is); otherwise
 * they describe the part and its unit, and a hors-stock article is created from this work order,
 * marked "à revoir" for the catalogue manager. One step writes the article, the work-order line
 * and the purchase request pointing at it; the request then goes through the normal approvals
 * and the buyer orders it. The part is received without any stock movement and booked to the
 * work order as parts actual (backend `inventory::non_stock`).
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { MeasureUnitSelect, useMeasureUnits } from "@/components/lookups/MeasureUnitPicker";
import { Button } from "@/components/ui/button";
import { DecimalField } from "@/components/ui/decimal-field";
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
import { createWoNonStockPurchase, searchNonStockArticles } from "@/services/wo-execution-service";
import type { NonStockArticle, WoNonStockPurchase } from "@shared/ipc-types";

import { woActionFailureText } from "./wo-action-error";

interface WoNonStockPurchaseDialogProps {
  woId: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (created: WoNonStockPurchase) => Promise<void> | void;
}

export function WoNonStockPurchaseDialog({
  woId,
  open,
  onOpenChange,
  onCreated,
}: WoNonStockPurchaseDialogProps) {
  const { t } = useTranslation("ot");
  const { units } = useMeasureUnits(open);
  const [search, setSearch] = useState("");
  const [matches, setMatches] = useState<NonStockArticle[]>([]);
  const [chosen, setChosen] = useState<NonStockArticle | null>(null);
  const [name, setName] = useState("");
  const [unitId, setUnitId] = useState<number | null>(null);
  const [qty, setQty] = useState<number | null>(1);
  const [estimate, setEstimate] = useState<number | null>(null);
  const [reason, setReason] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!open) return;
    setSearch("");
    setMatches([]);
    setChosen(null);
    setName("");
    setUnitId(null);
    setQty(1);
    setEstimate(null);
    setReason("");
    setFormError(null);
  }, [open]);

  // Existing hors-stock articles matching the text: reuse one rather than create a twin.
  useEffect(() => {
    if (!open) return;
    const term = search.trim();
    if (term.length < 2) {
      setMatches([]);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      searchNonStockArticles(term)
        .then((rows) => {
          if (!cancelled) setMatches(rows);
        })
        .catch(() => {
          if (!cancelled) setMatches([]);
        });
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, search]);

  const submit = async () => {
    if (qty == null || !Number.isFinite(qty) || qty <= 0) {
      setFormError(t("execution.error.invalidQuantity"));
      return;
    }
    if (!chosen) {
      if (!name.trim()) {
        setFormError(t("nonStockPurchase.nameRequired"));
        return;
      }
      if (unitId == null) {
        setFormError(t("nonStockPurchase.unitRequired"));
        return;
      }
    }
    setPending(true);
    setFormError(null);
    try {
      const created = await createWoNonStockPurchase({
        work_order_id: woId,
        article_id: chosen?.id ?? null,
        article_name: chosen ? null : name.trim(),
        unit_value_id: chosen ? null : unitId,
        quantity: qty,
        estimated_unit_cost: estimate,
        notes: null,
        reason: reason.trim() || null,
      });
      onOpenChange(false);
      await onCreated(created);
    } catch (e) {
      setFormError(woActionFailureText(t("nonStockPurchase.error"), e));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("nonStockPurchase.title")}</DialogTitle>
          <DialogDescription>{t("nonStockPurchase.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1">
            <Label htmlFor="wo-hs-search">{t("nonStockPurchase.search")}</Label>
            <Input
              id="wo-hs-search"
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                setChosen(null);
              }}
              placeholder={t("nonStockPurchase.searchPlaceholder")}
            />
            {matches.length > 0 && !chosen ? (
              <ul
                aria-label={t("nonStockPurchase.matches")}
                className="max-h-40 space-y-1 overflow-auto rounded-md border p-1"
              >
                {matches.map((m) => (
                  <li key={m.id}>
                    <button
                      type="button"
                      className="w-full rounded-sm px-2 py-1.5 text-left text-sm hover:bg-surface-2"
                      onClick={() => setChosen(m)}
                    >
                      {m.article_code} — {m.article_name}
                      {m.unit_symbol ? ` (${m.unit_symbol})` : ""}
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
            {chosen ? (
              <div className="flex items-center justify-between rounded-md border bg-surface-2 px-3 py-2 text-sm">
                <span>
                  {t("nonStockPurchase.reused", {
                    code: chosen.article_code,
                    name: chosen.article_name,
                  })}
                </span>
                <Button type="button" size="sm" variant="ghost" onClick={() => setChosen(null)}>
                  {t("nonStockPurchase.notThisOne")}
                </Button>
              </div>
            ) : null}
          </div>

          {!chosen ? (
            <div className="grid gap-3 sm:grid-cols-[2fr_1fr]">
              <div className="space-y-1">
                <Label htmlFor="wo-hs-name">{t("nonStockPurchase.name")}</Label>
                <Input
                  id="wo-hs-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={t("nonStockPurchase.namePlaceholder")}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="wo-hs-unit">{t("nonStockPurchase.unit")}</Label>
                <MeasureUnitSelect
                  id="wo-hs-unit"
                  value={unitId}
                  units={units}
                  onChange={setUnitId}
                />
              </div>
              <p className="text-xs text-text-muted sm:col-span-2">
                {t("nonStockPurchase.newHint")}
              </p>
            </div>
          ) : null}

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="wo-hs-qty">
                {t("nonStockPurchase.qty")}
                {chosen?.unit_symbol ? ` (${chosen.unit_symbol})` : ""}
              </Label>
              <DecimalField
                id="wo-hs-qty"
                decimals={3}
                min={0.001}
                value={qty}
                onValueChange={setQty}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="wo-hs-estimate">{t("nonStockPurchase.estimate")}</Label>
              <DecimalField
                id="wo-hs-estimate"
                decimals={2}
                min={0}
                value={estimate}
                onValueChange={setEstimate}
              />
            </div>
          </div>

          <div className="space-y-1">
            <Label htmlFor="wo-hs-reason">{t("nonStockPurchase.reason")}</Label>
            <Input
              id="wo-hs-reason"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder={t("nonStockPurchase.reasonPlaceholder")}
            />
          </div>

          {formError ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {formError}
            </p>
          ) : null}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("nonStockPurchase.cancel")}
          </Button>
          <Button type="button" onClick={() => void submit()} disabled={pending}>
            {t("nonStockPurchase.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
