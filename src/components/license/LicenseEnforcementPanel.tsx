import { AlertCircle, ShieldAlert, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  applyAdminLicenseAction,
  applyLicensingCompromiseResponse,
  getLicenseEnforcementStatus,
  listLicenseTraceEvents,
} from "@/services/license-service";
import type { ApplyAdminLicenseActionInput, LicenseStatusView, LicenseTraceEvent } from "@shared/ipc-types";

const ACTIONS: ReadonlyArray<ApplyAdminLicenseActionInput["action"]> = ["suspend", "revoke", "reactivate"];

function stateTone(state: string): "default" | "secondary" | "destructive" | "outline" {
  if (state === "active" || state === "trusted") return "secondary";
  if (state === "revoked" || state === "suspended") return "destructive";
  return "outline";
}

export function LicenseEnforcementPanel() {
  const [status, setStatus] = useState<LicenseStatusView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState<ApplyAdminLicenseActionInput["action"] | null>(null);
  const [traceEvents, setTraceEvents] = useState<LicenseTraceEvent[]>([]);
  const [compromiseLoading, setCompromiseLoading] = useState(false);

  const reload = async () => {
    setLoading(true);
    try {
      const next = await getLicenseEnforcementStatus();
      const traces = await listLicenseTraceEvents(6, null);
      setStatus(next);
      setTraceEvents(traces);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load license status.");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const trustIcon = useMemo(() => {
    if (!status) return null;
    if (status.trust_state === "trusted") return <ShieldCheck className="h-4 w-4 text-status-success" />;
    return <ShieldAlert className="h-4 w-4 text-status-warning" />;
  }, [status]);

  const runAction = async (action: ApplyAdminLicenseActionInput["action"]) => {
    setActionLoading(action);
    try {
      await applyAdminLicenseAction({
        action,
        reason: `Operator action from settings panel: ${action}`,
        expected_entitlement_state: status?.entitlement_state ?? null,
        expected_activation_state: status?.activation_state ?? null,
      });
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to apply '${action}'.`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleCompromiseResponse = async () => {
    setCompromiseLoading(true);
    try {
      await applyLicensingCompromiseResponse({
        issuer: "maintafox-vps",
        key_id: "key-v1",
        reason: "operator initiated compromise drill",
        force_revocation: true,
      });
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to apply compromise response.");
    } finally {
      setCompromiseLoading(false);
    }
  };

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-lg">License Enforcement</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {loading && !status && <p className="text-sm text-text-muted">Loading license state...</p>}
        {status && (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={stateTone(status.entitlement_state)}>Entitlement: {status.entitlement_state}</Badge>
              <Badge variant={stateTone(status.activation_state)}>Activation: {status.activation_state}</Badge>
              <Badge variant={stateTone(status.trust_state)}>
                <span className="mr-1 inline-flex">{trustIcon}</span>
                Trust: {status.trust_state}
              </Badge>
              {status.policy_sync_pending && <Badge variant="outline">Policy Sync Pending</Badge>}
            </div>

            <div className="rounded-md border border-border bg-surface-2 px-3 py-2 text-sm text-text-secondary">
              {status.actionable_message}
            </div>

            <div className="grid gap-1 text-sm text-text-muted">
              <span>Pending local writes: {status.pending_local_writes}</span>
              {status.last_admin_action && (
                <span>
                  Last admin action: {status.last_admin_action} at{" "}
                  {status.last_admin_action_at ? new Date(status.last_admin_action_at).toLocaleString() : "unknown"}
                </span>
              )}
            </div>

            <div className="flex flex-wrap gap-2">
              {ACTIONS.map((action) => (
                <Button
                  key={action}
                  size="sm"
                  variant={action === "reactivate" ? "default" : "outline"}
                  disabled={actionLoading !== null}
                  onClick={() => void runAction(action)}
                >
                  {actionLoading === action ? "Applying..." : action}
                </Button>
              ))}
              <Button
                size="sm"
                variant="destructive"
                disabled={compromiseLoading || actionLoading !== null}
                onClick={() => void handleCompromiseResponse()}
              >
                {compromiseLoading ? "Applying..." : "Compromise response"}
              </Button>
            </div>

            <div className="text-xs text-text-muted">
              <div className="mb-1 inline-flex items-center gap-1">
                <AlertCircle className="h-3.5 w-3.5" />
                Recovery Paths
              </div>
              <div className="flex flex-wrap gap-1">
                {status.recovery_paths.map((path) => (
                  <Badge key={path} variant="outline">
                    {path}
                  </Badge>
                ))}
              </div>
            </div>

            {traceEvents.length > 0 && (
              <div className="rounded-md border border-border bg-surface-2 px-3 py-2">
                <div className="mb-2 text-xs font-semibold text-text-secondary">Recent immutable traces</div>
                <div className="space-y-1 text-xs text-text-muted">
                  {traceEvents.map((trace) => (
                    <div key={trace.id} className="flex flex-wrap items-center gap-x-2 gap-y-1">
                      <span className="font-medium text-text-secondary">{trace.event_type}</span>
                      <span>{trace.outcome}</span>
                      <span>{new Date(trace.occurred_at).toLocaleString()}</span>
                      <Badge variant="outline">{trace.correlation_id}</Badge>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
