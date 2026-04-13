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
import { toErrorMessage } from "@/utils/errors";

interface ChangeHistoryEntry {
  policyId: number;
  changedAt: string;
  note: string;
}

export function RetentionPolicyPanel() {
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canEdit = can("adm.settings");

  const [rows, setRows] = useState<RetentionPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savingId, setSavingId] = useState<number | null>(null);
  const [selectedPolicyId, setSelectedPolicyId] = useState<number | null>(null);
  const [history, setHistory] = useState<ChangeHistoryEntry[]>([]);
  const [loadedAt, setLoadedAt] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listRetentionPolicies();
      setRows(data);
      setLoadedAt(new Date().toISOString());
      if (!selectedPolicyId) {
        const first = data.at(0);
        if (first) {
          setSelectedPolicyId(first.id);
        }
      }
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [selectedPolicyId]);

  useEffect(() => {
    void load();
  }, [load]);

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

        const changedKeys = Object.keys(changes);
        setHistory((prev) => [
          {
            policyId: policy.id,
            changedAt: new Date().toISOString(),
            note: `Updated ${changedKeys.join(", ")}`,
          },
          ...prev,
        ]);
      } catch (err) {
        setError(toErrorMessage(err));
        await load();
      } finally {
        setSavingId(null);
      }
    },
    [canEdit, load],
  );

  const selectedHistory = useMemo(
    () => history.filter((entry) => entry.policyId === selectedPolicyId),
    [history, selectedPolicyId],
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
        <div className="flex items-center gap-2">
          <History className="h-4 w-4" />
          <h4 className="text-sm font-semibold">Change history</h4>
        </div>

        {selectedPolicyId ? (
          <>
            <p className="text-xs text-muted-foreground">Policy ID: {selectedPolicyId}</p>
            {loadedAt && (
              <p className="text-xs text-muted-foreground">
                Snapshot loaded at {new Date(loadedAt).toLocaleString()}
              </p>
            )}
            {selectedHistory.length > 0 ? (
              <div className="space-y-2">
                {selectedHistory.map((entry, idx) => (
                  <div key={`${entry.policyId}-${entry.changedAt}-${idx}`} className="rounded border p-2">
                    <p className="text-xs text-muted-foreground">
                      {new Date(entry.changedAt).toLocaleString()}
                    </p>
                    <p className="text-sm">{entry.note}</p>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No edits in this session yet. Persistent audit timeline will be linked in SP07-F03.
              </p>
            )}
          </>
        ) : (
          <p className="text-sm text-muted-foreground">Select a row to inspect change history.</p>
        )}

        <Button size="sm" variant="outline" onClick={() => void load()}>
          Refresh
        </Button>
      </div>
    </div>
  );
}
