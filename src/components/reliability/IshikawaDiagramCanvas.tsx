import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  useEdgesState,
  useNodesState,
  type Connection,
  type Edge,
  type Node,
  type Viewport,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { mfCard } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { cn } from "@/lib/utils";
import {
  listFmecaAnalyses,
  listRamIshikawaDiagrams,
  upsertFmecaItem,
  upsertRamIshikawaDiagram,
} from "@/services/reliability-service";
import { createWo } from "@/services/wo-service";
import { pushAppToast } from "@/store/app-toast-store";
import { toErrorMessage } from "@/utils/errors";
import type { RamIshikawaDiagram } from "@shared/ipc-types";

type FlowPersist = {
  nodes: Node[];
  edges: Edge[];
  viewport?: { x: number; y: number; zoom: number };
};

const SPEC_VERSION = 1;

function labelFromNode(node: Node): string {
  const d = node.data;
  if (d && typeof d === "object" && "label" in d) {
    return String((d as Record<string, unknown>)["label"] ?? "");
  }
  return "";
}

function isPromotableNodeId(id: string): boolean {
  if (id === "effect") {
    return false;
  }
  if (id.startsWith("cat_")) {
    return false;
  }
  return true;
}

function isViewport(v: unknown): v is Viewport {
  if (v == null || typeof v !== "object") {
    return false;
  }
  const o = v as Record<string, unknown>;
  return (
    typeof o["x"] === "number" &&
    typeof o["y"] === "number" &&
    typeof o["zoom"] === "number" &&
    Number.isFinite(o["x"]) &&
    Number.isFinite(o["y"]) &&
    Number.isFinite(o["zoom"])
  );
}

function buildDefaultFlow(t: (k: string) => string): FlowPersist {
  const problem: Node = {
    id: "effect",
    type: "default",
    position: { x: 520, y: 220 },
    data: { label: t("governance.effect") },
  };
  const cats: { id: string; label: string; x: number; y: number }[] = [
    { id: "cat_machine", label: t("governance.categories.machine"), x: 40, y: 40 },
    { id: "cat_method", label: t("governance.categories.method"), x: 180, y: 20 },
    { id: "cat_material", label: t("governance.categories.material"), x: 320, y: 40 },
    { id: "cat_manpower", label: t("governance.categories.manpower"), x: 40, y: 360 },
    { id: "cat_measurement", label: t("governance.categories.measurement"), x: 220, y: 400 },
    { id: "cat_nature", label: t("governance.categories.motherNature"), x: 380, y: 360 },
  ];
  const branchNodes: Node[] = cats.map((c) => ({
    id: c.id,
    type: "default",
    position: { x: c.x, y: c.y },
    data: { label: c.label },
  }));
  const nodes = [problem, ...branchNodes];
  const edges: Edge[] = cats.map((c) => ({
    id: `e_${c.id}`,
    source: c.id,
    target: problem.id,
    type: "smoothstep",
  }));
  return { nodes, edges };
}

