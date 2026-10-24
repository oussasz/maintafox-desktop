import { History, Lock, RotateCcw, ShieldCheck } from "lucide-react";
import { type ReactElement, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { useStepUp } from "@/hooks/use-step-up";
import { applyI18nFallbackLanguage } from "@/lib/i18n-fallback";
import { listSettingChangeEvents, setSetting } from "@/services/settings-service";
import { useLocaleStore } from "@/stores/locale-store";
import type { CurrencyOption, SettingDefinitionView, SettingsChangeEvent } from "@shared/ipc-types";

type Translate = (key: string, options?: Record<string, unknown>) => string;

/** Shortest reason accepted for a high-risk change (same rule as the server). */
export const MIN_REASON_CHARS = 10;

function parseJson(raw: string | null): unknown {
  if (raw == null) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}

/** Human-readable value of a setting (language names, month names, units). */
export function formatSettingValue(
  def: Pick<SettingDefinitionView, "key" | "kind">,
  raw: string | null,
  t: Translate,
): string {
  if (raw == null) return def.kind === "optional_decimal" ? t("catalog.notSet") : "—";
  const value = parseJson(raw);
  switch (def.kind) {
    case "choice":
      return typeof value === "string" ? t(`options.language.${value}`) : String(value);
    case "month": {
      const n = Number(value);
      return Number.isInteger(n) && n >= 1 && n <= 12 ? t(`options.month.${n}`) : String(value);
    }
    case "integer":
      return t(`catalog.units.${def.key}`, { count: Number(value) });
    case "decimal":
      // The unit text formats the number in the interface language (`{{value, number}}`).
      return t(`catalog.units.${def.key}`, { value: Number(value) });
    case "optional_decimal":
      // Blank (no value, shipped `null`) is never zero.
      return value == null
        ? t("catalog.notSet")
        : t(`catalog.units.${def.key}`, { value: Number(value) });
    case "currency":
      return String(value);
    default:
      return raw;
  }
}

/** Stored JSON for an edited value of this kind. */
function toValueJson(def: SettingDefinitionView, input: string): string {
  if (def.kind === "optional_decimal") {
    // Blank clears the value (the server deletes the row); it is never sent as zero.
    return input.trim() === "" ? "null" : String(Number(input));
  }
  if (def.kind === "integer" || def.kind === "decimal" || def.kind === "month") {
    return String(Number(input));
  }
  return JSON.stringify(input);
}

/** "DZD — Dinar algérien (DA)" in the interface language, from Données de référence. */
export function currencyOptionLabel(c: CurrencyOption, locale: string): string {
  const name = locale === "en" ? (c.label_en ?? c.label) : c.label;
  return `${c.code} — ${name}${c.symbol ? ` (${c.symbol})` : ""}`;
}

/**
 * Currencies of Données de référence › Devises for the tenant currency picker: `options` is null
 * until the list is loaded; `error` is the failed load (the picker then stays disabled).
 */
export interface CurrencyListState {
  options: CurrencyOption[] | null;
  error: unknown;
  onRetry: () => void;
}

interface RowProps {
  def: SettingDefinitionView;
  currencies: CurrencyListState;
  canEdit: boolean;
  onSaved: () => void;
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

function SettingRow({ def, currencies, canEdit, onSaved, onToast }: RowProps) {
  const { t: tTyped } = useTranslation("settings");
  const t = tTyped as unknown as Translate;
  const { formatDateTime } = useFormatters();
  const setLocale = useLocaleStore((s) => s.setLocale);
  const setCurrencyCode = useLocaleStore((s) => s.setCurrencyCode);
  const activeLocale = useLocaleStore((s) => s.activeLocale);
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [history, setHistory] = useState<SettingsChangeEvent[] | null>(null);
  const [historyError, setHistoryError] = useState<unknown>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const { withStepUp, StepUpDialogElement } = useStepUp();
  /** A high-risk change waiting for its reason (confirmation dialog). */
  const [pendingHighRisk, setPendingHighRisk] = useState<string | null>(null);
  const [reason, setReason] = useState("");

  const current = parseJson(def.current_value_json);
  const currentText = current == null ? "" : String(current);
  const locked = def.locked_reason != null;
  // The currency picker waits for the list: never offer a partial or empty choice.
  const currencyUnavailable = def.kind === "currency" && currencies.options == null;
  const disabled = !canEdit || locked || saving || currencyUnavailable;

  const loadHistory = useCallback(async () => {
    setHistoryError(null);
    try {
      setHistory(await listSettingChangeEvents(20, def.key));
    } catch (err) {
      setHistory(null);
      setHistoryError(err);
    }
  }, [def.key]);

  /** Writes the value (step-up when the server asks for it); throws when refused. */
  const persist = async (valueJson: string, summary: string) => {
    setSaving(true);
    try {
      await withStepUp(() =>
        setSetting({
          key: def.key,
          scope: def.scope,
          value_json: valueJson,
          change_summary: summary,
        }),
      );
      const parsed = parseJson(valueJson);
      if (def.key === "locale.primary_language" && typeof parsed === "string") {
        await setLocale(parsed, true);
      }
      if (def.key === "locale.fallback_language" && typeof parsed === "string") {
        applyI18nFallbackLanguage(parsed);
      }
      if (def.key === "locale.currency_code" && typeof parsed === "string") {
        setCurrencyCode(parsed);
      }
      setDraft(null);
      onToast({ title: t("editor.saveSuccess"), variant: "success" });
      if (historyOpen) void loadHistory();
      onSaved();
    } finally {
      setSaving(false);
    }
  };

  const save = async (valueJson: string, summary: string) => {
    try {
      await persist(valueJson, summary);
    } catch {
      // The IPC layer reports the server's reason (validation, lock).
    }
  };

  /** A high-risk setting asks for the person's reason first (it is audited with the change). */
  const requestSave = (valueJson: string, summary: string) => {
    if (def.risk === "high") {
      setReason("");
      setPendingHighRisk(valueJson);
      return;
    }
    void save(valueJson, summary);
  };

  const commitDraft = () => {
    if (draft == null) return;
    if (draft.trim() === currentText) {
      setDraft(null);
      return;
    }
    const blankOptional = def.kind === "optional_decimal" && draft.trim() === "";
    if (
      (def.kind === "integer" || def.kind === "decimal" || def.kind === "optional_decimal") &&
      !blankOptional
    ) {
      const n = Number(draft);
      if (
        draft.trim() === "" ||
        !Number.isFinite(n) ||
        (def.kind === "integer" && !Number.isInteger(n)) ||
        (def.min != null && n < def.min) ||
        (def.max != null && n > def.max)
      ) {
        onToast({
          title: t("catalog.outOfRange", { min: def.min ?? "", max: def.max ?? "" }),
          variant: "destructive",
        });
        return;
      }
    }
    requestSave(toValueJson(def, draft.trim()), t("editor.auditUpdated", { key: def.key }));
  };

  const label = t(`keys.${def.key}`);
  const inputId = `setting-${def.key.replace(/\W/g, "-")}`;

  let control: ReactElement;
  if (def.kind === "choice" || def.kind === "month" || def.kind === "currency") {
    const options =
      def.kind === "choice"
        ? def.options.map((o) => ({ value: o, label: t(`options.language.${o}`) }))
        : def.kind === "month"
          ? Array.from({ length: 12 }, (_, i) => ({
              value: String(i + 1),
              label: t(`options.month.${i + 1}`),
            }))
          : (currencies.options ?? [])
              .filter((c) => c.is_active || c.code === currentText)
              .map((c) => ({ value: c.code, label: currencyOptionLabel(c, activeLocale) }));
    control = (
      <Select
        value={currentText}
        onValueChange={(v) =>
          requestSave(toValueJson(def, v), t("editor.auditUpdated", { key: def.key }))
        }
        disabled={disabled}
      >
        <SelectTrigger id={inputId} className="h-9 w-full" aria-label={label}>
          {currencyUnavailable && currencies.error == null ? (
            <span className="text-text-muted">{t("catalog.currencyLoading")}</span>
          ) : (
            <SelectValue />
          )}
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  } else {
    control = (
      <Input
        id={inputId}
        type="number"
        inputMode={
          def.kind === "decimal" || def.kind === "optional_decimal" ? "decimal" : "numeric"
        }
        {...(def.kind === "decimal" || def.kind === "optional_decimal" ? { step: "any" } : {})}
        {...(def.kind === "optional_decimal" ? { placeholder: t("catalog.notSet") } : {})}
        className="h-9 w-full"
        aria-label={label}
        value={draft ?? currentText}
        {...(def.min != null ? { min: def.min } : {})}
        {...(def.max != null ? { max: def.max } : {})}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commitDraft}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") setDraft(null);
        }}
      />
    );
  }

  return (
    <li className="space-y-2 py-3 first:pt-1 last:pb-1">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 flex-1 space-y-0.5">
          <label htmlFor={inputId} className="text-sm font-medium text-text-primary">
            {label}
          </label>
          <p className="text-xs text-text-secondary">{t(`catalog.help.${def.key}`)}</p>
          <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-secondary">
            <span>
              {t("catalog.shipped")}:{" "}
              <strong>{formatSettingValue(def, def.shipped_value_json, t)}</strong>
            </span>
            {def.differs_from_shipped ? (
              <StatusChip label={t("catalog.changed")} tone="info" />
            ) : null}
            {def.last_modified_by ? (
              <span>
                {t("catalog.lastChange", {
                  who: def.last_modified_by,
                  when: def.last_modified_at ? formatDateTime(def.last_modified_at) : "",
                })}
              </span>
            ) : null}
          </p>
        </div>
        <div className="flex w-full items-center gap-2 sm:w-[16rem]">
          <div className="min-w-0 flex-1">{control}</div>
          {canEdit && !locked && def.differs_from_shipped ? (
            <Button
              variant="ghost"
              size="sm"
              title={t("catalog.restoreShipped")}
              aria-label={t("catalog.restoreShipped")}
              disabled={saving || currencyUnavailable}
              onClick={() =>
                requestSave(def.shipped_value_json, t("catalog.restoredSummary", { key: def.key }))
              }
            >
              <RotateCcw className="h-4 w-4" aria-hidden />
            </Button>
          ) : null}
        </div>
      </div>

      {def.kind === "currency" && currencies.error != null ? (
        <ErrorState
          error={currencies.error}
          title={t("catalog.currencyLoadError")}
          onRetry={currencies.onRetry}
          className="px-4 py-4"
        />
      ) : null}

      {locked ? (
        <p className="flex items-start gap-1.5 rounded-md border border-tone-attention-border bg-tone-attention-bg px-2.5 py-1.5 text-xs text-tone-attention-fg">
          <Lock className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          {t(`catalog.locked.${def.key}`)}
        </p>
      ) : null}

      <ConfirmDialog
        open={pendingHighRisk != null}
        onOpenChange={(open) => {
          if (!open) setPendingHighRisk(null);
        }}
        title={t("catalog.highRisk.title", { setting: label })}
        description={t("catalog.highRisk.body", {
          from: formatSettingValue(def, def.current_value_json, t),
          to: formatSettingValue(def, pendingHighRisk, t),
        })}
        confirmLabel={t("catalog.highRisk.confirm")}
        confirmDisabled={reason.trim().length < MIN_REASON_CHARS}
        onConfirm={async () => {
          if (pendingHighRisk != null) await persist(pendingHighRisk, reason.trim());
        }}
      >
        <label className="grid gap-1.5 text-sm">
          <span className="text-text-secondary">{t("catalog.highRisk.reason")}</span>
          <Textarea
            rows={3}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            aria-describedby={`${inputId}-reason-hint`}
          />
          <span id={`${inputId}-reason-hint`} className="text-xs text-text-muted">
            {t("catalog.highRisk.reasonHint", { min: MIN_REASON_CHARS })}
          </span>
        </label>
      </ConfirmDialog>
      {StepUpDialogElement}

      <div>
        <button
          type="button"
          className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          aria-expanded={historyOpen}
          onClick={() => {
            const next = !historyOpen;
            setHistoryOpen(next);
            if (next) void loadHistory();
          }}
        >
          <History className="h-3.5 w-3.5" aria-hidden />
          {historyOpen ? t("catalog.hideHistory") : t("catalog.showHistory")}
        </button>
        {historyOpen ? (
          historyError != null ? (
            <ErrorState
              error={historyError}
              title={t("catalog.historyLoadError")}
              onRetry={() => void loadHistory()}
              className="mt-1 px-4 py-4"
            />
          ) : history == null ? (
            <p className="mt-1 text-xs text-text-secondary">{t("page.loading")}</p>
          ) : history.length === 0 ? (
            <p className="mt-1 text-xs text-text-secondary">{t("auditLog.empty")}</p>
          ) : (
            <ul className="mt-1 space-y-1 border-l-2 border-surface-border pl-3 text-xs">
              {history.map((h) => (
                <li key={h.id} className="text-text-secondary">
                  <span className="text-text-primary">{formatDateTime(h.changed_at)}</span>
                  {" · "}
                  {h.changed_by_name ?? t("catalog.system")}
                  {" · "}
                  {h.old_value_json != null || h.new_value_json != null ? (
                    <span>
                      {formatSettingValue(def, h.old_value_json, t)} →{" "}
                      {formatSettingValue(def, h.new_value_json, t)}
                      {h.required_step_up ? ` · ${h.change_summary}` : null}
                    </span>
                  ) : (
                    <span>{h.change_summary}</span>
                  )}
                </li>
              ))}
            </ul>
          )
        ) : null}
      </div>
    </li>
  );
}

