/**
 * Parts "à sortir" (L3-21): parts recorded on a work order by someone without the stock right.
 * Nothing left the store for them. The storekeeper issues each one (confirming or correcting the
 * quantity, a correction needs a reason) or cancels it with a reason; both are audited on the work
 * order, and a work order is not closed while one remains. Only the stock right (inv.manage) sees
 * and acts on this list (backend: `list_wo_parts_to_issue`, `issue_wo_part_to_issue`,
 * `cancel_wo_part_to_issue`).
 */
import { ClipboardList } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { listInventoryStockBalances } from "@/services/inventory-service";
import {
  cancelWoPartToIssue,
  issueWoPartToIssue,
  listWoPartsToIssue,
} from "@/services/wo-execution-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { InventoryStockBalance, WoPartToIssue } from "@shared/ipc-types";

/** Least length of a correction or cancellation reason (backend `TO_ISSUE_REASON_MIN_CHARS`). */
export const TO_ISSUE_REASON_MIN_CHARS = 5;

type Action = { kind: "issue" | "cancel"; row: WoPartToIssue };

function partLabel(row: WoPartToIssue): string {
  return row.article_label ?? row.article_ref ?? `#${row.id}`;
}

export function PartsToIssuePanel({
  reloadToken = 0,
  onChanged,
}: {
  reloadToken?: number;
  /** Called after an issue or a cancellation (stock and reservations changed). */
  onChanged?: () => void;
}) {
  const { t } = useTranslation("inventory");
  const { formatNumber } = useFormatters();
  const qty = useCallback(
    (v: number) => formatNumber(v, { maximumFractionDigits: 3 }),
    [formatNumber],
  );

  const [rows, setRows] = useState<WoPartToIssue[]>([]);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);

  const [action, setAction] = useState<Action | null>(null);
  const [quantity, setQuantity] = useState<number | null>(null);
  const [reason, setReason] = useState("");
  const [locationId, setLocationId] = useState<string>("");
  const [balances, setBalances] = useState<InventoryStockBalance[]>([]);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<unknown>(null);

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    listWoPartsToIssue()
      .then((list) => {
        if (!cancelled) setRows(list);
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [reloadToken, version]);

  // A line with neither a reservation nor a location: the storekeeper chooses where it leaves from.
  const needsLocation =
    action?.kind === "issue" &&
    action.row.reservation_id == null &&
    action.row.stock_location_id == null;
  const articleId = action?.row.article_id ?? null;
  useEffect(() => {
    if (!needsLocation || articleId == null) {
      setBalances([]);
      return;
    }
    let cancelled = false;
    listInventoryStockBalances({ article_id: articleId, warehouse_id: null })
      .then((list) => {
        if (!cancelled) setBalances(list.filter((b) => b.available_qty > 0));
      })
      .catch((e: unknown) => {
        if (!cancelled) setActionError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [needsLocation, articleId]);

  const open = (kind: Action["kind"], row: WoPartToIssue) => {
    setAction({ kind, row });
    setQuantity(row.pending_issue_qty);
    setReason("");
    setLocationId("");
    setActionError(null);
  };

  const corrected =
    action?.kind === "issue" && quantity != null && quantity !== action.row.pending_issue_qty;
  const reasonOk = reason.trim().length >= TO_ISSUE_REASON_MIN_CHARS;
  const canConfirm = useMemo(() => {
    if (!action || busy) return false;
    if (action.kind === "cancel") return reasonOk;
    if (quantity == null || quantity <= 0) return false;
    if (corrected && !reasonOk) return false;
    if (needsLocation && !locationId) return false;
    return true;
  }, [action, busy, quantity, corrected, reasonOk, needsLocation, locationId]);

  const confirm = async () => {
    if (!action || !canConfirm) return;
    const { row } = action;
    setBusy(true);
    setActionError(null);
    try {
      if (action.kind === "issue") {
        await issueWoPartToIssue({
          wo_part_id: row.id,
          quantity: quantity ?? 0,
          stock_location_id: needsLocation ? Number(locationId) : null,
          correction_reason: corrected ? reason.trim() : null,
        });
        pushAppToast({
          title: t("issueDesk.toIssue.issued", { part: partLabel(row), wo: row.wo_code }),
          variant: "success",
        });
      } else {
        await cancelWoPartToIssue({ wo_part_id: row.id, reason: reason.trim() });
        pushAppToast({
          title: t("issueDesk.toIssue.cancelled", { wo: row.wo_code }),
          variant: "success",
        });
      }
      setAction(null);
      setVersion((v) => v + 1);
      onChanged?.();
    } catch (e) {
      setActionError(e);
    } finally {
      setBusy(false);
    }
  };

  const unit = (row: WoPartToIssue) => row.unit_symbol ?? "";

  return (
    <section
      aria-labelledby="inv-parts-to-issue-title"
      className="rounded-lg border border-surface-border p-4"
    >
      <div className="mb-3 flex items-start gap-3">
        <ClipboardList className="mt-0.5 h-5 w-5 text-text-muted" aria-hidden />
        <div>
          <h2 id="inv-parts-to-issue-title" className="text-base font-semibold">
            {t("issueDesk.toIssue.title")}
            {rows.length > 0 ? ` (${rows.length})` : ""}
          </h2>
          <p className="text-sm text-text-muted">{t("issueDesk.toIssue.description")}</p>
        </div>
      </div>

      {loadError ? (
        <ErrorState error={loadError} title={t("issueDesk.toIssue.loadFailed")} />
      ) : rows.length === 0 ? (
        <p className="text-sm text-text-muted">{t("issueDesk.toIssue.empty")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="py-2 pr-3 font-medium">{t("issueDesk.toIssue.workOrder")}</th>
                <th className="py-2 pr-3 font-medium">{t("issueDesk.toIssue.part")}</th>
                <th className="py-2 pr-3 text-right font-medium">
                  {t("issueDesk.toIssue.quantity")}
                </th>
                <th className="py-2 pr-3 font-medium">{t("issueDesk.toIssue.location")}</th>
                <th className="py-2" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.id} className="border-b border-surface-border last:border-0">
                  <td className="py-2 pr-3">
                    <span className="font-mono">{row.wo_code}</span>
                    {row.wo_title ? (
                      <span className="block text-xs text-text-muted">{row.wo_title}</span>
                    ) : null}
                  </td>
                  <td className="py-2 pr-3">{partLabel(row)}</td>
                  <td className="py-2 pr-3 text-right tabular-nums">
                    {qty(row.pending_issue_qty)} {unit(row)}
                  </td>
                  <td className="py-2 pr-3">
                    {row.location_label ?? (
                      <span className="text-text-muted">{t("issueDesk.toIssue.noLocation")}</span>
                    )}
                  </td>
                  <td className="py-2 text-right">
                    <div className="flex justify-end gap-2">
                      <Button size="sm" onClick={() => open("issue", row)}>
                        {t("issueDesk.toIssue.issue")}
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => open("cancel", row)}>
                        {t("issueDesk.toIssue.cancel")}
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <Dialog open={action != null} onOpenChange={(next) => !busy && !next && setAction(null)}>
        <DialogContent className="max-w-md">
          {action ? (
            <>
              <DialogHeader>
                <DialogTitle>
                  {action.kind === "issue"
                    ? t("issueDesk.toIssue.issueTitle", {
                        part: partLabel(action.row),
                        wo: action.row.wo_code,
                      })
                    : t("issueDesk.toIssue.cancelTitle", { part: partLabel(action.row) })}
                </DialogTitle>
                <DialogDescription>
                  {action.kind === "issue"
                    ? t("issueDesk.toIssue.requested", {
                        qty: qty(action.row.pending_issue_qty),
                        unit: unit(action.row),
                      })
                    : t("issueDesk.toIssue.cancelHint")}
                </DialogDescription>
              </DialogHeader>

              <div className="space-y-4">
                {action.kind === "issue" ? (
                  <div className="space-y-1">
                    <Label htmlFor="inv-to-issue-qty">{t("issueDesk.toIssue.issueQuantity")}</Label>
                    <DecimalField
                      id="inv-to-issue-qty"
                      decimals={3}
                      min={0}
                      value={quantity}
                      onValueChange={setQuantity}
                      disabled={busy}
                    />
                  </div>
                ) : null}
                {needsLocation ? (
                  <div className="space-y-1">
                    <Label htmlFor="inv-to-issue-location">
                      {t("issueDesk.toIssue.chooseLocation")}
                    </Label>
                    <Select value={locationId} onValueChange={setLocationId} disabled={busy}>
                      <SelectTrigger id="inv-to-issue-location">
                        <SelectValue placeholder={t("issueDesk.toIssue.locationPlaceholder")} />
                      </SelectTrigger>
                      <SelectContent>
                        {balances.map((b) => (
                          <SelectItem key={b.location_id} value={String(b.location_id)}>
                            {t("issueDesk.toIssue.available", {
                              location: `${b.warehouse_code}/${b.location_code}`,
                              qty: qty(b.available_qty),
                            })}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                ) : null}
                {action.kind === "cancel" || corrected ? (
                  <div className="space-y-1">
                    <Label htmlFor="inv-to-issue-reason">
                      {action.kind === "cancel"
                        ? t("issueDesk.toIssue.cancelReason")
                        : t("issueDesk.toIssue.correctionReason")}
                    </Label>
                    <Textarea
                      id="inv-to-issue-reason"
                      value={reason}
                      onChange={(e) => setReason(e.target.value)}
                      rows={2}
                      disabled={busy}
                    />
                    {corrected ? (
                      <p className="text-xs text-text-muted">
                        {t("issueDesk.toIssue.correctionHint")}
                      </p>
                    ) : null}
                  </div>
                ) : null}
                {actionError ? (
                  <ErrorState error={actionError} title={t("issueDesk.toIssue.actionFailed")} />
                ) : null}
              </div>

              <DialogFooter>
                <Button variant="outline" onClick={() => setAction(null)} disabled={busy}>
                  {t("issueDesk.toIssue.cancel")}
                </Button>
                <Button
                  variant={action.kind === "cancel" ? "destructive" : "default"}
                  onClick={() => void confirm()}
                  disabled={!canConfirm}
                >
                  {action.kind === "issue"
                    ? t("issueDesk.toIssue.confirmIssue")
                    : t("issueDesk.toIssue.confirmCancel")}
                </Button>
              </DialogFooter>
            </>
          ) : null}
        </DialogContent>
      </Dialog>
    </section>
  );
}
