/**
 * Purchasing rules: stored exchange rates (supplier currency → tenant currency, used at receipt
 * when no rate is entered) and per-role approval limits for requisitions and purchase orders.
 * Separation of duties (the requester never approves their own document) is enforced by the
 * backend on every approval; this panel only states it.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
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
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type ApprovalDocumentType,
  type CurrencyExchangeRate,
  type ProcurementApprovalLimit,
  createCurrencyExchangeRate,
  deactivateCurrencyExchangeRate,
  listCurrencyExchangeRates,
  listProcurementApprovalLimits,
  upsertProcurementApprovalLimit,
} from "@/services/inventory-service";
import { listRoles } from "@/services/rbac-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

const DOC_TYPES: readonly ApprovalDocumentType[] = ["REQUISITION", "PURCHASE_ORDER"];

/** ISO 4217 code check (three letters). */
export function isCurrencyCode(value: string): boolean {
  return /^[A-Za-z]{3}$/.test(value.trim());
}

/** Parses a decimal typed with a comma or a dot; NaN when not a number. */
export function parseDecimal(value: string): number {
  const trimmed = value.trim().replace(",", ".");
  return trimmed === "" ? Number.NaN : Number(trimmed);
}

interface RoleRow {
  id: number;
  name: string;
}

export function ProcurementRulesPanel({ reloadToken = 0 }: { reloadToken?: number }) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatCurrency, formatNumber, formatDate, currencyCode } = useFormatters();
  const rateLabel = useCallback(
    (v: number) => formatNumber(v, { maximumFractionDigits: 6 }),
    [formatNumber],
  );

  // ── Exchange rates ──────────────────────────────────────────────────────────
  const [rates, setRates] = useState<CurrencyExchangeRate[]>([]);
  const [ratesLoading, setRatesLoading] = useState(false);
  const [ratesError, setRatesError] = useState<unknown>(null);
  const [rateFormOpen, setRateFormOpen] = useState(false);
  const [rateFrom, setRateFrom] = useState("");
  const [rateValue, setRateValue] = useState("");
  const [rateValidFrom, setRateValidFrom] = useState("");
  const [rateSource, setRateSource] = useState("");
  const [rateSaving, setRateSaving] = useState(false);
  const [rateError, setRateError] = useState<unknown>(null);
  const [retiring, setRetiring] = useState<CurrencyExchangeRate | null>(null);

  const loadRates = useCallback(async () => {
    setRatesLoading(true);
    setRatesError(null);
    try {
      setRates(await listCurrencyExchangeRates());
    } catch (err) {
      setRatesError(err);
    } finally {
      setRatesLoading(false);
    }
  }, []);

  // ── Approval limits ─────────────────────────────────────────────────────────
  const [limits, setLimits] = useState<ProcurementApprovalLimit[]>([]);
  const [roles, setRoles] = useState<RoleRow[]>([]);
  const [limitsLoading, setLimitsLoading] = useState(false);
  const [limitsError, setLimitsError] = useState<unknown>(null);
  const [rolesError, setRolesError] = useState<unknown>(null);
  const [limitEdit, setLimitEdit] = useState<{ role: RoleRow; doc: ApprovalDocumentType } | null>(
    null,
  );
  const [limitAmount, setLimitAmount] = useState("");
  const [limitActive, setLimitActive] = useState(true);
  const [limitSaving, setLimitSaving] = useState(false);
  const [limitError, setLimitError] = useState<unknown>(null);

  // The full role list is `list_roles` (adm.roles). Without it the table shows the roles that
  // already have a limit (`list_procurement_approval_limits` returns their names): not asked.
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canListRoles = can(P.ADM_ROLES);

  const loadLimits = useCallback(async () => {
    setLimitsLoading(true);
    setLimitsError(null);
    setRolesError(null);
    try {
      setLimits(await listProcurementApprovalLimits());
    } catch (err) {
      setLimitsError(err);
    }
    if (!canListRoles) {
      setRoles([]);
      setLimitsLoading(false);
      return;
    }
    try {
      const roleRows = await listRoles();
      setRoles(
        roleRows
          .filter((r) => r.status !== "retired")
          .map((r) => ({ id: r.id, name: r.name }))
          .sort((a, b) => a.name.localeCompare(b.name)),
      );
    } catch (err) {
      // Without role administration rights the table shows the roles that have a limit.
      setRolesError(err);
    } finally {
      setLimitsLoading(false);
    }
  }, [canListRoles]);

  useEffect(() => {
    void loadRates();
  }, [loadRates, reloadToken]);

  useEffect(() => {
    if (!permissionsLoading) void loadLimits();
  }, [loadLimits, permissionsLoading, reloadToken]);

  const openRateForm = () => {
    setRateFrom("");
    setRateValue("");
    setRateValidFrom(new Date().toISOString().slice(0, 10));
    setRateSource("");
    setRateError(null);
    setRateFormOpen(true);
  };

  const parsedRate = parseDecimal(rateValue);
  const rateBlocker =
    !isCurrencyCode(rateFrom) || rateFrom.trim().toUpperCase() === currencyCode
      ? t("rules.fx.form.blockers.currency", { currency: currencyCode })
      : !(parsedRate > 0)
        ? t("rules.fx.form.blockers.rate")
        : !rateValidFrom
          ? t("rules.fx.form.blockers.date")
          : null;

  const saveRate = async () => {
    if (rateBlocker || rateSaving) return;
    setRateSaving(true);
    setRateError(null);
    try {
      await createCurrencyExchangeRate({
        from_currency: rateFrom.trim().toUpperCase(),
        to_currency: null,
        rate: parsedRate,
        valid_from: rateValidFrom,
        source: rateSource.trim() || null,
      });
      setRateFormOpen(false);
      pushAppToast({ title: t("rules.fx.saved"), variant: "success" });
      await loadRates();
    } catch (err) {
      setRateError(err);
    } finally {
      setRateSaving(false);
    }
  };

  const rateColumns = useMemo<ColumnDef<CurrencyExchangeRate>[]>(
    () => [
      {
        id: "pair",
        header: t("rules.fx.columns.pair"),
        cell: ({ row }) => (
          <span className="tabular-nums">
            {t("rules.fx.pair", {
              from: row.original.from_currency,
              rate: rateLabel(row.original.rate),
              to: row.original.to_currency,
            })}
          </span>
        ),
      },
      {
        accessorKey: "valid_from",
        header: t("rules.fx.columns.validFrom"),
        cell: ({ row }) => formatDate(row.original.valid_from),
      },
      {
        accessorKey: "source",
        header: t("rules.fx.columns.source"),
        cell: ({ row }) => row.original.source ?? "—",
      },
      {
        accessorKey: "created_by_name",
        header: t("rules.fx.columns.createdBy"),
        cell: ({ row }) => row.original.created_by_name ?? "—",
      },
      {
        accessorKey: "is_active",
        header: t("rules.fx.columns.status"),
        cell: ({ row }) =>
          row.original.is_active === 1 ? (
            <StatusChip label={t("rules.fx.active")} tone="success" />
          ) : (
            <StatusChip label={t("rules.fx.retired")} tone="neutral" />
          ),
      },
      {
        id: "actions",
        header: () => <span className="sr-only">{t("rules.fx.columns.actions")}</span>,
        cell: ({ row }) =>
          row.original.is_active === 1 ? (
            <PermissionGate permission={P.INV_PROCURE}>
              <Button
                variant="ghost"
                className="h-11"
                onClick={(e) => {
                  e.stopPropagation();
                  setRetiring(row.original);
                }}
              >
                {t("rules.fx.deactivate")}
              </Button>
            </PermissionGate>
          ) : null,
      },
    ],
    [formatDate, rateLabel, t],
  );

  // Roles shown: every role (when readable) plus any role that has a limit.
  const roleRows = useMemo<RoleRow[]>(() => {
    const byId = new Map<number, RoleRow>(roles.map((r) => [r.id, r]));
    for (const l of limits) {
      if (!byId.has(l.role_id)) byId.set(l.role_id, { id: l.role_id, name: l.role_name });
    }
    return [...byId.values()].sort((a, b) => a.name.localeCompare(b.name));
  }, [roles, limits]);

  const limitOf = (roleId: number, doc: ApprovalDocumentType) =>
    limits.find((l) => l.role_id === roleId && l.document_type === doc) ?? null;
  const configured = (doc: ApprovalDocumentType) =>
    limits.some((l) => l.document_type === doc && l.is_active === 1);

  const limitCell = (roleId: number, doc: ApprovalDocumentType) => {
    const limit = limitOf(roleId, doc);
    if (limit && limit.is_active === 1) {
      return limit.max_amount == null ? (
        <StatusChip label={t("rules.limits.unlimited")} tone="success" />
      ) : (
        <span className="tabular-nums">{formatCurrency(limit.max_amount)}</span>
      );
    }
    return configured(doc) ? (
      <StatusChip label={t("rules.limits.notAllowed")} tone="neutral" />
    ) : (
      <span className="text-text-muted">{t("rules.limits.noRule")}</span>
    );
  };

  const openLimit = (role: RoleRow, doc: ApprovalDocumentType) => {
    const existing = limitOf(role.id, doc);
    setLimitEdit({ role, doc });
    setLimitAmount(existing?.max_amount != null ? String(existing.max_amount) : "");
    setLimitActive(existing ? existing.is_active === 1 : true);
    setLimitError(null);
  };

  const parsedLimit = limitAmount.trim() === "" ? null : parseDecimal(limitAmount);
  const limitBlocker =
    parsedLimit !== null && !(parsedLimit >= 0) ? t("rules.limits.form.blocker") : null;

  const saveLimit = async () => {
    if (!limitEdit || limitBlocker || limitSaving) return;
    setLimitSaving(true);
    setLimitError(null);
    try {
      await upsertProcurementApprovalLimit({
        role_id: limitEdit.role.id,
        document_type: limitEdit.doc,
        max_amount: parsedLimit,
        is_active: limitActive,
      });
      setLimitEdit(null);
      pushAppToast({ title: t("rules.limits.saved"), variant: "success" });
      await loadLimits();
    } catch (err) {
      setLimitError(err);
    } finally {
      setLimitSaving(false);
    }
  };

  const limitColumns = useMemo<ColumnDef<RoleRow>[]>(
    () => [
      { accessorKey: "name", header: t("rules.limits.columns.role") },
      ...DOC_TYPES.map(
        (doc): ColumnDef<RoleRow> => ({
          id: doc,
          header: t(`rules.limits.docTypes.${doc}`),
          cell: ({ row }) => (
            <span className="flex flex-wrap items-center gap-2">
              {limitCell(row.original.id, doc)}
              <PermissionGate permission={P.ADM_ROLES}>
                <Button
                  variant="ghost"
                  className="h-11"
                  aria-label={t("rules.limits.editFor", {
                    role: row.original.name,
                    doc: t(`rules.limits.docTypes.${doc}`),
                  })}
                  onClick={(e) => {
                    e.stopPropagation();
                    openLimit(row.original, doc);
                  }}
                >
                  {t("rules.limits.edit")}
                </Button>
              </PermissionGate>
            </span>
          ),
        }),
      ),
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps -- cells read the latest limits
    [limits, t, formatCurrency],
  );

  return (
    <div className="space-y-6">
      <section
        aria-labelledby="inv-fx-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 id="inv-fx-title" className="text-base font-semibold">
              {t("rules.fx.title")}
            </h2>
            <p className="max-w-3xl text-sm text-text-muted">
              {t("rules.fx.description", { currency: currencyCode })}
            </p>
          </div>
          <PermissionGate permission={P.INV_PROCURE}>
            <Button className="h-11" onClick={openRateForm}>
              <Plus className="mr-1 h-4 w-4" aria-hidden />
              {t("rules.fx.add")}
            </Button>
          </PermissionGate>
        </div>
        {ratesError ? (
          <ErrorState
            error={ratesError}
            title={t("rules.fx.loadError")}
            onRetry={() => void loadRates()}
          />
        ) : !ratesLoading && rates.length === 0 ? (
          <p className="text-sm text-text-muted">{t("rules.fx.empty")}</p>
        ) : (
          <DataTable
            columns={rateColumns}
            data={rates}
            isLoading={ratesLoading}
            searchable={false}
          />
        )}
      </section>

      <section
        aria-labelledby="inv-limits-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-3">
          <h2 id="inv-limits-title" className="text-base font-semibold">
            {t("rules.limits.title")}
          </h2>
          <p className="max-w-3xl text-sm text-text-muted">
            {t("rules.limits.description", { currency: currencyCode })}
          </p>
          <p className="mt-1 max-w-3xl text-sm font-medium">{t("rules.limits.sod")}</p>
        </div>
        {limitsError ? (
          <ErrorState
            error={limitsError}
            title={t("rules.limits.loadError")}
            onRetry={() => void loadLimits()}
          />
        ) : (
          <>
            {rolesError ? (
              <p className="mb-2 text-xs text-text-muted">{t("rules.limits.rolesUnavailable")}</p>
            ) : null}
            {!limitsLoading && roleRows.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rules.limits.empty")}</p>
            ) : (
              <DataTable
                columns={limitColumns}
                data={roleRows}
                isLoading={limitsLoading}
                searchable={roleRows.length > 10}
              />
            )}
          </>
        )}
      </section>

      <Dialog open={rateFormOpen} onOpenChange={(open) => !rateSaving && setRateFormOpen(open)}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("rules.fx.add")}</DialogTitle>
            <DialogDescription>
              {t("rules.fx.form.description", { currency: currencyCode })}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="inv-fx-from">{t("rules.fx.form.from")}</Label>
              <Input
                id="inv-fx-from"
                className="h-11 font-mono uppercase"
                maxLength={3}
                value={rateFrom}
                onChange={(e) => setRateFrom(e.target.value.toUpperCase())}
                placeholder="EUR"
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-fx-rate">
                {t("rules.fx.form.rate", { currency: currencyCode })}
              </Label>
              <Input
                id="inv-fx-rate"
                inputMode="decimal"
                className="h-11 tabular-nums"
                value={rateValue}
                onChange={(e) => setRateValue(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-fx-valid-from">{t("rules.fx.form.validFrom")}</Label>
              <DateField
                id="inv-fx-valid-from"
                className="h-11"
                value={rateValidFrom}
                onChange={(v) => setRateValidFrom(v)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-fx-source">{t("rules.fx.form.source")}</Label>
              <Input
                id="inv-fx-source"
                className="h-11"
                value={rateSource}
                onChange={(e) => setRateSource(e.target.value)}
              />
            </div>
          </div>
          {rateBlocker ? (
            <p id="inv-fx-blocker" role="status" className="text-xs text-text-muted">
              {rateBlocker}
            </p>
          ) : (
            <p className="text-xs text-text-muted tabular-nums">
              {t("rules.fx.pair", {
                from: rateFrom.trim().toUpperCase(),
                rate: rateLabel(parsedRate),
                to: currencyCode,
              })}
            </p>
          )}
          {rateError ? <ErrorState error={rateError} title={t("rules.fx.form.error")} /> : null}
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={rateSaving}
              onClick={() => setRateFormOpen(false)}
            >
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={rateSaving || rateBlocker !== null}
              aria-describedby={rateBlocker ? "inv-fx-blocker" : undefined}
              onClick={() => void saveRate()}
            >
              {t("rules.fx.form.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={retiring !== null}
        onOpenChange={(open) => !open && setRetiring(null)}
        title={t("rules.fx.deactivateConfirm.title")}
        description={
          retiring
            ? t("rules.fx.deactivateConfirm.description", {
                from: retiring.from_currency,
                rate: rateLabel(retiring.rate),
                to: retiring.to_currency,
                date: formatDate(retiring.valid_from),
              })
            : ""
        }
        confirmLabel={t("rules.fx.deactivate")}
        destructive
        onConfirm={async () => {
          if (!retiring) return;
          await deactivateCurrencyExchangeRate(retiring.id);
          setRetiring(null);
          await loadRates();
        }}
      />

      <Dialog
        open={limitEdit !== null}
        onOpenChange={(open) => !open && !limitSaving && setLimitEdit(null)}
      >
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>
              {limitEdit
                ? t("rules.limits.form.title", {
                    role: limitEdit.role.name,
                    doc: t(`rules.limits.docTypes.${limitEdit.doc}`),
                  })
                : ""}
            </DialogTitle>
            <DialogDescription>{t("rules.limits.form.description")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="inv-limit-amount">
                {t("rules.limits.form.maxAmount", { currency: currencyCode })}
              </Label>
              <Input
                id="inv-limit-amount"
                inputMode="decimal"
                className="h-11 tabular-nums"
                value={limitAmount}
                onChange={(e) => setLimitAmount(e.target.value)}
                aria-describedby="inv-limit-amount-hint"
              />
              <p id="inv-limit-amount-hint" className="text-xs text-text-muted">
                {t("rules.limits.form.maxHint")}
              </p>
            </div>
            <div className="flex min-h-11 items-center gap-3">
              <Switch
                id="inv-limit-active"
                checked={limitActive}
                onCheckedChange={setLimitActive}
              />
              <Label htmlFor="inv-limit-active">{t("rules.limits.form.active")}</Label>
            </div>
          </div>
          {limitBlocker ? (
            <p role="status" className="text-xs text-text-muted">
              {limitBlocker}
            </p>
          ) : null}
          {limitError ? (
            <ErrorState error={limitError} title={t("rules.limits.form.error")} />
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={limitSaving}
              onClick={() => setLimitEdit(null)}
            >
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={limitSaving || limitBlocker !== null}
              onClick={() => void saveLimit()}
            >
              {t("rules.limits.form.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
