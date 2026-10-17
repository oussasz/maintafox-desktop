/**
 * Notifications (destinataires et escalade) — the plant's notification settings (COMPANY).
 *
 * The categories, their module and severity are fixed by Maintafox (SYSTEM list
 * `NOTIFICATION.CATEGORIES`): none can be added or removed here. The plant chooses, per category,
 * whether it is raised, who receives it (the people the record names, their organisation, or the
 * holders of a permission), whether it must be acknowledged and which escalation applies; the
 * escalation delays and targets of each level; and how long notifications are kept. The shipped
 * values are shown as they are, so nothing works on a hidden default.
 */

import type { TFunction } from "i18next";
import { Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { categoryLabel, looseT } from "@/components/notifications/notification-text";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type EscalationStepInput,
  type NotificationConfig,
  type NotificationEscalationStep,
  type NotificationRuleConfig,
  type UpdateNotificationRuleInput,
  addNotificationEscalationStep,
  getNotificationConfig,
  removeNotificationEscalationStep,
  setNotificationRetention,
  updateNotificationEscalationStep,
  updateNotificationRule,
} from "@/services/notification-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import { P, PERMISSION_CATALOG } from "@shared/rbac/permissions.generated";

const PERMISSION_PREFIX = "permission:";
const NONE = "__none__";
const PERMISSION_OPTION = "__permission__";
const PERMISSION_NAMES = PERMISSION_CATALOG.map((p) => p.name).sort();

/** Whole minutes > 0, else null. */
function minutes(raw: string): number | null {
  const n = Number(raw.trim());
  return Number.isInteger(n) && n > 0 ? n : null;
}

export function NotificationRulesHost() {
  const { t } = useTranslation("notifications");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const [config, setConfig] = useState<NotificationConfig | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setConfig(await getNotificationConfig());
    } catch (err) {
      setLoadError(err);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  /** Runs a change; the backend returns the whole configuration as saved. */
  const save = useCallback(async (change: () => Promise<NotificationConfig>) => {
    setSaving(true);
    setError(null);
    try {
      setConfig(await change());
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setSaving(false);
    }
  }, []);

  const updateRule = (payload: UpdateNotificationRuleInput) =>
    void save(() => updateNotificationRule(payload));

  if (loadError && !config) {
    return (
      <ErrorState
        className="m-4"
        error={loadError}
        title={t("rules.loadError")}
        onRetry={() => void load()}
      />
    );
  }
  if (!config) return <LoadingState className="m-4" label={t("rules.loading")} />;

  return (
    <div className="flex h-full flex-col overflow-auto bg-surface-1">
      <div className="border-b border-surface-border px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-semibold text-text-primary">{t("rules.title")}</h2>
          <StatusChip label={t("rules.companyList")} tone="info" title={t("rules.companyHint")} />
        </div>
        <p className="mt-1 text-xs text-text-muted">{t("rules.hint")}</p>
        {!canManage ? <p className="mt-1 text-xs text-text-muted">{t("rules.readOnly")}</p> : null}
        {error ? (
          <p role="alert" className="mt-2 text-sm text-destructive">
            {error}
          </p>
        ) : null}
      </div>

      <section className="border-b border-surface-border px-4 py-3" aria-labelledby="nr-rules">
        <h3 id="nr-rules" className="mb-2 text-sm font-semibold text-text-primary">
          {t("rules.recipientsTitle")}
        </h3>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[56rem] text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="px-2 py-2 font-medium">{t("rules.col.category")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.active")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.recipients")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.ack")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.escalation")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.leadTime")}</th>
              </tr>
            </thead>
            <tbody>
              {config.rules.map((rule) => (
                <RuleRow
                  key={rule.rule_id}
                  rule={rule}
                  config={config}
                  disabled={!canManage || saving}
                  onUpdate={updateRule}
                />
              ))}
            </tbody>
          </table>
        </div>
      </section>

      {config.policies.map((policy) => (
        <section
          key={policy.id}
          className="border-b border-surface-border px-4 py-3"
          aria-labelledby={`nr-policy-${policy.id}`}
        >
          <h3 id={`nr-policy-${policy.id}`} className="text-sm font-semibold text-text-primary">
            {t("rules.escalationTitle", { name: policy.name })}
          </h3>
          <p className="mb-2 text-xs text-text-muted">{t("rules.escalationHint")}</p>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-surface-border text-left text-xs text-text-muted">
                <th className="px-2 py-2 font-medium">{t("rules.col.level")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.delay")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.target")}</th>
                <th className="px-2 py-2 font-medium">{t("rules.col.reference")}</th>
                <th className="px-2 py-2" />
              </tr>
            </thead>
            <tbody>
              {policy.steps.map((step, index) => (
                <StepRow
                  key={step.id}
                  step={step}
                  targets={config.escalation_targets}
                  disabled={!canManage || saving}
                  isLast={index === policy.steps.length - 1}
                  onSave={(input) =>
                    void save(() => updateNotificationEscalationStep(step.id, input))
                  }
                  onRemove={() => void save(() => removeNotificationEscalationStep(step.id))}
                />
              ))}
              {canManage ? (
                <StepRow
                  key={`new-${policy.id}-${policy.steps.length}`}
                  step={null}
                  level={policy.steps.length + 1}
                  targets={config.escalation_targets}
                  disabled={saving}
                  isLast={false}
                  onSave={(input) =>
                    void save(() => addNotificationEscalationStep(policy.id, input))
                  }
                  onRemove={() => undefined}
                />
              ) : null}
            </tbody>
          </table>
        </section>
      ))}

      <RetentionSection
        config={config}
        disabled={!canManage || saving}
        onSave={(days) => void save(() => setNotificationRetention(days))}
      />
    </div>
  );
}

