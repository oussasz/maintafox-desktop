/**
 * Hors-stock articles (L3-04, CMMS Advisor option B): parts bought for one work order, never in
 * the store, out of stock lists, counts, reorder and valuation. An article created from a work
 * order is "à revoir": the catalogue manager (article edit right, inv.manage) keeps it hors
 * stock, moves it to stock (it becomes a stocked article), or marks it a duplicate of another
 * article. Nothing is deleted; a duplicate is no longer offered for new requests.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorText } from "@/lib/app-error";
import {
  type InventoryArticleRow,
  listNonStockArticles,
  reviewNonStockArticle,
} from "@/services/inventory-service";
import type { NonStockArticle } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ReviewAction = "KEEP" | "PROMOTE" | "DUPLICATE";

interface NonStockReviewPanelProps {
  /** Articles of the catalogue: the article a duplicate duplicates is one of them. */
  articles: InventoryArticleRow[];
}

export function NonStockReviewPanel({ articles }: NonStockReviewPanelProps) {
  const { t } = useTranslation("inventory");
  const { can } = usePermissions();
  const canReview = can(P.INV_MANAGE);
  const [rows, setRows] = useState<NonStockArticle[]>([]);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [toReviewOnly, setToReviewOnly] = useState(true);
  const [duplicateOf, setDuplicateOf] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);

  const load = useCallback(async () => {
    setState("loading");
    try {
      setRows(await listNonStockArticles(toReviewOnly));
      setState("ready");
    } catch {
      setState("error");
    }
  }, [toReviewOnly]);

  useEffect(() => {
    void load();
  }, [load]);

  const duplicateTargets = useMemo(() => articles.filter((a) => a.is_active === 1), [articles]);

  const review = async (article: NonStockArticle, action: ReviewAction) => {
    const target = duplicateOf[article.id];
    if (action === "DUPLICATE" && !target) {
      setError(t("nonStock.chooseDuplicate"));
      return;
    }
    setBusyId(article.id);
    setError(null);
    try {
      await reviewNonStockArticle({
        article_id: article.id,
        action,
        duplicate_of_article_id: action === "DUPLICATE" ? Number(target) : null,
      });
      await load();
    } catch (e) {
      setError(`${t("nonStock.reviewFailed")} ${appErrorText(e)}`);
    } finally {
      setBusyId(null);
    }
  };

  const stateLabel = (s: string | null) =>
    s === "TO_REVIEW"
      ? t("nonStock.state.toReview")
      : s === "DUPLICATE"
        ? t("nonStock.state.duplicate")
        : t("nonStock.state.reviewed");

  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold">{t("nonStock.title")}</h2>
          <p className="text-xs text-text-muted">{t("nonStock.description")}</p>
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={toReviewOnly}
            onChange={(e) => setToReviewOnly(e.target.checked)}
          />
          {t("nonStock.toReviewOnly")}
        </label>
      </div>

      {error ? (
        <p role="alert" className="text-sm text-tone-danger-fg">
          {error}
        </p>
      ) : null}

      {state === "loading" ? (
        <p className="text-sm text-text-muted">{t("nonStock.loading")}</p>
      ) : state === "error" ? (
        <p className="text-sm text-tone-danger-fg">{t("nonStock.loadFailed")}</p>
      ) : rows.length === 0 ? (
        <p className="text-sm text-text-muted">{t("nonStock.empty")}</p>
      ) : (
        <ul className="space-y-2">
          {rows.map((a) => (
            <li key={a.id} className="space-y-2 rounded-md border p-3 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium">
                  {a.article_code} — {a.article_name}
                </span>
                {a.unit_symbol ? <span className="text-text-muted">({a.unit_symbol})</span> : null}
                <StatusChip
                  tone={a.catalogue_review_state === "TO_REVIEW" ? "attention" : "neutral"}
                  label={stateLabel(a.catalogue_review_state)}
                />
                {a.origin_work_order_code ? (
                  <span className="text-xs text-text-muted">
                    {t("nonStock.fromWorkOrder", { code: a.origin_work_order_code })}
                  </span>
                ) : null}
              </div>
              {canReview && a.catalogue_review_state !== "DUPLICATE" ? (
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === a.id}
                    onClick={() => void review(a, "KEEP")}
                  >
                    {t("nonStock.keep")}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === a.id}
                    onClick={() => void review(a, "PROMOTE")}
                  >
                    {t("nonStock.promote")}
                  </Button>
                  <select
                    aria-label={t("nonStock.duplicateOf")}
                    className="h-8 rounded-md border bg-background px-2 text-sm"
                    value={duplicateOf[a.id] ?? ""}
                    onChange={(e) => setDuplicateOf((m) => ({ ...m, [a.id]: e.target.value }))}
                  >
                    <option value="">{t("nonStock.duplicateOf")}</option>
                    {duplicateTargets
                      .filter((d) => d.id !== a.id)
                      .map((d) => (
                        <option key={d.id} value={String(d.id)}>
                          {d.article_code} — {d.article_name}
                        </option>
                      ))}
                  </select>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === a.id}
                    onClick={() => void review(a, "DUPLICATE")}
                  >
                    {t("nonStock.markDuplicate")}
                  </Button>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
