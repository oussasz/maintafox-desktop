/**
 * Issue desk: the storekeeper scans (or types) a part, picks the work order it goes to and
 * issues it. The issue posts through `issue_inventory_stock_to_work_order`, the same ISSUE
 * movement as part usage on the work order (reservation consumed, average cost, lots FEFO).
 *
 * The scan field works with a keyboard-wedge scanner: the scanner types the code and presses
 * Enter, which looks the code up; Enter in the quantity field issues.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { PackageCheck, ScanLine } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { WorkOrderReferencePicker } from "@/components/inventory/WorkOrderReferencePicker";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type InventoryArticleRow,
  type IssueOutcome,
  type LotStock,
  type ScanResolution,
  isLotTracked,
  issueInventoryStockToWorkOrder,
  listInventoryArticles,
  listInventoryExpiringLots,
  listInventoryLotStock,
  listInventoryReservations,
  listInventoryStockBalances,
  resolveInventoryScan,
} from "@/services/inventory-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { InventoryStockBalance, StockReservation, WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Expiry warning windows offered (days). */
export const EXPIRY_WINDOWS = [30, 60, 90] as const;
const AUTO_LOT = "auto";
const DIRECT_ISSUE = "direct";
/** Reservation source types written by the WO module and by the desk. */
const WORK_ORDER_SOURCES: ReadonlySet<string> = new Set([
  "WO",
  "WORK_ORDER",
  "WORK_ORDER_PART",
  "work_order",
]);
const OPEN_RESERVATION_STATUSES: ReadonlySet<string> = new Set(["active", "partial"]);

/** First lot a FEFO pick would take: the earliest-expiring lot that has not expired. */
export function fefoSuggestion(lots: LotStock[]): LotStock | null {
  return (
    lots.find((l) => l.quantity > 0 && (l.days_to_expiry == null || l.days_to_expiry >= 0)) ?? null
  );
}

/** Tone of an expiry: expired = danger, within 30 days = warning, later = neutral. */
export function expiryTone(daysToExpiry: number | null): Tone {
  if (daysToExpiry == null) return "neutral";
  if (daysToExpiry < 0) return "danger";
  if (daysToExpiry <= 30) return "warning";
  return "neutral";
}

export interface InventoryIssueDeskProps {
  /** Bumped by the page refresh button. */
  reloadToken?: number;
}