function RuleRow({
  rule,
  config,
  disabled,
  onUpdate,
}: {
  rule: NotificationRuleConfig;
  config: NotificationConfig;
  disabled: boolean;
  onUpdate: (payload: UpdateNotificationRuleInput) => void;
}) {
  const { t } = useTranslation("notifications");
  const [adding, setAdding] = useState<string>("");
  const [lead, setLead] = useState(rule.lead_time_minutes?.toString() ?? "");
  useEffect(() => setLead(rule.lead_time_minutes?.toString() ?? ""), [rule.lead_time_minutes]);

  const available = config.audiences.filter((a) => !rule.audiences.includes(a));
  const setAudiences = (audiences: string[]) => onUpdate({ rule_id: rule.rule_id, audiences });
  const label = categoryLabel(rule.category_code, t);
  const leadValue = minutes(lead);

  return (
    <tr className="border-b border-surface-border align-top">
      <td className="px-2 py-2">
        <div className="font-medium text-text-primary">{label}</div>
        <div className="text-xs text-text-muted">
          {t(`module.${rule.module_code}`)} · {t(`severity.${rule.default_severity}`)}
        </div>
      </td>
      <td className="px-2 py-2">
        <Switch
          checked={rule.is_active}
          disabled={disabled}
          aria-label={`${t("rules.col.active")} ${label}`}
          onCheckedChange={(v) => onUpdate({ rule_id: rule.rule_id, is_active: v })}
        />
      </td>
      <td className="px-2 py-2">
        {rule.fixed_routing ? (
          <p className="text-xs text-text-muted">{t(`rules.fixedRouting.${rule.category_code}`)}</p>
        ) : (
          <div className="flex flex-wrap items-center gap-1.5">
            {rule.audiences.length === 0 ? (
              <StatusChip label={t("rules.noRecipient")} tone="warning" />
            ) : null}
            {rule.audiences.map((a) => (
              <span
                key={a}
                className="inline-flex items-center gap-1 rounded-full border border-surface-border bg-surface-1 px-2 py-0.5 text-xs"
              >
                {audienceLabel(a, t)}
                {!disabled ? (
                  <button
                    type="button"
                    className="rounded-full p-0.5 hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    aria-label={t("rules.removeAudience", { audience: audienceLabel(a, t) })}
                    onClick={() => setAudiences(rule.audiences.filter((x) => x !== a))}
                  >
                    <X className="h-3 w-3" aria-hidden />
                  </button>
                ) : null}
              </span>
            ))}
            {!disabled ? (
              adding === PERMISSION_OPTION ? (
                <Select
                  onValueChange={(p) => {
                    setAdding("");
                    setAudiences([...rule.audiences, `${PERMISSION_PREFIX}${p}`]);
                  }}
                >
                  <SelectTrigger
                    className="h-7 w-48 text-xs"
                    aria-label={t("rules.pickPermission")}
                  >
                    <SelectValue placeholder={t("rules.pickPermission")} />
                  </SelectTrigger>
                  <SelectContent>
                    {PERMISSION_NAMES.filter(
                      (p) => !rule.audiences.includes(`${PERMISSION_PREFIX}${p}`),
                    ).map((p) => (
                      <SelectItem key={p} value={p}>
                        {p}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <Select
                  value=""
                  onValueChange={(v) => {
                    if (v === PERMISSION_OPTION) setAdding(PERMISSION_OPTION);
                    else setAudiences([...rule.audiences, v]);
                  }}
                >
                  <SelectTrigger className="h-7 w-40 text-xs" aria-label={t("rules.addAudience")}>
                    <SelectValue placeholder={t("rules.addAudience")} />
                  </SelectTrigger>
                  <SelectContent>
                    {available.map((a) => (
                      <SelectItem key={a} value={a}>
                        {t(`audience.${a}`)}
                      </SelectItem>
                    ))}
                    <SelectItem value={PERMISSION_OPTION}>
                      {t("audience.permissionPick")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              )
            ) : null}
          </div>
        )}
      </td>
      <td className="px-2 py-2">
        <Switch
          checked={rule.requires_ack}
          disabled={disabled}
          aria-label={`${t("rules.col.ack")} ${label}`}
          onCheckedChange={(v) => onUpdate({ rule_id: rule.rule_id, requires_ack: v })}
        />
      </td>
      <td className="px-2 py-2">
        <Select
          value={rule.escalation_policy_id?.toString() ?? NONE}
          disabled={disabled}
          onValueChange={(v) =>
            onUpdate({ rule_id: rule.rule_id, escalation_policy_id: v === NONE ? null : Number(v) })
          }
        >
          <SelectTrigger
            className="h-8 w-48 text-xs"
            aria-label={`${t("rules.col.escalation")} ${label}`}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NONE}>{t("rules.noEscalation")}</SelectItem>
            {config.policies.map((p) => (
              <SelectItem key={p.id} value={p.id.toString()}>
                {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {rule.escalation_policy_id != null && !rule.requires_ack ? (
          <p className="mt-1 text-xs text-text-muted">{t("rules.escalationNeedsAck")}</p>
        ) : null}
      </td>
      <td className="px-2 py-2">
        {rule.uses_lead_time ? (
          <div className="flex items-center gap-1.5">
            <Input
              type="number"
              inputMode="numeric"
              min={1}
              step={1}
              className="h-8 w-20 text-sm"
              disabled={disabled}
              value={lead}
              aria-label={`${t("rules.col.leadTime")} ${label}`}
              aria-invalid={lead !== "" && leadValue == null}
              onChange={(e) => setLead(e.target.value)}
            />
            <span className="text-xs text-text-muted">{t("rules.minutes")}</span>
            {leadValue != null && leadValue !== rule.lead_time_minutes ? (
              <Button
                size="sm"
                variant="outline"
                className="h-8"
                disabled={disabled}
                onClick={() => onUpdate({ rule_id: rule.rule_id, lead_time_minutes: leadValue })}
              >
                {t("rules.save")}
              </Button>
            ) : null}
          </div>
        ) : (
          <span className="text-xs text-text-muted">—</span>
        )}
      </td>
    </tr>
  );
}

function audienceLabel(code: string, t: TFunction<"notifications">): string {
  const tr = looseT(t);
  if (code.startsWith(PERMISSION_PREFIX)) {
    return tr("audience.permission", { permission: code.slice(PERMISSION_PREFIX.length) });
  }
  return tr(`audience.${code}`);
}

function StepRow({
  step,
  level,
  targets,
  disabled,
  isLast,
  onSave,
  onRemove,
}: {
  step: NotificationEscalationStep | null;
  level?: number;
  targets: string[];
  disabled: boolean;
  isLast: boolean;
  onSave: (input: EscalationStepInput) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation("notifications");
  const [wait, setWait] = useState(step?.wait_minutes.toString() ?? "");
  const [target, setTarget] = useState(step?.target ?? "");
  const [reference, setReference] = useState(step?.reference ?? "");
  useEffect(() => {
    setWait(step?.wait_minutes.toString() ?? "");
    setTarget(step?.target ?? "");
    setReference(step?.reference ?? "");
  }, [step]);

  const waitValue = minutes(wait);
  const needsReference = target === "permission" || target === "role";
  const valid = waitValue != null && target !== "" && (!needsReference || reference.trim() !== "");
  const dirty =
    step == null ||
    waitValue !== step.wait_minutes ||
    target !== step.target ||
    (reference || null) !== (step.reference || null);
  const levelNo = step?.level_no ?? level ?? 0;
  const knownTargets = useMemo(
    () => (target && !targets.includes(target) ? [...targets, target] : targets),
    [target, targets],
  );

  return (
    <tr className="border-b border-surface-border">
      <td className="px-2 py-2 text-text-primary">
        {step ? t("rules.level", { level: levelNo }) : t("rules.newLevel", { level: levelNo })}
      </td>
      <td className="px-2 py-2">
        <div className="flex items-center gap-1.5">
          <Input
            type="number"
            inputMode="numeric"
            min={1}
            step={1}
            className="h-8 w-20 text-sm"
            disabled={disabled}
            value={wait}
            aria-label={t("rules.col.delay")}
            aria-invalid={wait !== "" && waitValue == null}
            onChange={(e) => setWait(e.target.value)}
          />
          <span className="text-xs text-text-muted">{t("rules.minutesAfterAlert")}</span>
        </div>
      </td>
      <td className="px-2 py-2">
        <Select value={target} disabled={disabled} onValueChange={setTarget}>
          <SelectTrigger className="h-8 w-56 text-xs" aria-label={t("rules.col.target")}>
            <SelectValue placeholder={t("rules.pickTarget")} />
          </SelectTrigger>
          <SelectContent>
            {knownTargets.map((x) => (
              <SelectItem key={x} value={x}>
                {t(`escalationTarget.${x}`, { defaultValue: x })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </td>
      <td className="px-2 py-2">
        {target === "permission" ? (
          <Select value={reference} disabled={disabled} onValueChange={setReference}>
            <SelectTrigger className="h-8 w-48 text-xs" aria-label={t("rules.col.reference")}>
              <SelectValue placeholder={t("rules.pickPermission")} />
            </SelectTrigger>
            <SelectContent>
              {PERMISSION_NAMES.map((p) => (
                <SelectItem key={p} value={p}>
                  {p}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : target === "role" ? (
          <Input
            className="h-8 w-48 text-sm"
            disabled={disabled}
            value={reference}
            placeholder={t("rules.roleName")}
            aria-label={t("rules.roleName")}
            onChange={(e) => setReference(e.target.value)}
          />
        ) : (
          <span className="text-xs text-text-muted">—</span>
        )}
      </td>
      <td className="px-2 py-2">
        <div className="flex items-center justify-end gap-1.5">
          {!disabled && dirty ? (
            <Button
              size="sm"
              variant={step ? "outline" : "default"}
              className="h-8"
              disabled={!valid}
              onClick={() =>
                waitValue != null &&
                onSave({
                  wait_minutes: waitValue,
                  target,
                  reference: needsReference ? reference.trim() : null,
                })
              }
            >
              {step ? (
                t("rules.save")
              ) : (
                <>
                  <Plus className="me-1 h-3.5 w-3.5" aria-hidden />
                  {t("rules.addLevel")}
                </>
              )}
            </Button>
          ) : null}
          {step && isLast && !disabled ? (
            <Button
              size="sm"
              variant="ghost"
              className="h-8"
              aria-label={t("rules.removeLevel", { level: levelNo })}
              onClick={onRemove}
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden />
            </Button>
          ) : null}
        </div>
      </td>
    </tr>
  );
}

function RetentionSection({
  config,
  disabled,
  onSave,
}: {
  config: NotificationConfig;
  disabled: boolean;
  onSave: (days: number) => void;
}) {
  const { t } = useTranslation("notifications");
  const r = config.retention;
  const [days, setDays] = useState(r.expiry_days.toString());
  useEffect(() => setDays(r.expiry_days.toString()), [r.expiry_days]);
  const n = Number(days);
  const valid = Number.isInteger(n) && n >= r.min_days && n <= r.max_days;
  return (
    <section className="px-4 py-3" aria-labelledby="nr-retention">
      <h3 id="nr-retention" className="text-sm font-semibold text-text-primary">
        {t("rules.retentionTitle")}
      </h3>
      <p className="mb-2 text-xs text-text-muted">
        {t("rules.retentionHint", { min: r.min_days, max: r.max_days, default: r.default_days })}
      </p>
      <div className="flex items-center gap-2">
        <Input
          type="number"
          inputMode="numeric"
          min={r.min_days}
          max={r.max_days}
          className="h-8 w-24 text-sm"
          disabled={disabled}
          value={days}
          aria-label={t("rules.retentionDays")}
          aria-invalid={!valid}
          onChange={(e) => setDays(e.target.value)}
        />
        <span className="text-sm text-text-muted">{t("rules.days")}</span>
        {r.is_default ? <StatusChip label={t("rules.defaultValue")} tone="neutral" /> : null}
        {!disabled && valid && n !== r.expiry_days ? (
          <Button size="sm" variant="outline" className="h-8" onClick={() => onSave(n)}>
            {t("rules.save")}
          </Button>
        ) : null}
      </div>
    </section>
  );
}
