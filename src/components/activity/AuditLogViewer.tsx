import { Download, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type AuditEventDetail,
  type AuditFilter,
  type AuditEventSummary,
  exportAuditLog,
  getAuditEvent,
  listAuditEvents,
} from "@/services/activity-service";
import { getSessionInfo } from "@/services/auth-service";
import { toErrorMessage } from "@/utils/errors";

const PAGE_SIZE = 40;

interface AuditLogViewerProps {
  className?: string;
}

export function AuditLogViewer({ className }: AuditLogViewerProps) {
  const { can } = usePermissions();

  const [items, setItems] = useState<AuditEventSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [detailsMap, setDetailsMap] = useState<Record<number, AuditEventDetail>>({});
  const [currentUserId, setCurrentUserId] = useState<number | null>(null);

  const [actionCode, setActionCode] = useState("");
  const [result, setResult] = useState("");
  const [actorId, setActorId] = useState("");
  const [retentionClass, setRetentionClass] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [offset, setOffset] = useState(0);

  const [exportReason, setExportReason] = useState("");
  const [mySecurityOnly, setMySecurityOnly] = useState(false);

  const filter = useMemo<AuditFilter>(() => {
    const out: AuditFilter = { limit: PAGE_SIZE, offset };
    if (actionCode.trim()) out.action_code = actionCode.trim();
    if (result.trim()) out.result = result.trim();
    if (retentionClass.trim()) out.retention_class = retentionClass.trim();
    if (dateFrom) out.date_from = `${dateFrom}T00:00:00Z`;
    if (dateTo) out.date_to = `${dateTo}T23:59:59Z`;
    const forcedActor = mySecurityOnly ? currentUserId : null;
    if (typeof forcedActor === "number") {
      out.actor_id = forcedActor;
    } else if (actorId.trim()) {
      const parsed = Number(actorId);
      if (!Number.isNaN(parsed)) out.actor_id = parsed;
    }
    return out;
  }, [actionCode, actorId, currentUserId, dateFrom, dateTo, mySecurityOnly, offset, result, retentionClass]);

  const visibleItems = useMemo(() => {
    if (!mySecurityOnly) return items;
    return items.filter((row) => row.auth_context === "step_up" || row.auth_context === "password");
  }, [items, mySecurityOnly]);

  const loadData = async () => {
    try {
      setLoading(true);
      setError(null);
      setItems(await listAuditEvents(filter));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void getSessionInfo().then((session) => {
      setCurrentUserId(typeof session.user_id === "number" ? session.user_id : null);
    });
  }, []);

  return (
    <Card className={className}>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Audit Log</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
          <Input
            placeholder="action_code prefix"
            value={actionCode}
            onChange={(e) => setActionCode(e.target.value)}
          />
          <Input placeholder="result" value={result} onChange={(e) => setResult(e.target.value)} />
          <Input placeholder="actor_id" value={actorId} onChange={(e) => setActorId(e.target.value)} />
          <Input
            placeholder="retention_class"
            value={retentionClass}
            onChange={(e) => setRetentionClass(e.target.value)}
          />
          <Input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} />
          <Input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => void loadData()}>
            Apply filters
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setActionCode("");
              setResult("");
              setActorId("");
              setRetentionClass("");
              setDateFrom("");
              setDateTo("");
              setOffset(0);
            }}
          >
            Reset
          </Button>
          <Button
            size="sm"
            variant={mySecurityOnly ? "default" : "outline"}
            onClick={() => setMySecurityOnly((prev) => !prev)}
          >
            <ShieldCheck className="mr-1 h-3 w-3" />
            My Security Events
          </Button>
        </div>

        {can("log.export") && (
          <div className="rounded-md border p-2">
            <div className="mb-2 text-xs text-muted-foreground">
              Export writes a meta-audit row (`export.audit_log`) and returns JSON payload.
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Textarea
                className="min-h-[64px] flex-1"
                placeholder="Export reason (required)"
                value={exportReason}
                onChange={(e) => setExportReason(e.target.value)}
              />
              <Button
                disabled={!exportReason.trim()}
                onClick={() =>
                  void (async () => {
                    const result = await exportAuditLog({
                      filter,
                      export_reason: exportReason.trim(),
                    });
                    const blob = new Blob([JSON.stringify(result.rows_json, null, 2)], {
                      type: "application/json;charset=utf-8",
                    });
                    const url = URL.createObjectURL(blob);
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = `audit-export-${result.event_export_run_id}.json`;
                    document.body.appendChild(a);
                    a.click();
                    document.body.removeChild(a);
                    URL.revokeObjectURL(url);
                    setExportReason("");
                    await loadData();
                  })().catch((err: unknown) => {
                    setError(toErrorMessage(err));
                  })
                }
              >
                <Download className="mr-1 h-4 w-4" />
                Export Audit Log
              </Button>
            </div>
          </div>
        )}

        {error && <div className="text-sm text-destructive">{error}</div>}
        {loading && <div className="text-sm text-muted-foreground">Loading audit log...</div>}

        <div className="rounded-md border">
          <div className="grid grid-cols-[1fr_auto_auto_auto_auto] gap-2 border-b bg-muted/40 px-3 py-2 text-xs font-medium text-muted-foreground">
            <span>Action</span>
            <span>Actor</span>
            <span>Auth</span>
            <span>Result</span>
            <span>Time</span>
          </div>
          <div className="max-h-[480px] overflow-auto">
            {visibleItems.map((row) => (
              <div key={row.id} className="border-b px-3 py-2 text-sm">
                <button
                  type="button"
                  className="grid w-full grid-cols-[1fr_auto_auto_auto_auto] gap-2 text-left"
                  onClick={() =>
                    void (async () => {
                      setExpandedId((prev) => (prev === row.id ? null : row.id));
                      if (!detailsMap[row.id]) {
                        try {
                          const detail = await getAuditEvent(row.id);
                          setDetailsMap((prev) => ({ ...prev, [row.id]: detail }));
                        } catch {
                          // keep row visible even when detail fetch fails
                        }
                      }
                    })()
                  }
                >
                  <span className="truncate">{row.action_code}</span>
                  <span className="text-xs text-muted-foreground">
                    {row.actor_username ?? row.actor_id ?? "system"}
                  </span>
                  <Badge variant="outline">{row.auth_context}</Badge>
                  <Badge variant={row.result === "success" ? "outline" : "destructive"}>{row.result}</Badge>
                  <span className="text-xs text-muted-foreground">{new Date(row.happened_at).toLocaleString()}</span>
                </button>
                {expandedId === row.id && (
                  <div className="mt-2 rounded bg-muted p-2 text-xs">
                    <div>
                      <span className="text-muted-foreground">target:</span> {row.target_type ?? "—"} /{" "}
                      {row.target_id ?? "—"}
                    </div>
                    <div>
                      <span className="text-muted-foreground">retention:</span> {row.retention_class}
                    </div>
                    <div className="mt-1 font-mono">
                      before_hash: {detailsMap[row.id]?.before_hash ?? "null"}
                    </div>
                    <div className="font-mono">after_hash: {detailsMap[row.id]?.after_hash ?? "null"}</div>
                    <pre className="mt-2 max-h-52 overflow-auto rounded border bg-background p-2 font-mono text-[11px]">
                      {JSON.stringify(detailsMap[row.id]?.details_json ?? {}, null, 2)}
                    </pre>
                  </div>
                )}
              </div>
            ))}
            {!loading && visibleItems.length === 0 && (
              <div className="p-4 text-center text-sm text-muted-foreground">No audit events found.</div>
            )}
          </div>
        </div>

        <div className="flex items-center justify-end gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={offset === 0}
            onClick={() => setOffset((prev) => Math.max(0, prev - PAGE_SIZE))}
          >
            Previous
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={items.length < PAGE_SIZE}
            onClick={() => setOffset((prev) => prev + PAGE_SIZE)}
          >
            Next
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