export function InventoryIssueDesk({ reloadToken = 0 }: InventoryIssueDeskProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatCurrency, formatDate } = useFormatters();
  const qty = useCallback(
    (v: number) => formatNumber(v, { maximumFractionDigits: 3 }),
    [formatNumber],
  );

  const scanRef = useRef<HTMLInputElement>(null);
  const [scanValue, setScanValue] = useState("");
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<unknown>(null);
  const [scan, setScan] = useState<ScanResolution | null>(null);
  const [articleRow, setArticleRow] = useState<InventoryArticleRow | null>(null);

  const [workOrder, setWorkOrder] = useState<WorkOrder | null>(null);
  const [workOrderCode, setWorkOrderCode] = useState<string | null>(null);

  const [balances, setBalances] = useState<InventoryStockBalance[]>([]);
  const [reservations, setReservations] = useState<StockReservation[]>([]);
  const [lots, setLots] = useState<LotStock[]>([]);
  const [contextError, setContextError] = useState<unknown>(null);

  const [locationId, setLocationId] = useState<number>(0);
  const [reservationChoice, setReservationChoice] = useState<string>(DIRECT_ISSUE);
  const [lotChoice, setLotChoice] = useState<string>(AUTO_LOT);
  const [quantity, setQuantity] = useState("");
  const [issuing, setIssuing] = useState(false);
  const [issueError, setIssueError] = useState<unknown>(null);
  const [outcome, setOutcome] = useState<{
    result: IssueOutcome;
    articleCode: string;
    unit: string;
  } | null>(null);

  const [expiryWindow, setExpiryWindow] = useState<number>(EXPIRY_WINDOWS[0]);
  const [expiring, setExpiring] = useState<LotStock[]>([]);
  const [expiringLoading, setExpiringLoading] = useState(false);
  const [expiringError, setExpiringError] = useState<unknown>(null);
  const [expiringVersion, setExpiringVersion] = useState(0);

  const tracked = articleRow ? isLotTracked(articleRow) : false;
  const unit = scan?.unit_code ?? "";

  // ── Expiry warning list ──────────────────────────────────────────────────────
  useEffect(() => {
    let cancelled = false;
    setExpiringLoading(true);
    setExpiringError(null);
    listInventoryExpiringLots(expiryWindow)
      .then((rows) => {
        if (!cancelled) setExpiring(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setExpiringError(err);
      })
      .finally(() => {
        if (!cancelled) setExpiringLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [expiryWindow, expiringVersion, reloadToken]);

  // ── Stock of the scanned article ─────────────────────────────────────────────
  useEffect(() => {
    if (!scan) {
      setBalances([]);
      setArticleRow(null);
      return;
    }
    let cancelled = false;
    setContextError(null);
    Promise.all([
      listInventoryStockBalances({ article_id: scan.article_id, warehouse_id: null }),
      listInventoryArticles({ search: scan.article_code }),
    ])
      .then(([balanceRows, articleRows]) => {
        if (cancelled) return;
        const stocked = balanceRows.filter((b) => b.available_qty > 0);
        setBalances(stocked);
        setArticleRow(articleRows.find((a) => a.id === scan.article_id) ?? null);
        setLocationId((current) => {
          if (current > 0 && stocked.some((b) => b.location_id === current)) return current;
          const preferred = articleRows.find(
            (a) => a.id === scan.article_id,
          )?.preferred_location_id;
          const pick =
            stocked.find((b) => b.location_id === preferred) ??
            [...stocked].sort((a, b) => b.available_qty - a.available_qty)[0];
          return pick?.location_id ?? 0;
        });
      })
      .catch((err: unknown) => {
        if (!cancelled) setContextError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [scan, reloadToken]);

  // ── Planned parts of the work order for this article ─────────────────────────
  useEffect(() => {
    if (!scan || !workOrder) {
      setReservations([]);
      setReservationChoice(DIRECT_ISSUE);
      return;
    }
    let cancelled = false;
    listInventoryReservations({ article_id: scan.article_id, source_id: workOrder.id })
      .then((rows) => {
        if (cancelled) return;
        setReservations(
          rows.filter(
            (r) =>
              r.source_id === workOrder.id &&
              WORK_ORDER_SOURCES.has(r.source_type) &&
              OPEN_RESERVATION_STATUSES.has(r.status) &&
              r.quantity_reserved - r.quantity_issued > 0,
          ),
        );
      })
      .catch((err: unknown) => {
        if (!cancelled) setContextError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [scan, workOrder]);

  // ── Lots at the chosen location ──────────────────────────────────────────────
  useEffect(() => {
    if (!scan || !tracked || locationId <= 0) {
      setLots([]);
      return;
    }
    let cancelled = false;
    listInventoryLotStock(scan.article_id, locationId)
      .then((rows) => {
        if (cancelled) return;
        const inStock = rows.filter((l) => l.quantity > 0);
        setLots(inStock);
        setLotChoice((current) =>
          current !== AUTO_LOT && inStock.some((l) => String(l.lot_id) === current)
            ? current
            : AUTO_LOT,
        );
      })
      .catch((err: unknown) => {
        if (!cancelled) setContextError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [scan, tracked, locationId]);

  const selectedReservation = useMemo(
    () => reservations.find((r) => String(r.id) === reservationChoice) ?? null,
    [reservations, reservationChoice],
  );
  const effectiveLocationId = selectedReservation?.location_id ?? locationId;
  const selectedBalance = balances.find((b) => b.location_id === effectiveLocationId) ?? null;
  const chosenLot = lots.find((l) => String(l.lot_id) === lotChoice) ?? null;
  const fefo = fefoSuggestion(lots);
  const parsedQty = Number(quantity.replace(",", "."));
  const reservedLeft = selectedReservation
    ? selectedReservation.quantity_reserved - selectedReservation.quantity_issued
    : null;
  const maxQty =
    reservedLeft ??
    (chosenLot ? Math.min(chosenLot.quantity, selectedBalance?.available_qty ?? 0) : null) ??
    selectedBalance?.available_qty ??
    0;

  const blocker = !scan
    ? t("issueDesk.blockers.article")
    : !workOrder && !selectedReservation
      ? t("issueDesk.blockers.workOrder")
      : effectiveLocationId <= 0
        ? t("issueDesk.blockers.location")
        : !(parsedQty > 0)
          ? t("issueDesk.blockers.quantity")
          : parsedQty > maxQty + 1e-9
            ? t("issueDesk.blockers.tooMuch", { qty: qty(maxQty), unit })
            : null;

  const resetArticle = useCallback(() => {
    setScan(null);
    setScanValue("");
    setLocationId(0);
    setLotChoice(AUTO_LOT);
    setReservationChoice(DIRECT_ISSUE);
    setQuantity("");
    setIssueError(null);
    window.setTimeout(() => scanRef.current?.focus(), 0);
  }, []);

  const onScan = async (event: FormEvent) => {
    event.preventDefault();
    const value = scanValue.trim();
    if (!value || scanning) return;
    setScanning(true);
    setScanError(null);
    setIssueError(null);
    setOutcome(null);
    try {
      const found = await resolveInventoryScan(value);
      setScan(found);
      setLocationId(0);
      setLotChoice(found.lot_id != null ? String(found.lot_id) : AUTO_LOT);
      setReservationChoice(DIRECT_ISSUE);
      setQuantity(found.lot_id != null ? "1" : "");
    } catch (err) {
      setScan(null);
      setScanError(err);
    } finally {
      setScanning(false);
    }
  };

  // A scanned serial / lot sits at one location: move there once the lots are known.
  useEffect(() => {
    if (!scan?.lot_id || balances.length === 0) return;
    void listInventoryLotStock(scan.article_id, null)
      .then((rows) => {
        const lot = rows.find((l) => l.lot_id === scan.lot_id && l.quantity > 0);
        if (lot) setLocationId(lot.location_id);
      })
      .catch(() => undefined);
  }, [scan, balances.length]);

  const onIssue = async (event: FormEvent) => {
    event.preventDefault();
    if (!scan || blocker || issuing) return;
    setIssuing(true);
    setIssueError(null);
    try {
      const result = await issueInventoryStockToWorkOrder({
        article_id: scan.article_id,
        location_id: effectiveLocationId,
        quantity: parsedQty,
        work_order_id: selectedReservation ? null : (workOrder?.id ?? null),
        reservation_id: selectedReservation?.id ?? null,
        lot_id: tracked && chosenLot ? chosenLot.lot_id : null,
        notes: null,
      });
      setOutcome({ result, articleCode: scan.article_code, unit });
      pushAppToast({
        title: t("issueDesk.outcome.toast", {
          qty: qty(parsedQty),
          unit,
          code: scan.article_code,
          wo: result.work_order_code ?? workOrder?.code ?? "",
        }),
        variant: "success",
      });
      setExpiringVersion((v) => v + 1);
      resetArticle();
    } catch (err) {
      setIssueError(err);
    } finally {
      setIssuing(false);
    }
  };

  const lotLabel = (lot: LotStock) => lot.serial_number ?? lot.lot_number ?? "";

  const expiringColumns = useMemo<ColumnDef<LotStock>[]>(
    () => [
      {
        accessorKey: "article_code",
        header: t("issueDesk.expiring.columns.article"),
        cell: ({ row }) => (
          <span>
            <span className="font-mono text-xs">{row.original.article_code}</span>{" "}
            <span className="text-text-secondary">{row.original.article_name}</span>
          </span>
        ),
      },
      {
        id: "lot",
        header: t("issueDesk.expiring.columns.lot"),
        cell: ({ row }) => <span className="font-mono text-xs">{lotLabel(row.original)}</span>,
      },
      {
        id: "location",
        header: t("issueDesk.expiring.columns.location"),
        cell: ({ row }) => `${row.original.warehouse_code}/${row.original.location_code}`,
      },
      {
        accessorKey: "quantity",
        header: t("issueDesk.expiring.columns.quantity"),
        cell: ({ row }) => <span className="tabular-nums">{qty(row.original.quantity)}</span>,
      },
      {
        accessorKey: "expiry_date",
        header: t("issueDesk.expiring.columns.expiry"),
        cell: ({ row }) => {
          const days = row.original.days_to_expiry;
          const label =
            days == null
              ? "—"
              : days < 0
                ? t("issueDesk.expiring.expired", { count: Math.abs(days) })
                : days === 0
                  ? t("issueDesk.expiring.today")
                  : t("issueDesk.expiring.daysLeft", { count: days });
          return (
            <span className="flex flex-wrap items-center gap-2">
              <span className="tabular-nums">
                {row.original.expiry_date ? formatDate(row.original.expiry_date) : "—"}
              </span>
              <StatusChip label={label} tone={expiryTone(days)} />
            </span>
          );
        },
      },
    ],
    [formatDate, qty, t],
  );

  const unitCostLabel = (v: number | null) =>
    v == null ? t("issueDesk.outcome.unvalued") : formatCurrency(v);

  return (
    <div className="space-y-6">
      <section
        aria-labelledby="inv-issue-desk-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-4 flex items-start gap-3">
          <PackageCheck className="mt-0.5 h-5 w-5 text-text-muted" aria-hidden />
          <div>
            <h2 id="inv-issue-desk-title" className="text-base font-semibold">
              {t("issueDesk.title")}
            </h2>
            <p className="text-sm text-text-muted">{t("issueDesk.description")}</p>
          </div>
        </div>

        <div className="grid gap-6 lg:grid-cols-2">
          <div className="space-y-4">
            <form onSubmit={(e) => void onScan(e)} className="space-y-1">
              <Label htmlFor="inv-issue-scan">{t("issueDesk.scan.label")}</Label>
              <div className="flex gap-2">
                <Input
                  id="inv-issue-scan"
                  ref={scanRef}
                  autoFocus
                  autoComplete="off"
                  className="h-11 font-mono"
                  value={scanValue}
                  onChange={(e) => setScanValue(e.target.value)}
                  placeholder={t("issueDesk.scan.placeholder")}
                  aria-describedby="inv-issue-scan-hint"
                />
                <Button
                  type="submit"
                  variant="outline"
                  className="h-11"
                  disabled={scanning || !scanValue.trim()}
                >
                  <ScanLine className="mr-1 h-4 w-4" aria-hidden />
                  {t("issueDesk.scan.submit")}
                </Button>
              </div>
              <p id="inv-issue-scan-hint" className="text-xs text-text-muted">
                {t("issueDesk.scan.hint")}
              </p>
            </form>
            {scanError ? (
              <ErrorState error={scanError} title={t("issueDesk.scan.notFound")} />
            ) : null}
            {scan ? (
              <div className="flex flex-wrap items-center gap-2 rounded-md border border-surface-border bg-surface-2 p-3 text-sm">
                <span className="font-mono text-xs">{scan.article_code}</span>
                <span className="font-medium">{scan.article_name}</span>
                <StatusChip label={t(`issueDesk.scan.matched.${scan.matched_on}`)} tone="info" />
                <Button
                  type="button"
                  variant="ghost"
                  className="ml-auto h-11"
                  onClick={resetArticle}
                >
                  {t("issueDesk.article.clear")}
                </Button>
              </div>
            ) : null}

            <div className="space-y-1">
              <Label htmlFor="inv-issue-wo">{t("issueDesk.workOrder.label")}</Label>
              <WorkOrderReferencePicker
                id="inv-issue-wo"
                value={workOrderCode}
                onChange={setWorkOrderCode}
                onSelect={setWorkOrder}
              />
            </div>
          </div>

          <form
            onSubmit={(e) => void onIssue(e)}
            className="space-y-4"
            aria-label={t("issueDesk.formLabel")}
          >
            {contextError ? <ErrorState error={contextError} /> : null}
            {scan && workOrder && reservations.length > 0 ? (
              <div className="space-y-1">
                <Label htmlFor="inv-issue-reservation">{t("issueDesk.reservation.label")}</Label>
                <Select
                  value={reservationChoice}
                  onValueChange={(v) => v && setReservationChoice(v)}
                >
                  <SelectTrigger id="inv-issue-reservation" className="h-11">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DIRECT_ISSUE}>{t("issueDesk.reservation.none")}</SelectItem>
                    {reservations.map((r) => (
                      <SelectItem key={r.id} value={String(r.id)}>
                        {t("issueDesk.reservation.option", {
                          qty: qty(r.quantity_reserved - r.quantity_issued),
                          unit,
                          location: `${r.warehouse_code}/${r.location_code}`,
                        })}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ) : null}

            <div className="space-y-1">
              <Label htmlFor="inv-issue-location">{t("issueDesk.location.label")}</Label>
              <Select
                value={String(effectiveLocationId)}
                onValueChange={(v) => {
                  // Radix reports "" while its options mount; keep the current choice.
                  if (v) setLocationId(Number(v));
                }}
                disabled={!scan || selectedReservation !== null}
              >
                <SelectTrigger id="inv-issue-location" className="h-11">
                  <SelectValue placeholder={t("issueDesk.location.placeholder")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("issueDesk.location.placeholder")}</SelectItem>
                  {balances.map((b) => (
                    <SelectItem key={b.location_id} value={String(b.location_id)}>
                      {b.warehouse_code}/{b.location_code} —{" "}
                      {t("issueDesk.location.available", { qty: qty(b.available_qty), unit })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {scan && balances.length === 0 ? (
                <p className="text-xs text-text-muted">{t("issueDesk.location.empty")}</p>
              ) : null}
            </div>

            {tracked ? (
              <div className="space-y-1">
                <Label htmlFor="inv-issue-lot">{t("issueDesk.lot.label")}</Label>
                <Select
                  value={lotChoice}
                  onValueChange={(v) => v && setLotChoice(v)}
                  disabled={lots.length === 0}
                >
                  <SelectTrigger id="inv-issue-lot" className="h-11">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={AUTO_LOT}>
                      {fefo
                        ? t("issueDesk.lot.auto", { lot: lotLabel(fefo) })
                        : t("issueDesk.lot.autoNone")}
                    </SelectItem>
                    {lots.map((l) => (
                      <SelectItem key={l.lot_id} value={String(l.lot_id)}>
                        {t("issueDesk.lot.option", {
                          lot: lotLabel(l),
                          qty: qty(l.quantity),
                          unit,
                        })}
                        {l.expiry_date
                          ? ` · ${
                              l.days_to_expiry != null && l.days_to_expiry < 0
                                ? t("issueDesk.lot.expired")
                                : t("issueDesk.lot.expires", { date: formatDate(l.expiry_date) })
                            }`
                          : ""}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-text-muted">{t("issueDesk.lot.hint")}</p>
              </div>
            ) : null}

            <div className="space-y-1">
              <Label htmlFor="inv-issue-qty">
                {t("issueDesk.quantity.label")}
                {unit ? ` (${unit})` : ""}
              </Label>
              <Input
                id="inv-issue-qty"
                inputMode="decimal"
                className="h-11 tabular-nums"
                value={quantity}
                disabled={!scan}
                onChange={(e) => setQuantity(e.target.value)}
                aria-describedby={blocker ? "inv-issue-blocker" : undefined}
              />
            </div>

            {blocker ? (
              <p id="inv-issue-blocker" role="status" className="text-xs text-text-muted">
                {blocker}
              </p>
            ) : null}
            {issueError ? <ErrorState error={issueError} title={t("issueDesk.error")} /> : null}

            <PermissionGate permission={P.INV_MANAGE}>
              <Button type="submit" className="h-11 w-full" disabled={issuing || blocker !== null}>
                {issuing ? t("issueDesk.submitting") : t("issueDesk.submit")}
              </Button>
            </PermissionGate>

            {outcome ? (
              <div
                role="status"
                className="space-y-1 rounded-md border border-surface-border p-3 text-sm"
              >
                <p className="font-medium">
                  {t("issueDesk.outcome.title", {
                    code: outcome.articleCode,
                    wo: outcome.result.work_order_code ?? "",
                  })}
                </p>
                <ul className="space-y-0.5 text-text-secondary">
                  {outcome.result.lines.map((line) => (
                    <li key={line.transaction_id} className="tabular-nums">
                      {t("issueDesk.outcome.line", {
                        qty: qty(line.quantity),
                        unit: outcome.unit,
                        lot:
                          (line.serial_number ?? line.lot_number)
                            ? ` · ${line.serial_number ?? line.lot_number}`
                            : "",
                        cost: unitCostLabel(line.unit_cost),
                      })}
                    </li>
                  ))}
                </ul>
                {outcome.result.total_cost != null ? (
                  <p className="tabular-nums">
                    {t("issueDesk.outcome.total", {
                      total: formatCurrency(outcome.result.total_cost),
                    })}
                  </p>
                ) : null}
              </div>
            ) : null}
          </form>
        </div>
      </section>

      <section
        aria-labelledby="inv-expiring-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
          <h2 id="inv-expiring-title" className="text-base font-semibold">
            {t("issueDesk.expiring.title", { days: expiryWindow })}
          </h2>
          <div className="space-y-1">
            <Label htmlFor="inv-expiring-window">{t("issueDesk.expiring.window")}</Label>
            <Select
              value={String(expiryWindow)}
              onValueChange={(v) => v && setExpiryWindow(Number(v))}
            >
              <SelectTrigger id="inv-expiring-window" className="h-11 w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {EXPIRY_WINDOWS.map((d) => (
                  <SelectItem key={d} value={String(d)}>
                    {t("issueDesk.expiring.windowOption", { count: d })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        {expiringError ? (
          <ErrorState
            error={expiringError}
            title={t("issueDesk.expiring.loadError")}
            onRetry={() => setExpiringVersion((v) => v + 1)}
          />
        ) : !expiringLoading && expiring.length === 0 ? (
          <p className="text-sm text-text-muted">
            {t("issueDesk.expiring.empty", { days: expiryWindow })}
          </p>
        ) : (
          <DataTable
            columns={expiringColumns}
            data={expiring}
            isLoading={expiringLoading}
            searchable={false}
          />
        )}
      </section>
    </div>
  );
}