function IshikawaDiagramInner({ equipmentId }: { equipmentId: number }) {
  const { t } = useTranslation("reliability");
  const { info } = useSession();
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [meta, setMeta] = useState<RamIshikawaDiagram | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadedViewport, setLoadedViewport] = useState<Viewport | null>(null);
  const [flowMountKey, setFlowMountKey] = useState(0);
  const viewportRef = useRef<Viewport>({ x: 0, y: 0, zoom: 1 });

  const [causeOpen, setCauseOpen] = useState(false);
  const [causeDraft, setCauseDraft] = useState("");
  const [pendingCategoryId, setPendingCategoryId] = useState<string | null>(null);
  const [editingNodeId, setEditingNodeId] = useState<string | null>(null);

  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; nodeId: string } | null>(null);
  const [promoteOpen, setPromoteOpen] = useState(false);
  const [promoteTarget, setPromoteTarget] = useState<"wo" | "fmeca">("wo");
  const [promoteNodeId, setPromoteNodeId] = useState<string | null>(null);
  const [promoteCause, setPromoteCause] = useState("");
  const [promoteBusy, setPromoteBusy] = useState(false);

  const load = useCallback(async () => {
    setErr(null);
    try {
      const rows = await listRamIshikawaDiagrams({ equipment_id: equipmentId, limit: 1 });
      const row = rows[0];
      if (row?.flow_json) {
        try {
          const parsed = JSON.parse(row.flow_json) as { spec_version?: number } & FlowPersist;
          if (parsed.nodes?.length) {
            setNodes(parsed.nodes);
            setEdges(parsed.edges ?? []);
            const vp = parsed.viewport;
            setLoadedViewport(isViewport(vp) ? vp : null);
            viewportRef.current = isViewport(vp) ? vp : { x: 0, y: 0, zoom: 1 };
            setFlowMountKey((k) => k + 1);
            setMeta(row);
            return;
          }
        } catch {
          /* fall through to default */
        }
        const d = buildDefaultFlow(t);
        setNodes(d.nodes);
        setEdges(d.edges);
        setLoadedViewport(null);
        viewportRef.current = { x: 0, y: 0, zoom: 1 };
        setFlowMountKey((k) => k + 1);
        setMeta(row);
        return;
      }
      const d = buildDefaultFlow(t);
      setNodes(d.nodes);
      setEdges(d.edges);
      setLoadedViewport(null);
      viewportRef.current = { x: 0, y: 0, zoom: 1 };
      setFlowMountKey((k) => k + 1);
      setMeta(null);
    } catch {
      setErr(t("governance.loadError"));
      const d = buildDefaultFlow(t);
      setNodes(d.nodes);
      setEdges(d.edges);
      setLoadedViewport(null);
      setMeta(null);
    }
  }, [equipmentId, setEdges, setNodes, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const onConnect = useCallback(
    (c: Connection) => setEdges((eds) => addEdge({ ...c, type: "smoothstep" }, eds)),
    [setEdges],
  );

  const openAddCause = (categoryId: string) => {
    setPendingCategoryId(categoryId);
    setEditingNodeId(null);
    setCauseDraft("");
    setCauseOpen(true);
  };

  const commitCause = async () => {
    const label = causeDraft.trim();
    if (label === "") {
      return;
    }
    let nextNodes = nodes;
    let nextEdges = edges;
    if (editingNodeId != null) {
      nextNodes = nodes.map((n) =>
        n.id === editingNodeId ? { ...n, data: { ...n.data, label } } : n,
      );
    } else if (pendingCategoryId != null) {
      const categoryId = pendingCategoryId;
      const id = `cause_${Date.now().toString(36)}`;
      const cat = nodes.find((n) => n.id === categoryId);
      const position = cat
        ? { x: cat.position.x + 30, y: cat.position.y + (Math.random() * 40 + 24) }
        : { x: 200, y: 180 };
      const n: Node = {
        id,
        type: "default",
        position,
        data: { label },
      };
      nextNodes = [...nodes, n];
      nextEdges = [...edges, { id: `e_${id}`, source: categoryId, target: id, type: "smoothstep" }];
    }
    setNodes(nextNodes);
    setEdges(nextEdges);
    setCauseOpen(false);
    setPendingCategoryId(null);
    setEditingNodeId(null);
    setSaving(true);
    setErr(null);
    try {
      const saved = await persistFlow(nextNodes, nextEdges);
      setMeta(saved);
      await load();
      pushAppToast({ title: t("governance.saved"), variant: "success" });
    } catch (e) {
      const msg = toErrorMessage(e);
      setErr(msg);
      pushAppToast({ title: msg, variant: "destructive" });
    } finally {
      setSaving(false);
    }
  };

  const onNodeDoubleClick = useCallback((_e: MouseEvent, node: Node) => {
    const d = node.data;
    const raw =
      d && typeof d === "object" && "label" in d ? (d as Record<string, unknown>)["label"] : "";
    setEditingNodeId(node.id);
    setPendingCategoryId(null);
    setCauseDraft(String(raw ?? ""));
    setCauseOpen(true);
  }, []);

  const onMoveEnd = useCallback((_e: unknown, viewport: Viewport) => {
    viewportRef.current = viewport;
  }, []);

  const persistFlow = useCallback(
    async (draftNodes = nodes, draftEdges = edges): Promise<RamIshikawaDiagram> => {
      const flow_json = JSON.stringify({
        spec_version: SPEC_VERSION,
        nodes: draftNodes,
        edges: draftEdges,
        viewport: viewportRef.current,
      });
      return upsertRamIshikawaDiagram({
        id: meta?.id ?? null,
        equipment_id: equipmentId,
        expected_row_version: meta?.row_version ?? null,
        title: "Ishikawa",
        flow_json,
      });
    },
    [nodes, edges, meta, equipmentId],
  );

  const onSave = async () => {
    setSaving(true);
    setErr(null);
    try {
      const saved = await persistFlow();
      setMeta(saved);
      pushAppToast({ title: t("governance.saved"), variant: "success" });
    } catch (e) {
      const msg = toErrorMessage(e);
      setErr(msg);
      pushAppToast({ title: msg, variant: "destructive" });
    } finally {
      setSaving(false);
    }
  };

  const openPromoteFromCtx = (nodeId: string) => {
    const n = nodes.find((x) => x.id === nodeId);
    if (!n) {
      return;
    }
    setPromoteNodeId(nodeId);
    setPromoteCause(labelFromNode(n));
    setPromoteTarget("wo");
    setPromoteOpen(true);
    setCtxMenu(null);
  };

  const onPromoteSubmit = async () => {
    const cause = promoteCause.trim();
    if (cause === "" || promoteNodeId == null) {
      return;
    }
    if (promoteTarget === "wo" && info?.user_id == null) {
      setErr(t("governance.promoteNeedUser"));
      return;
    }
    setPromoteBusy(true);
    setErr(null);
    try {
      const saved = await persistFlow();
      setMeta(saved);
      if (promoteTarget === "wo" && info?.user_id != null) {
        await createWo({
          type_code: "corrective",
          equipment_id: equipmentId,
          title: `RCA: ${cause}`,
          description: cause,
          creator_id: info.user_id,
          source_ram_ishikawa_diagram_id: saved.id,
          source_ishikawa_flow_node_id: promoteNodeId,
          source_rca_cause_text: cause,
        });
        pushAppToast({ title: t("governance.promoteSuccessWo"), variant: "success" });
      } else {
        const analyses = await listFmecaAnalyses({ equipment_id: equipmentId, limit: 5 });
        const a = analyses[0];
        if (a == null) {
          setErr(t("governance.promoteNeedFmecaAnalysis"));
          return;
        }
        await upsertFmecaItem({
          analysis_id: a.id,
          severity: 5,
          occurrence: 5,
          detectability: 5,
          functional_failure: cause,
          failure_effect: cause,
          recommended_action: `Address RCA cause: ${cause}`,
          source_ram_ishikawa_diagram_id: saved.id,
          source_ishikawa_flow_node_id: promoteNodeId,
        });
        pushAppToast({ title: t("governance.promoteSuccessFmeca"), variant: "success" });
      }
      setPromoteOpen(false);
      setPromoteNodeId(null);
    } catch (e) {
      setErr(toErrorMessage(e));
    } finally {
      setPromoteBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      {ctxMenu ? (
        <div
          className="fixed z-[300] min-w-[200px] rounded-md border border-surface-border bg-surface-1 py-1 text-xs shadow-lg"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          role="menu"
        >
          <button
            type="button"
            className="block w-full px-3 py-2 text-left text-text-primary hover:bg-surface-2"
            onClick={() => openPromoteFromCtx(ctxMenu.nodeId)}
          >
            {t("governance.promoteMenu")}
          </button>
        </div>
      ) : null}
      {err ? <p className="text-xs text-text-danger">{err}</p> : null}
      <div className="flex flex-wrap gap-2">
        {(
          [
            ["cat_machine", "governance.categories.machine"],
            ["cat_method", "governance.categories.method"],
            ["cat_material", "governance.categories.material"],
            ["cat_manpower", "governance.categories.manpower"],
            ["cat_measurement", "governance.categories.measurement"],
            ["cat_nature", "governance.categories.motherNature"],
          ] as const
        ).map(([id, key]) => (
          <button
            key={id}
            type="button"
            className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-[11px] text-text-primary hover:bg-surface-3"
            onClick={() => openAddCause(id)}
          >
            + {t("governance.addCause")} ({t(key)})
          </button>
        ))}
        <button
          type="button"
          disabled={saving}
          className="rounded-md border border-primary/35 bg-primary/10 px-3 py-1 text-xs font-medium text-text-primary disabled:opacity-50"
          onClick={() => void onSave()}
        >
          {saving ? "…" : t("governance.save")}
        </button>
      </div>
      <div className={cn(mfCard.insetCanvas, "min-h-[420px]")}>
        <ReactFlow
          key={flowMountKey}
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onNodeDoubleClick={onNodeDoubleClick}
          onPaneClick={() => setCtxMenu(null)}
          onNodeContextMenu={(e, node) => {
            e.preventDefault();
            if (!isPromotableNodeId(node.id)) {
              return;
            }
            setCtxMenu({ x: e.clientX, y: e.clientY, nodeId: node.id });
          }}
          onMoveEnd={onMoveEnd}
          fitView={loadedViewport == null}
          defaultViewport={loadedViewport ?? { x: 0, y: 0, zoom: 1 }}
          minZoom={0.35}
          maxZoom={1.5}
          proOptions={{ hideAttribution: true }}
          className="bg-transparent"
        >
          <Background
            variant={BackgroundVariant.Dots}
            gap={20}
            size={1}
            color="var(--surface-border)"
          />
          <Controls className="!border-surface-border !bg-surface-1" />
          <MiniMap className="!border-surface-border !bg-surface-1" />
        </ReactFlow>
      </div>

      <Dialog open={causeOpen} onOpenChange={setCauseOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {editingNodeId != null
                ? t("governance.causeModalEditTitle")
                : t("governance.causeModalTitle")}
            </DialogTitle>
          </DialogHeader>
          <label className="grid gap-1.5 text-sm">
            <span className="text-text-muted">{t("governance.causeModalLabel")}</span>
            <input
              className="rounded-md border border-surface-border bg-surface-2 px-3 py-2 text-sm text-text-primary"
              value={causeDraft}
              onChange={(e) => setCauseDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void commitCause();
                }
              }}
              autoFocus
            />
          </label>
          <DialogFooter className="gap-2 sm:gap-0">
            <button
              type="button"
              className="rounded-md border border-surface-border px-3 py-1.5 text-xs text-text-secondary hover:bg-surface-2"
              onClick={() => setCauseOpen(false)}
            >
              {t("governance.causeModalCancel")}
            </button>
            <button
              type="button"
              className="rounded-md border border-primary/35 bg-primary/10 px-3 py-1.5 text-xs font-medium text-text-primary"
              onClick={() => void commitCause()}
            >
              {t("governance.causeModalSave")}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={promoteOpen} onOpenChange={setPromoteOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("governance.promoteTitle")}</DialogTitle>
          </DialogHeader>
          <p className="text-xs text-text-muted">{t("governance.promoteBody")}</p>
          <label className="grid gap-1.5 text-sm">
            <span className="text-text-muted">{t("governance.causeModalLabel")}</span>
            <input
              className="rounded-md border border-surface-border bg-surface-2 px-3 py-2 text-sm text-text-primary"
              value={promoteCause}
              onChange={(e) => setPromoteCause(e.target.value)}
            />
          </label>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={cn(
                "rounded-md border px-3 py-1.5 text-xs",
                promoteTarget === "wo"
                  ? "border-primary/40 bg-primary/10 text-text-primary"
                  : "border-surface-border bg-surface-2 text-text-secondary",
              )}
              onClick={() => setPromoteTarget("wo")}
            >
              {t("governance.promoteTargetWo")}
            </button>
            <button
              type="button"
              className={cn(
                "rounded-md border px-3 py-1.5 text-xs",
                promoteTarget === "fmeca"
                  ? "border-primary/40 bg-primary/10 text-text-primary"
                  : "border-surface-border bg-surface-2 text-text-secondary",
              )}
              onClick={() => setPromoteTarget("fmeca")}
            >
              {t("governance.promoteTargetFmeca")}
            </button>
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <button
              type="button"
              className="rounded-md border border-surface-border px-3 py-1.5 text-xs text-text-secondary hover:bg-surface-2"
              onClick={() => setPromoteOpen(false)}
            >
              {t("governance.causeModalCancel")}
            </button>
            <button
              type="button"
              disabled={promoteBusy}
              className="rounded-md border border-primary/35 bg-primary/10 px-3 py-1.5 text-xs font-medium text-text-primary disabled:opacity-50"
              onClick={() => void onPromoteSubmit()}
            >
              {promoteBusy ? "…" : t("governance.promoteSubmit")}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

export function IshikawaDiagramCanvas({ equipmentId }: { equipmentId: number }) {
  return (
    <ReactFlowProvider>
      <IshikawaDiagramInner equipmentId={equipmentId} />
    </ReactFlowProvider>
  );
}
