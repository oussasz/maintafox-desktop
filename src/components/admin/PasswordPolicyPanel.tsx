import { type FormEvent, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getPasswordPolicy, updateRbacSetting } from "@/services/rbac-service";
import type { PasswordPolicySettings } from "@shared/ipc-types";

/**
 * Admin panel for configuring password policy settings.
 * Reads/writes from rbac_settings table. Requires adm.settings permission.
 */
export function PasswordPolicyPanel() {
  const { t } = useTranslation("admin");
  const [policy, setPolicy] = useState<PasswordPolicySettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const load = useCallback(async () => {
    try {
      const data = await getPasswordPolicy();
      setPolicy(data);
    } catch {
      // keep previous data
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!policy) return;

    setSaving(true);
    setError(null);
    setSuccess(false);

    try {
      await Promise.all([
        updateRbacSetting("password_max_age_days", String(policy.max_age_days)),
        updateRbacSetting("password_warn_days", String(policy.warn_days_before_expiry)),
        updateRbacSetting("password_min_length", String(policy.min_length)),
        updateRbacSetting("password_require_uppercase", policy.require_uppercase ? "1" : "0"),
        updateRbacSetting("password_require_lowercase", policy.require_lowercase ? "1" : "0"),
        updateRbacSetting("password_require_digit", policy.require_digit ? "1" : "0"),
        updateRbacSetting("password_require_special", policy.require_special ? "1" : "0"),
      ]);
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("passwordPolicy.saveError"));
    } finally {
      setSaving(false);
    }
  };

  if (!policy) {
    return (
      <div className="flex h-32 items-center justify-center">
        <div className="h-5 w-5 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  const update = <K extends keyof PasswordPolicySettings>(key: K, val: PasswordPolicySettings[K]) =>
    setPolicy((p) => (p ? { ...p, [key]: val } : p));

  return (
    <div className="rounded-lg border border-surface-border bg-surface-1 p-6">
      <h2 className="text-lg font-semibold text-text-primary">{t("passwordPolicy.title")}</h2>

      <form onSubmit={handleSubmit} className="mt-4 max-w-md space-y-4">
        <NumberField
          label={t("passwordPolicy.maxAgeDays")}
          hint={t("passwordPolicy.maxAgeDaysHint")}
          value={policy.max_age_days}
          onChange={(v) => update("max_age_days", v)}
          min={0}
          max={365}
        />
        <NumberField
          label={t("passwordPolicy.warnDays")}
          value={policy.warn_days_before_expiry}
          onChange={(v) => update("warn_days_before_expiry", v)}
          min={0}
          max={90}
        />
        <NumberField
          label={t("passwordPolicy.minLength")}
          value={policy.min_length}
          onChange={(v) => update("min_length", v)}
          min={4}
          max={64}
        />

        <div className="space-y-2">
          <p className="text-sm font-medium text-text-primary">{t("passwordPolicy.complexity")}</p>
          <Toggle
            label={t("passwordPolicy.requireUppercase")}
            checked={policy.require_uppercase}
            onChange={(v) => update("require_uppercase", v)}
          />
          <Toggle
            label={t("passwordPolicy.requireLowercase")}
            checked={policy.require_lowercase}
            onChange={(v) => update("require_lowercase", v)}
          />
          <Toggle
            label={t("passwordPolicy.requireDigit")}
            checked={policy.require_digit}
            onChange={(v) => update("require_digit", v)}
          />
          <Toggle
            label={t("passwordPolicy.requireSpecial")}
            checked={policy.require_special}
            onChange={(v) => update("require_special", v)}
          />
        </div>

        <div className="rounded-md bg-surface-2 p-3">
          <p className="mb-1 text-xs font-medium text-text-muted">{t("passwordPolicy.preview")}</p>
          <p className="text-sm text-text-secondary">
            {t("passwordPolicy.previewText", { len: policy.min_length })}
            {policy.require_uppercase ? ` + ${t("passwordPolicy.upper")}` : ""}
            {policy.require_lowercase ? ` + ${t("passwordPolicy.lower")}` : ""}
            {policy.require_digit ? ` + ${t("passwordPolicy.digit")}` : ""}
            {policy.require_special ? ` + ${t("passwordPolicy.special")}` : ""}
            {policy.max_age_days > 0
              ? `. ${t("passwordPolicy.expiresEvery", { days: policy.max_age_days })}`
              : `. ${t("passwordPolicy.noExpiry")}`}
          </p>
        </div>

        {error && <p className="text-sm text-status-danger">{error}</p>}
        {success && <p className="text-sm text-emerald-600">{t("passwordPolicy.saved")}</p>}

        <button type="submit" disabled={saving} className="btn-primary text-sm">
          {saving ? t("passwordPolicy.saving") : t("passwordPolicy.save")}
        </button>
      </form>
    </div>
  );
}

function NumberField({
  label,
  hint,
  value,
  onChange,
  min,
  max,
}: {
  label: string;
  hint?: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
}) {
  return (
    <div>
      <label className="flex items-center justify-between">
        <span className="text-sm text-text-primary">{label}</span>
        <input
          type="number"
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
          min={min}
          max={max}
          className="w-20 rounded-md border border-surface-border bg-surface-0
                     px-2 py-1 text-right text-sm text-text-primary
                     focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
      </label>
      {hint && <p className="mt-0.5 text-xs text-text-muted">{hint}</p>}
    </div>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-2">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 rounded border-surface-border text-primary
                   focus:ring-primary focus:ring-offset-0"
      />
      <span className="text-sm text-text-secondary">{label}</span>
    </label>
  );
}