interface SettingsCatalogSectionsProps {
  categories: readonly string[];
  definitions: SettingDefinitionView[];
  currencies: CurrencyListState;
  rolesAllowed: string[];
  canEdit: boolean;
  onSaved: () => void;
  onToast: RowProps["onToast"];
}

/**
 * Settings of the given sections, from the server catalog: current vs shipped value, who may
 * change it, lock state, and per-setting change history. Only settings the application reads.
 */
export function SettingsCatalogSections({
  categories,
  definitions,
  currencies,
  rolesAllowed,
  canEdit,
  onSaved,
  onToast,
}: SettingsCatalogSectionsProps) {
  const { t: tTyped } = useTranslation("settings");
  const t = tTyped as unknown as Translate;
  return (
    <div className="flex flex-col gap-4">
      {categories.map((category) => {
        const rows = definitions.filter((d) => d.category === category && d.editor === "section");
        if (rows.length === 0) return null;
        return (
          <Card key={category} className="shadow-none">
            <CardHeader className="space-y-1 px-5 py-4">
              <CardTitle className="text-base font-semibold">
                {t(`categories.${category}`)}
              </CardTitle>
              <p className="flex flex-wrap items-center gap-1.5 text-xs text-text-secondary">
                <ShieldCheck className="h-3.5 w-3.5" aria-hidden />
                {rolesAllowed.length > 0
                  ? t("catalog.whoMayChange", { roles: rolesAllowed.join(", ") })
                  : t("catalog.nobodyMayChange")}
              </p>
            </CardHeader>
            <CardContent className="px-5 pb-4 pt-0">
              <ul className="divide-y divide-border">
                {rows.map((def) => (
                  <SettingRow
                    key={def.key}
                    currencies={currencies}
                    def={def}
                    canEdit={canEdit}
                    onSaved={onSaved}
                    onToast={onToast}
                  />
                ))}
              </ul>
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}
