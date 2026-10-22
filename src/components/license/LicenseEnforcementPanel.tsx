/**
 * Settings → Integrations: licence enforcement state and the administrator's licence actions.
 *
 * Suspend, revoke, reactivate and the signing-key compromise response change what the whole site
 * may do: each one opens a confirmation that names the effect and asks for a reason (kept in the
 * licence audit), and runs through step-up re-authentication (the backend requires it). The
 * compromise response targets the issuer and key that signed the active envelope, as reported by
 * the backend — never a name written here.
 */
import { ShieldAlert, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Label } from "@/components/ui/label";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { editionLabel } from "@/lib/edition-label";
import {
  applyAdminLicenseAction,
  applyLicensingCompromiseResponse,
  getLicenseEnforcementStatus,
} from "@/services/license-service";
import type { LicenseStatusView } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type LicenseAction = "suspend" | "revoke" | "reactivate" | "compromise";

/** Shortest reason accepted for an action that changes what the whole site may do. */
export const LICENSE_REASON_MIN_LENGTH = 10;

const GOOD_STATES = new Set(["active", "trusted", "verified", "soft_accepted"]);
const BAD_STATES = new Set(["revoked", "suspended", "invalid_signature", "untrusted"]);

/** Tone of a licence, activation or trust state (status is never shown by colour alone). */
export function licenseStateTone(state: string | null | undefined): Tone {
  if (!state) return "neutral";
  if (GOOD_STATES.has(state)) return "success";
  if (BAD_STATES.has(state) || state.startsWith("denied_")) return "danger";
  if (state.startsWith("pending") || state.startsWith("degraded")) return "warning";
  return "neutral";
}

/**
 * The one piece of advice for the current state, in the same order of precedence as the backend's
 * enforcement (policy sync, licence, product activation, machine activation, device trust).
 */
export type LicenseAdviceKey =
  | "advice.policySync"
  | "advice.revoked"
  | "advice.suspended"
  | "advice.activationDenied"
  | "advice.activationDegraded"
  | "advice.activationPending"
  | "advice.machinePendingRevocation"
  | "advice.machineRevoked"
  | "advice.untrusted"
  | "advice.healthy"
  | "advice.activationIncomplete";

export function licenseAdviceKey(s: LicenseStatusView): LicenseAdviceKey {
  if (s.policy_sync_pending) return "advice.policySync";
  if (s.entitlement_state === "revoked") return "advice.revoked";
  if (s.entitlement_state === "suspended") return "advice.suspended";
  if (s.activation_state.startsWith("denied_")) return "advice.activationDenied";
  if (s.activation_state === "degraded_api_unavailable") return "advice.activationDegraded";
  if (s.activation_state === "pending_online_validation") return "advice.activationPending";
  if (s.machine_activation_state === "pending_revocation") return "advice.machinePendingRevocation";
  if (s.machine_activation_state === "revoked") return "advice.machineRevoked";
  if (s.trust_state !== "trusted") return "advice.untrusted";
  if (s.activation_state === "active") return "advice.healthy";
  return "advice.activationIncomplete";
}

