import type { ColumnDef } from "@tanstack/react-table";
import { useCallback, useEffect, useMemo, useState } from "react";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSession } from "@/hooks/use-session";
import {
  approveInventoryCountLine,
  createInventoryCountSession,
  listInventoryArticles,
  listInventoryCountLines,
  listInventoryCountSessions,
  listInventoryLocations,
  listInventoryReconciliationFindings,
  listInventoryReconciliationRuns,
  listInventoryTransactions,
  listInventoryWarehouses,
  postInventoryCountSession,
  reverseInventoryCountSession,
  runInventoryReconciliation,
  transitionInventoryCountSession,
  upsertInventoryCountLine,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  InventoryArticle,
  InventoryCountLine,
  InventoryCountSession,
  InventoryReconciliationFinding,
  InventoryReconciliationRun,
  InventoryTransaction,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";

export function InventoryControlsPanel() {
  const { info } = useSession();
  const [warehouses, setWarehouses] = useState<Warehouse[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [articles, setArticles] = useState<InventoryArticle[]>([]);
  const [sessions, setSessions] = useState<InventoryCountSession[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [lines, setLines] = useState<InventoryCountLine[]>([]);
  const [runs, setRuns] = useState<InventoryReconciliationRun[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<number | null>(null);
  const [findings, setFindings] = useState<InventoryReconciliationFinding[]>([]);
  const [traceRows, setTraceRows] = useState<InventoryTransaction[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [newWarehouseId, setNewWarehouseId] = useState(0);
  const [newLocationId, setNewLocationId] = useState(0);
  const [newThreshold, setNewThreshold] = useState(5);
  const [lineArticleId, setLineArticleId] = useState(0);
  const [lineLocationId, setLineLocationId] = useState(0);
  const [lineCountQty, setLineCountQty] = useState(0);
  const [lineVarianceReasonCode, setLineVarianceReasonCode] = useState("");
  const [lineReviewerEvidence, setLineReviewerEvidence] = useState("");
  const [sessionReversalReason, setSessionReversalReason] = useState("");

  const selectedSession = useMemo(
    () => sessions.find((session) => session.id === selectedSessionId) ?? null,
    [sessions, selectedSessionId],
  );

  const loadAll = async () => {
    setLoading(true);
    setError(null);
    try {
      const [warehouseRows, locationRows, articleRows, sessionRows, runRows, txRows] = await Promise.all([
        listInventoryWarehouses(),
        listInventoryLocations(null),
        listInventoryArticles({ search: null }),
        listInventoryCountSessions(),
        listInventoryReconciliationRuns(),
        listInventoryTransactions({ source_type: "COUNT_SESSION", limit: 120 }),
      ]);
      setWarehouses(warehouseRows.filter((row) => row.is_active === 1));
      setLocations(locationRows.filter((row) => row.is_active === 1));
      setArticles(articleRows.filter((row) => row.is_active === 1));
      setSessions(sessionRows);
      setRuns(runRows);
      setTraceRows(txRows);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAll();
  }, []);

  useEffect(() => {
    if (!selectedSessionId) {
      setLines([]);
      return;
    }
    void listInventoryCountLines(selectedSessionId)
      .then(setLines)
      .catch((err) => setError(toErrorMessage(err)));
  }, [selectedSessionId]);

  useEffect(() => {
    if (!selectedRunId) {
      setFindings([]);
      return;
    }
    void listInventoryReconciliationFindings(selectedRunId)
      .then(setFindings)
      .catch((err) => setError(toErrorMessage(err)));
  }, [selectedRunId]);

  const withSave = async (work: () => Promise<void>) => {
    setSaving(true);
    setError(null);
    try {
      await work();
      await loadAll();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const refreshCountLines = useCallback(async () => {
    if (!selectedSessionId) {
      setLines([]);
      return;
    }
    try {
      const ls = await listInventoryCountLines(selectedSessionId);
      setLines(ls);
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [selectedSessionId]);

  const lineColumns: ColumnDef<InventoryCountLine>[] = [
    { accessorKey: "article_code", header: "Article" },
    { accessorKey: "location_code", header: "Location" },
    { accessorKey: "system_qty", header: "System qty" },
    { accessorKey: "counted_qty", header: "Counted qty" },
    { accessorKey: "variance_qty", header: "Variance" },
    {
      accessorKey: "is_critical",
      header: "Critical",
      cell: ({ row }) => (row.original.is_critical === 1 ? <Badge variant="destructive">Critical</Badge> : <Badge>Normal</Badge>),
    },
    {
      accessorKey: "approved_by_id",
      header: "Reviewer evidence",
      cell: ({ row }) =>
        row.original.approved_by_id ? (
          <span className="text-xs">Reviewer #{row.original.approved_by_id}</span>
        ) : (
          <span className="text-xs text-text-muted">Pending</span>
        ),
    },
  ];

  const findingColumns: ColumnDef<InventoryReconciliationFinding>[] = [
    { accessorKey: "article_code", header: "Article" },
    { accessorKey: "warehouse_code", header: "Warehouse" },
    { accessorKey: "location_code", header: "Location" },
    { accessorKey: "balance_on_hand", header: "Balance" },
    { accessorKey: "ledger_expected_on_hand", header: "Ledger expected" },
    { accessorKey: "drift_qty", header: "Drift" },
    {
      accessorKey: "is_break",
      header: "Severity",
      cell: ({ row }) => (row.original.is_break === 1 ? <Badge variant="destructive">Break</Badge> : <Badge>Info</Badge>),
    },
  ];

  const traceColumns: ColumnDef<InventoryTransaction>[] = [
    { accessorKey: "performed_at", header: "When" },
    { accessorKey: "movement_type", header: "Movement" },
    { accessorKey: "article_code", header: "Article" },
    { accessorKey: "location_code", header: "Location" },
    { accessorKey: "quantity", header: "Qty" },
    { accessorKey: "source_ref", header: "Source ref" },
    { accessorKey: "reason", header: "Reason" },
  ];

  return (
    <div className="space-y-4">
      {error ? <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm">{error}</div> : null}

      <PermissionGate permission="inv.count">
        <div className="rounded-md border p-4">
          <h3 className="mb-2 text-sm font-semibold">Cycle-count Session Lifecycle</h3>
          <div className="grid gap-2 md:grid-cols-4">
            <div className="space-y-1">
              <Label className="text-xs">Warehouse</Label>
              <Select value={String(newWarehouseId)} onValueChange={(v) => setNewWarehouseId(Number(v))}>
              <SelectTrigger><SelectValue placeholder="Warehouse" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Select warehouse</SelectItem>
                {warehouses.map((w) => (
                  <SelectItem key={w.id} value={String(w.id)}>{w.code} - {w.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Scope location (optional)</Label>
              <Select value={String(newLocationId)} onValueChange={(v) => setNewLocationId(Number(v))}>
              <SelectTrigger><SelectValue placeholder="Scope location (optional)" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">All session locations</SelectItem>
                {locations
                  .filter((l) => newWarehouseId <= 0 || l.warehouse_id === newWarehouseId)
                  .map((l) => (
                    <SelectItem key={l.id} value={String(l.id)}>{l.warehouse_code}/{l.code}</SelectItem>
                  ))}
              </SelectContent>
            </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-cc-threshold" className="text-xs">
                Critical abs threshold
              </Label>
              <Input
                id="inv-cc-threshold"
                type="number"
                min={0}
                step="0.1"
                value={newThreshold}
                onChange={(e) => setNewThreshold(Number(e.target.value || 0))}
              />
            </div>
            <div className="flex items-end">
            <Button
              disabled={saving || newWarehouseId <= 0}
              onClick={() =>
                void withSave(async () => {
                  const session = await createInventoryCountSession({
                    warehouse_id: newWarehouseId,
                    location_id: newLocationId > 0 ? newLocationId : null,
                    critical_abs_threshold: newThreshold,
                    actor_id: info?.user_id ?? null,
                  });
                  setSelectedSessionId(session.id);
                })
              }
            >
              Create session
            </Button>
            </div>
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            <Select value={String(selectedSessionId ?? 0)} onValueChange={(v) => setSelectedSessionId(Number(v) || null)}>
              <SelectTrigger className="w-72"><SelectValue placeholder="Select session" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Select session</SelectItem>
                {sessions.map((session) => (
                  <SelectItem key={session.id} value={String(session.id)}>{session.session_code} ({session.status})</SelectItem>
                ))}
              </SelectContent>
            </Select>
            {selectedSession ? (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || selectedSession.status !== "draft"}
                  onClick={() => void withSave(async () => {
                    await transitionInventoryCountSession({
                      session_id: selectedSession.id,
                      expected_row_version: selectedSession.row_version,
                      next_status: "counting",
                      actor_id: info?.user_id ?? null,
                    });
                  })}
                >
                  Start counting
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || selectedSession.status !== "counting"}
                  onClick={() => void withSave(async () => {
                    await transitionInventoryCountSession({
                      session_id: selectedSession.id,
                      expected_row_version: selectedSession.row_version,
                      next_status: "submitted",
                      actor_id: info?.user_id ?? null,
                    });
                  })}
                >
                  Submit
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || selectedSession.status !== "submitted"}
                  onClick={() => void withSave(async () => {
                    await transitionInventoryCountSession({
                      session_id: selectedSession.id,
                      expected_row_version: selectedSession.row_version,
                      next_status: "approved",
                      actor_id: info?.user_id ?? null,
                    });
                  })}
                >
                  Approve session
                </Button>
                <Button
                  size="sm"
                  disabled={saving || selectedSession.status !== "approved"}
                  onClick={() => void withSave(async () => {
                    await postInventoryCountSession({
                      session_id: selectedSession.id,
                      expected_row_version: selectedSession.row_version,
                      actor_id: info?.user_id ?? null,
                    });
                  })}
                >
                  Post variances
                </Button>
              </>
            ) : null}
          </div>
        </div>
      </PermissionGate>

      {selectedSession ? (
        <div className="rounded-md border p-4">
          <h3 className="mb-2 text-sm font-semibold">Count Lines and Reviewer Evidence</h3>
          <div className="grid gap-2 md:grid-cols-5">
            <div className="space-y-1">
              <Label className="text-xs">Article</Label>
              <Select value={String(lineArticleId)} onValueChange={(v) => setLineArticleId(Number(v))}>
              <SelectTrigger><SelectValue placeholder="Article" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Select article</SelectItem>
                {articles.map((a) => (
                  <SelectItem key={a.id} value={String(a.id)}>{a.article_code} - {a.article_name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Location</Label>
              <Select value={String(lineLocationId)} onValueChange={(v) => setLineLocationId(Number(v))}>
              <SelectTrigger><SelectValue placeholder="Location" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Select location</SelectItem>
                {locations
                  .filter((l) => l.warehouse_id === selectedSession.warehouse_id)
                  .map((l) => (
                    <SelectItem key={l.id} value={String(l.id)}>{l.warehouse_code}/{l.code}</SelectItem>
                  ))}
              </SelectContent>
            </Select>
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Counted quantity</Label>
              <Input type="number" min={0} step="0.01" value={lineCountQty} onChange={(e) => setLineCountQty(Number(e.target.value || 0))} />
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Variance reason code</Label>
              <Input value={lineVarianceReasonCode} onChange={(e) => setLineVarianceReasonCode(e.target.value)} placeholder="If variance vs system" />
            </div>
            <div className="flex items-end">
            <Button
              disabled={saving || lineArticleId <= 0 || lineLocationId <= 0}
              onClick={() => {
                void (async () => {
                  setSaving(true);
                  setError(null);
                  try {
                    await upsertInventoryCountLine({
                      session_id: selectedSession.id,
                      article_id: lineArticleId,
                      location_id: lineLocationId,
                      counted_qty: lineCountQty,
                      variance_reason_code: lineVarianceReasonCode.trim() || null,
                    });
                    await refreshCountLines();
                  } catch (err) {
                    setError(toErrorMessage(err));
                  } finally {
                    setSaving(false);
                  }
                })();
              }}
            >
              Upsert line
            </Button>
            </div>
          </div>
          <div className="mt-2 flex gap-2">
            <div className="min-w-0 flex-1 space-y-1">
              <Label className="text-xs">Reviewer evidence note</Label>
              <Input value={lineReviewerEvidence} onChange={(e) => setLineReviewerEvidence(e.target.value)} />
            </div>
            <Button
              variant="outline"
              disabled={saving || !lineReviewerEvidence.trim() || lines.every((line) => line.approval_required === 0 || line.approved_by_id !== null)}
              onClick={() => void withSave(async () => {
                const pending = lines.find((line) => line.approval_required === 1 && line.approved_by_id === null);
                if (!pending || !info?.user_id) return;
                await approveInventoryCountLine({
                  line_id: pending.id,
                  expected_row_version: pending.row_version,
                  reviewer_id: info.user_id,
                  reviewer_evidence: lineReviewerEvidence,
                });
              })}
            >
              Approve next pending variance
            </Button>
          </div>
          <div className="mt-3">
            <DataTable columns={lineColumns} data={lines} searchable={false} isLoading={loading} />
          </div>
          {selectedSession.status === "posted" ? (
            <div className="mt-3 flex gap-2">
              <Input value={sessionReversalReason} onChange={(e) => setSessionReversalReason(e.target.value)} placeholder="Reversal reason (mandatory)" />
              <Button
                variant="destructive"
                disabled={saving || sessionReversalReason.trim().length < 5}
                onClick={() => void withSave(async () => {
                  await reverseInventoryCountSession({
                    session_id: selectedSession.id,
                    expected_row_version: selectedSession.row_version,
                    reason: sessionReversalReason,
                    actor_id: info?.user_id ?? null,
                  });
                })}
              >
                Reverse posting
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="rounded-md border p-4">
        <h3 className="mb-2 text-sm font-semibold">Daily Integrity and Reconciliation</h3>
        <PermissionGate permission="erp.reconcile">
          <Button
            size="sm"
            disabled={saving}
            onClick={() => void withSave(async () => {
              await runInventoryReconciliation({ actor_id: info?.user_id ?? null, drift_break_threshold: 0.01 });
            })}
          >
            Run daily reconciliation
          </Button>
        </PermissionGate>
        <div className="mt-2 flex gap-2">
          <Select value={String(selectedRunId ?? 0)} onValueChange={(v) => setSelectedRunId(Number(v) || null)}>
            <SelectTrigger className="w-72"><SelectValue placeholder="Select reconciliation run" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="0">Select run</SelectItem>
              {runs.map((run) => (
                <SelectItem key={run.id} value={String(run.id)}>{run.run_code} ({run.drift_rows} drift rows)</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="mt-3">
          <DataTable columns={findingColumns} data={findings} searchable={false} isLoading={loading} />
        </div>
      </div>

      <div className="rounded-md border p-4">
        <h3 className="mb-2 text-sm font-semibold">Forensic Trace (Immutable Mutation Feed)</h3>
        <DataTable columns={traceColumns} data={traceRows} searchable={false} isLoading={loading} />
      </div>
    </div>
  );
}
