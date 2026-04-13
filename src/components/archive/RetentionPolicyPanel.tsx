import { History, Lock } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type RetentionPolicy,
  listRetentionPolicies,
  updateRetentionPolicy,
} from "@/services/archive-service";
import { type ActivityEventSummary, listActivityEvents } from "@/services/activity-service";
import { toErrorMessage } from "@/utils/errors";

export function RetentionPolicyPanel() {
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canEdit = can("adm.settings");

  const [rows, setRows] = useState<RetentionPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savingId, setSavingId] = useState<number | null>(null);
  const [selectedPolicyId, setSelectedPolicyId] = useState<number | null>(null);
  const [persistedHistory, setPersistedHistory] = useState<ActivityEventSummary[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listRetentionPolicies();
      setRows(data);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const loadHistory = useCallback(async (policyId: number) => {
    setHistoryLoading(true);
    try {
      const events = await listActivityEvents({
        source_module: "archive",
        source_record_type: "retention_policy",
        source_record_id: String(policyId),
        limit: 50,
        offset: 0,
      });
      setPersistedHistory(events);
    } catch {
      // Non-critical: silently ignore if activity_events not yet available
      setPersistedHistory([]);
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (selectedPolicyId === null && rows.length > 0) {
      setSelectedPolicyId(rows[0]!.id);
    }
  }, [rows, selectedPolicyId]);

  useEffect(() => {
    if (selectedPolicyId !== null) {
      void loadHistory(selectedPolicyId);
    }
  }, [selectedPolicyId, loadHistory]);

  const patchPolicy = useCallback(
    async (policy: RetentionPolicy, changes: Partial<RetentionPolicy>) => {
      if (!canEdit) return;
      const next = { ...policy, ...changes };
      if (next.purge_mode === "never" && next.allow_purge) {
        setError("Cannot enable allow_purge when purge_mode is 'never'.");
        return;
      }

      setError(null);
      setRows((prev) => prev.map((item) => (item.id === policy.id ? next : item)));
      setSavingId(policy.id);

      try {
        const payload: {
          policy_id: number;
          retention_years?: number;
          purge_mode?: string;
          allow_restore?: boolean;
          allow_purge?: boolean;
          requires_legal_hold_check?: boolean;
        } = { policy_id: policy.id };

        if (changes.retention_years !== undefined) payload.retention_years = changes.retention_years;
        if (changes.purge_mode !== undefined) payload.purge_mode = changes.purge_mode;
        if (changes.allow_restore !== undefined) payload.allow_restore = changes.allow_restore;
        if (changes.allow_purge !== undefined) payload.allow_purge = changes.allow_purge;
        if (changes.requires_legal_hold_check !== undefined) {
          payload.requires_legal_hold_check = changes.requires_legal_hold_check;
        }

        await updateRetentionPolicy(payload);

        // Reload persisted history from activity_events after successful update
        if (policy.id === selectedPolicyId) {
          void loadHistory(policy.id);
        }
      } catch (err) {
        setError(toErrorMessage(err));
        await load();
      } finally {
        setSavingId(null);
      }
    },
    [canEdit, load, loadHistory, selectedPolicyId],
  );

  if (loading) {
    return <div className="text-sm text-muted-foreground">Loading retention policies...</div>;
  }

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_300px]">
      <div className="space-y-3 rounded-lg border p-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">Retention Policies</h3>
          <Badge variant="outline">{rows.length} policies</Badge>
        </div>
        {error && <div className="text-sm text-destructive">{error}</div>}
        {permissionsLoading ? null : !canEdit ? (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Lock className="h-3.5 w-3.5" />
            Read-only: requires adm.settings permission to edit.
          </div>
        ) : null}

        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Module</TableHead>
              <TableHead>Class</TableHead>
              <TableHead>Retention (years)</TableHead>
              <TableHead>Purge mode</TableHead>
              <TableHead>Allow restore</TableHead>
              <TableHead>Allow purge</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const disableAllowPurge = row.purge_mode === "never";
              return (
                <TableRow
                  key={row.id}
                  className={selectedPolicyId === row.id ? "bg-muted/40" : undefined}
                  onClick={() => setSelectedPolicyId(row.id)}
                >
                  <TableCell>{row.module_code}</TableCell>
                  <TableCell>{row.archive_class}</TableCell>
                  <TableCell>
                    <Input
                      type="number"
                      min={0}
                      className="h-8 w-24"
                      disabled={!canEdit || savingId === row.id}
                      value={row.retention_years}
                      onChange={(e) => {
                        const nextValue = Number.parseInt(e.target.value, 10);
                        if (Number.isNaN(nextValue)) return;
                        void patchPolicy(row, { retention_years: nextValue });
                      }}
                    />
                  </TableCell>
                  <TableCell>
                    <Select
                      value={row.purge_mode}
                      disabled={!canEdit || savingId === row.id}
                      onValueChange={(value) =>
                        void patchPolicy(row, {
                          purge_mode: value,
                          allow_purge: value === "never" ? false : row.allow_purge,
                        })
                      }
                    >
                      <SelectTrigger className="h-8 w-40">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="manual_approval">manual_approval</SelectItem>
                        <SelectItem value="scheduled">scheduled</SelectItem>
                        <SelectItem value="never">never</SelectItem>
                      </SelectContent>
                    </Select>
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={row.allow_restore}
                      disabled={!canEdit || savingId === row.id}
                      onCheckedChange={(checked) => void patchPolicy(row, { allow_restore: checked })}
                    />
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={row.allow_purge}
                      disabled={!canEdit || savingId === row.id || disableAllowPurge}
                      onCheckedChange={(checked) => void patchPolicy(row, { allow_purge: checked })}
                    />
                    {disableAllowPurge && (
                      <p className="mt-1 text-[10px] text-muted-foreground">
                        Disabled by purge_mode='never'
                      </p>
                    )}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>

      <div className="space-y-3 rounded-lg border p-4">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <History className="h-4 w-4" />
            <h4 className="text-sm font-semibold">Change history</h4>
          </div>
          {selectedPolicyId && (
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-xs"
              onClick={() => void loadHistory(selectedPolicyId)}
            >
              Refresh
            </Button>
          )}
        </div>

        {selectedPolicyId ? (
          <>
            {historyLoading ? (
              <p className="text-xs text-muted-foreground">Loading history…</p>
            ) : persistedHistory.length > 0 ? (
              <div className="max-h-[400px] space-y-2 overflow-y-auto">
                {persistedHistory.map((event) => {
                  const summary =
                    event.summary_json && typeof event.summary_json === "object"
                      ? (event.summary_json as Record<string, unknown>)
                      : null;
                  return (
                    <div key={event.id} className="rounded border p-2">
                      <p className="text-xs text-muted-foreground">
                        {new Date(event.happened_at).toLocaleString()}
                      </p>
                      <p className="mt-0.5 text-sm font-medium">{event.event_code}</p>
                      {summary?.["result"] != null && (
                        <p className="text-xs text-muted-foreground">
                          Result: {`${summary["result"]}`}
                        </p>
                      )}
                      {event.actor_username && (
                        <p className="text-xs text-muted-foreground">By: {event.actor_username}</p>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No persisted events found for this policy.
              </p>
            )}
          </>
        ) : (
          <p className="text-sm text-muted-foreground">Select a row to inspect change history.</p>
        )}
      </div>
    </div>
  );
}
