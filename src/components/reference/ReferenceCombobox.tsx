/**
 * ReferenceCombobox — standard selector for values managed in Données de référence.
 *
 * Governance (A/B/C): docs/engineering/REFERENCE_GOVERNANCE.md
 * - Create CTA only when capabilities.can_operational_create && ref.manage.
 * - After create: refresh options and auto-select the new value (code or id).
 *
 * See docs/engineering/REFERENCE_COMBOBOX.md
 */

import { Check, ChevronsUpDown, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceCreateModal } from "@/components/reference/ReferenceCreateModal";
import {
  addOptionLabel,
  createButtonLabel,
  emptyStateLabel,
  getReferenceTypeConfig,
  type ReferenceTypeId,
} from "@/components/reference/reference-types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  getReferenceGovernanceCapabilitiesByCode,
  listPublishedReferenceValuesByDomainCode,
} from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type { ReferenceGovernanceCapabilities, ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type ReferenceValueMode = "code" | "id";

export interface ReferenceComboboxProps {
  referenceType: ReferenceTypeId;
  /** Controlled value: domain code (default) or stringified reference_values.id. */
  value: string | null;
  onChange: (value: string | null) => void;
  /** Parent reference_values.id when the type is hierarchical. */
  parentValueId?: number | null;
  /**
   * Alternative to parentValueId: resolve parent id from the parent domain by code.
   * Preferred when the form only stores codes (e.g. Asset family → subfamily).
   */
  parentCode?: string | null;
  parentLabel?: string | null;
  label?: string;
  placeholder?: string;
  disabled?: boolean;
  /** When false, hides create action even if governance allows it. Default true. */
  allowCreate?: boolean;
  /** Show a "none" option that clears the value. Default true. */
  allowClear?: boolean;
  /**
   * Form value shape. `"code"` (default) for most APIs; `"id"` when the form
   * stores reference_values.id (e.g. DI symptom_code_id).
   */
  valueMode?: ReferenceValueMode;
  /** Domain codes to hide from the dropdown (e.g. convert-only dispositions). */
  excludeCodes?: readonly string[];
  className?: string;
  "aria-invalid"?: boolean;
  id?: string;
}

function optionKey(o: ReferenceValue, mode: ReferenceValueMode): string {
  return mode === "id" ? String(o.id) : o.code;
}

export function ReferenceCombobox({
  referenceType,
  value,
  onChange,
  parentValueId = null,
  parentCode = null,
  parentLabel = null,
  placeholder = "",
  disabled = false,
  allowCreate = true,
  allowClear = true,
  valueMode = "code",
  excludeCodes,
  className,
  "aria-invalid": ariaInvalid,
  id,
}: ReferenceComboboxProps) {
  const { t, i18n } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const cfg = getReferenceTypeConfig(referenceType);

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<ReferenceValue[]>([]);
  const [caps, setCaps] = useState<ReferenceGovernanceCapabilities | null>(null);
  const [resolvedParentId, setResolvedParentId] = useState<number | null>(null);
  const [resolvedParentLabel, setResolvedParentLabel] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const reloadToken = useRef(0);

  const needsParent = Boolean(cfg.parentDomainCode);
  const effectiveParentId = parentValueId ?? resolvedParentId;
  const effectiveParentLabel = parentLabel ?? resolvedParentLabel;
  const parentReady = !needsParent || (effectiveParentId != null && effectiveParentId > 0);
  const canShowCreate =
    allowCreate && Boolean(caps?.can_operational_create) && canManage && parentReady && !disabled;

  useEffect(() => {
    let cancelled = false;
    getReferenceGovernanceCapabilitiesByCode(cfg.domainCode, null)
      .then((c) => {
        if (!cancelled) setCaps(c);
      })
      .catch(() => {
        if (!cancelled) setCaps(null);
      });
    return () => {
      cancelled = true;
    };
  }, [cfg.domainCode]);

  useEffect(() => {
    if (!needsParent || !cfg.parentDomainCode) {
      setResolvedParentId(null);
      setResolvedParentLabel(null);
      return;
    }
    if (parentValueId != null && parentValueId > 0) {
      setResolvedParentId(parentValueId);
      return;
    }
    const code = parentCode?.trim();
    if (!code) {
      setResolvedParentId(null);
      setResolvedParentLabel(null);
      return;
    }
    let cancelled = false;
    void listPublishedReferenceValuesByDomainCode(cfg.parentDomainCode)
      .then((rows) => {
        if (cancelled) return;
        const match = rows.find((r) => r.code === code) ?? null;
        setResolvedParentId(match?.id ?? null);
        setResolvedParentLabel(match?.label ?? null);
      })
      .catch(() => {
        if (cancelled) return;
        setResolvedParentId(null);
        setResolvedParentLabel(null);
      });
    return () => {
      cancelled = true;
    };
  }, [cfg.parentDomainCode, needsParent, parentCode, parentValueId]);

  const loadItems = useCallback(async () => {
    const token = ++reloadToken.current;
    setLoading(true);
    setLoadError(null);
    try {
      const rows = await listPublishedReferenceValuesByDomainCode(cfg.domainCode);
      if (token !== reloadToken.current) return;
      const filtered = needsParent ? rows.filter((r) => r.parent_id === effectiveParentId) : rows;
      setItems(filtered);
    } catch (err) {
      if (token !== reloadToken.current) return;
      setItems([]);
      setLoadError(toErrorMessage(err));
    } finally {
      if (token === reloadToken.current) {
        setLoading(false);
      }
    }
  }, [cfg.domainCode, effectiveParentId, needsParent]);

  useEffect(() => {
    void loadItems();
  }, [loadItems]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const selected = useMemo(
    () => items.find((o) => optionKey(o, valueMode) === value) ?? null,
    [items, value, valueMode],
  );

  const filtered = useMemo(() => {
    const excluded = new Set(excludeCodes ?? []);
    const visible = excluded.size > 0 ? items.filter((o) => !excluded.has(o.code)) : items;
    const q = query.trim().toLowerCase();
    if (!q) return visible;
    return visible.filter((o) => {
      const hay = `${o.label} ${o.code}`.toLowerCase();
      return hay.includes(q);
    });
  }, [items, query, excludeCodes]);

  const handlePick = useCallback(
    (next: string | null) => {
      onChange(next);
      setOpen(false);
      setQuery("");
    },
    [onChange],
  );

  const handleCreated = useCallback(
    async (created: ReferenceValue) => {
      await loadItems();
      onChange(optionKey(created, valueMode));
      setCreateOpen(false);
      setOpen(false);
      setQuery("");
    },
    [loadItems, onChange, valueMode],
  );

  const displayLabel = selected ? selected.label : value ? value : "";

  const isDisabled = disabled || (needsParent && !parentReady);

  return (
    <div ref={rootRef} className={cn("relative w-full", className)}>
      <Button
        id={id}
        type="button"
        variant="outline"
        role="combobox"
        aria-expanded={open}
        aria-invalid={ariaInvalid}
        disabled={isDisabled}
        className={cn(
          "h-auto min-h-9 w-full justify-between font-normal px-3 py-2 text-left",
          !selected && "text-text-muted",
        )}
        onClick={() => !isDisabled && setOpen((o) => !o)}
      >
        <span className="truncate">
          {selected || value ? displayLabel : placeholder || emptyStateLabel(cfg, i18n.language)}
        </span>
        <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
      </Button>

      {open && (
        <div className="absolute left-0 right-0 top-full z-50 mt-1 rounded-md border border-surface-border bg-surface-1 p-2 shadow-lg">
          <Input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={placeholder || t("combobox.searchPlaceholder")}
            className="mb-2 h-8 text-sm"
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setOpen(false);
                setQuery("");
              }
            }}
          />

          {loadError && (
            <p className="mb-2 px-1 text-xs text-destructive" role="alert">
              {loadError}
            </p>
          )}

          <ul className="max-h-60 overflow-auto rounded-sm border border-surface-border/80">
            {loading ? (
              <li className="px-2 py-3 text-center text-xs text-text-muted">
                {t("combobox.loading")}
              </li>
            ) : filtered.length === 0 ? (
              <li className="space-y-2 px-2 py-3 text-center">
                <p className="text-xs text-text-muted">{emptyStateLabel(cfg, i18n.language)}</p>
                {canShowCreate ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="default"
                    className="gap-1"
                    onClick={() => {
                      setOpen(false);
                      setCreateOpen(true);
                    }}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {createButtonLabel(cfg, i18n.language)}
                  </Button>
                ) : null}
              </li>
            ) : (
              <>
                {allowClear && (
                  <li>
                    <button
                      type="button"
                      className={cn(
                        "flex w-full items-start gap-2 px-2 py-2 text-left text-sm hover:bg-muted",
                        value == null && "bg-muted/80",
                      )}
                      onClick={() => handlePick(null)}
                    >
                      <Check
                        className={cn(
                          "mt-0.5 h-4 w-4 shrink-0",
                          value == null ? "opacity-100" : "opacity-0",
                        )}
                      />
                      <span className="text-text-muted">{t("combobox.noneOption")}</span>
                    </button>
                  </li>
                )}
                {filtered.map((o) => {
                  const key = optionKey(o, valueMode);
                  return (
                    <li key={o.id}>
                      <button
                        type="button"
                        className={cn(
                          "flex w-full items-start gap-2 px-2 py-2 text-left text-sm hover:bg-muted",
                          key === value && "bg-muted/80",
                        )}
                        onClick={() => handlePick(key)}
                      >
                        <Check
                          className={cn(
                            "mt-0.5 h-4 w-4 shrink-0",
                            key === value ? "opacity-100" : "opacity-0",
                          )}
                        />
                        <span className="block font-medium leading-tight">{o.label}</span>
                      </button>
                    </li>
                  );
                })}
                {canShowCreate && (
                  <li className="border-t border-surface-border/80">
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 px-2 py-2 text-left text-sm font-medium text-primary hover:bg-muted"
                      onClick={() => {
                        setOpen(false);
                        setCreateOpen(true);
                      }}
                    >
                      <Plus className="h-4 w-4 shrink-0" />
                      <span>{addOptionLabel(cfg, i18n.language)}</span>
                    </button>
                  </li>
                )}
              </>
            )}
          </ul>
        </div>
      )}

      <ReferenceCreateModal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        referenceType={referenceType}
        parentValueId={effectiveParentId}
        parentLabel={effectiveParentLabel}
        onCreated={(v) => void handleCreated(v)}
      />
    </div>
  );
}