export function LicenseEnforcementPanel() {
  const { t, i18n } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [status, setStatus] = useState<LicenseStatusView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<LicenseAction | null>(null);
  const [reason, setReason] = useState("");

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await getLicenseEnforcementStatus());
      setError(null);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const stateLabel = (kind: "entitlement" | "activation" | "trust", code: string) => {
    const key = `license.states.${code}`;
    return i18n.exists(`settings:${key}`) ? t(key) : t("license.states.other", { kind });
  };

  const openAction = (action: LicenseAction) => {
    setReason("");
    setPending(action);
  };

  /** Runs inside the confirmation: a rejection keeps it open, the global toast says why. */
  const confirmAction = async () => {
    if (!pending || !status) return;
    const why = reason.trim();
    if (pending === "compromise") {
      const issuer = status.envelope_issuer;
      const keyId = status.envelope_key_id;
      if (!issuer || !keyId) return;
      await withStepUp(() =>
        applyLicensingCompromiseResponse({
          issuer,
          key_id: keyId,
          reason: why,
          force_revocation: true,
        }),
      );
    } else {
      await withStepUp(() =>
        applyAdminLicenseAction({
          action: pending,
          reason: why,
          expected_entitlement_state: status.entitlement_state,
          // Admin concurrency checks machine contract state, not product activation.
          expected_activation_state: status.machine_activation_state,
        }),
      );
    }
    await reload();
  };

  const canManage = can(P.LIC_MANAGE);
  const hasEnvelopeSigner = Boolean(status?.envelope_issuer && status?.envelope_key_id);
  const reasonOk = reason.trim().length >= LICENSE_REASON_MIN_LENGTH;

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-lg">{t("license.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {error != null && status == null ? (
          <ErrorState
            error={error}
            title={t("license.loadError")}
            onRetry={() => void reload()}
            retrying={loading}
          />
        ) : status == null ? (
          <LoadingState />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <StatusChip
                tone={licenseStateTone(status.entitlement_state)}
                label={`${t("license.entitlement")} : ${stateLabel("entitlement", status.entitlement_state)}`}
              />
              <StatusChip
                tone={licenseStateTone(status.activation_state)}
                label={`${t("license.activation")} : ${stateLabel("activation", status.activation_state)}`}
              />
              <StatusChip
                tone={licenseStateTone(status.trust_state)}
                icon={status.trust_state === "trusted" ? ShieldCheck : ShieldAlert}
                label={`${t("license.trust")} : ${stateLabel("trust", status.trust_state)}`}
              />
              {status.policy_sync_pending ? (
                <StatusChip tone="warning" label={t("license.policySyncPending")} />
              ) : null}
              {status.license_edition ? (
                <StatusChip
                  tone="neutral"
                  label={t("license.edition", {
                    edition: editionLabel(status.license_edition) ?? status.license_edition,
                  })}
                />
              ) : null}
            </div>

            <p className="rounded-md border border-surface-border bg-surface-2 px-3 py-2 text-sm text-text-secondary">
              {t(`license.${licenseAdviceKey(status)}`)}
            </p>

            <dl className="grid gap-1 text-sm text-text-muted sm:grid-cols-2">
              <div>
                <dt className="inline">{t("license.pendingWrites")} </dt>
                <dd className="inline font-medium text-text-primary">
                  {status.pending_local_writes}
                </dd>
              </div>
              {status.last_admin_action ? (
                <div>
                  <dt className="inline">{t("license.lastAction")} </dt>
                  <dd className="inline font-medium text-text-primary">
                    {t(`license.actions.${status.last_admin_action}.label`, {
                      defaultValue: t("license.actions.other"),
                    })}
                    {status.last_admin_action_at
                      ? ` · ${formatDateTime(status.last_admin_action_at)}`
                      : ""}
                  </dd>
                </div>
              ) : null}
            </dl>

            {canManage ? (
              <div className="flex flex-wrap gap-2">
                {(["suspend", "revoke", "reactivate"] as const).map((action) => (
                  <Button
                    key={action}
                    variant={action === "reactivate" ? "default" : "outline"}
                    className="h-11"
                    onClick={() => openAction(action)}
                  >
                    {t(`license.actions.${action}.label`)}
                  </Button>
                ))}
                <Button
                  variant="destructive"
                  className="h-11"
                  disabled={!hasEnvelopeSigner}
                  title={hasEnvelopeSigner ? undefined : t("license.actions.compromise.noEnvelope")}
                  onClick={() => openAction("compromise")}
                >
                  {t("license.actions.compromise.label")}
                </Button>
                {!hasEnvelopeSigner ? (
                  <p className="basis-full text-xs text-text-muted">
                    {t("license.actions.compromise.noEnvelope")}
                  </p>
                ) : null}
              </div>
            ) : null}

            {status.recent_traces.length > 0 ? (
              <details className="text-xs text-text-muted">
                <summary className="cursor-pointer select-none">
                  {t("license.traces", { count: status.recent_traces.length })}
                </summary>
                <ul className="mt-2 space-y-1">
                  {status.recent_traces.map((trace) => (
                    <li key={trace.id} className="flex flex-wrap gap-x-2">
                      <span className="text-text-secondary">
                        {formatDateTime(trace.occurred_at)}
                      </span>
                      <span>{trace.event_type}</span>
                      <span>{trace.outcome}</span>
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </>
        )}
      </CardContent>

      <ConfirmDialog
        open={pending != null}
        onOpenChange={(open) => !open && setPending(null)}
        {...(pending ? { title: t(`license.actions.${pending}.confirmTitle`) } : {})}
        description={pending ? t(`license.actions.${pending}.confirmBody`) : ""}
        confirmLabel={pending ? t(`license.actions.${pending}.confirm`) : ""}
        destructive={pending !== "reactivate"}
        confirmDisabled={!reasonOk}
        onConfirm={confirmAction}
      >
        <div className="grid gap-1.5">
          <Label htmlFor="license-action-reason">{t("license.reason")}</Label>
          <Textarea
            id="license-action-reason"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            aria-describedby="license-action-reason-hint"
          />
          <p id="license-action-reason-hint" className="text-xs text-text-muted">
            {t("license.reasonHint", { min: LICENSE_REASON_MIN_LENGTH })}
          </p>
        </div>
      </ConfirmDialog>
      {StepUpDialogElement}
    </Card>
  );
}
