import {
  Background,
  BackgroundVariant,
  type Connection,
  Controls,
  type Edge,
  type EdgeChange,
  MiniMap,
  type Node,
  type NodeChange,
  ReactFlow,
  ReactFlowProvider,
  type Viewport,
  addEdge,
  useEdgesState,
  useNodesState,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { type MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { UnsavedChangesGuard } from "@/components/ui/unsaved-changes-guard";
import { woTypeLabel } from "@/components/wo/use-wo-type-labels";
import { mfCard } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { cn } from "@/lib/utils";
import { listRamIshikawaDiagrams, upsertRamIshikawaDiagram } from "@/services/reliability-service";
import { createWo } from "@/services/wo-service";
import { pushAppToast } from "@/store/app-toast-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type { RamIshikawaDiagram } from "@shared/ipc-types";

/** A diagram cause handed to the study's FMECA form (the analyst rates it there). */
export interface IshikawaFmecaPromotion {
  cause: string;
  diagramId: number;
  nodeId: string;
}

/** Corrective natures (EN 13306) a root-cause work order may take. */
const CORRECTIVE_NATURES = new Set(["corrective_immediate", "corrective_deferred"]);

type FlowPersist = {
  nodes: Node[];
  edges: Edge[];
  viewport?: { x: number; y: number; zoom: number };
};

const SPEC_VERSION = 1;
const FISHBONE_EFFECT_ID = "effect";
const FISHBONE_SPINE_START_ID = "spine_start";
const FISHBONE_SPINE_END_ID = "spine_end";
const FISHBONE_CATEGORIES = [
  "cat_machine",
  "cat_method",
  "cat_material",
  "cat_manpower",
  "cat_measurement",
  "cat_nature",
] as const;
const FISHBONE_TOP = new Set<string>(["cat_machine", "cat_method", "cat_material"]);
const FISHBONE_SPINE_Y = 500;

function isFishboneHelperNode(id: string): boolean {
  return id.startsWith("spine_");
}

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
  if (isFishboneHelperNode(id)) {
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

function getNodeLabel(node: Node | undefined, fallback: string): string {
  if (!node?.data || typeof node.data !== "object") {
    return fallback;
  }
  const raw = (node.data as Record<string, unknown>)["label"];
  if (typeof raw === "string" && raw.trim() !== "") {
    return raw;
  }
  return fallback;
}

function applyFishboneLayout(nodes: Node[], edges: Edge[], t: (k: string) => string): Node[] {
  const byId = new Map(nodes.map((n) => [n.id, n] as const));
  const positions: Record<string, { x: number; y: number }> = {
    [FISHBONE_SPINE_START_ID]: { x: 0, y: FISHBONE_SPINE_Y },
    [FISHBONE_SPINE_END_ID]: { x: 1000, y: FISHBONE_SPINE_Y },
    effect: { x: 1000, y: 470 },
    cat_machine: { x: 160, y: 300 },
    cat_method: { x: 340, y: 300 },
    cat_material: { x: 520, y: 300 },
    cat_manpower: { x: 160, y: 700 },
    cat_measurement: { x: 340, y: 700 },
    cat_nature: { x: 520, y: 700 },
    spine_cat_machine: { x: 360, y: FISHBONE_SPINE_Y },
    spine_cat_method: { x: 540, y: FISHBONE_SPINE_Y },
    spine_cat_material: { x: 720, y: FISHBONE_SPINE_Y },
    spine_cat_manpower: { x: 360, y: FISHBONE_SPINE_Y },
    spine_cat_measurement: { x: 540, y: FISHBONE_SPINE_Y },
    spine_cat_nature: { x: 720, y: FISHBONE_SPINE_Y },
  };

  const baseLabels: Record<string, string> = {
    effect: t("governance.effect"),
    cat_machine: t("governance.categories.machine"),
    cat_method: t("governance.categories.method"),
    cat_material: t("governance.categories.material"),
    cat_manpower: t("governance.categories.manpower"),
    cat_measurement: t("governance.categories.measurement"),
    cat_nature: t("governance.categories.motherNature"),
  };

  const nextById = new Map<string, Node>();
  const fallbackPos = { x: 0, y: FISHBONE_SPINE_Y };
  const getPos = (id: string) => positions[id] ?? fallbackPos;
  const spineHelpers: Node[] = [
    {
      id: FISHBONE_SPINE_START_ID,
      type: "default",
      position: getPos(FISHBONE_SPINE_START_ID),
      data: { label: "" },
      draggable: false,
      selectable: false,
      style: { width: 1, height: 1, opacity: 0, border: "none", background: "transparent" },
    },
    {
      id: FISHBONE_SPINE_END_ID,
      type: "default",
      position: getPos(FISHBONE_SPINE_END_ID),
      data: { label: "" },
      draggable: false,
      selectable: false,
      style: { width: 1, height: 1, opacity: 0, border: "none", background: "transparent" },
    },
  ];
  for (const helper of spineHelpers) {
    nextById.set(helper.id, helper);
  }

  for (const catId of FISHBONE_CATEGORIES) {
    const spineId = `spine_${catId}`;
    nextById.set(spineId, {
      id: spineId,
      type: "default",
      position: getPos(spineId),
      data: { label: "" },
      draggable: false,
      selectable: false,
      style: { width: 1, height: 1, opacity: 0, border: "none", background: "transparent" },
    });
  }

  for (const id of [FISHBONE_EFFECT_ID, ...FISHBONE_CATEGORIES]) {
    const existing = byId.get(id);
    const p = getPos(id);
    nextById.set(id, {
      id,
      type: "default",
      position: p,
      data: { label: getNodeLabel(existing, baseLabels[id] ?? "") },
    });
  }

  const orderedCauseIds = new Map<string, string[]>();
  for (const catId of FISHBONE_CATEGORIES) {
    orderedCauseIds.set(catId, []);
  }
  for (const e of edges) {
    if (!orderedCauseIds.has(e.source)) {
      continue;
    }
    if (e.target === FISHBONE_EFFECT_ID) {
      continue;
    }
    if (!byId.has(e.target)) {
      continue;
    }
    const arr = orderedCauseIds.get(e.source);
    if (arr && !arr.includes(e.target)) {
      arr.push(e.target);
    }
  }

  for (const catId of FISHBONE_CATEGORIES) {
    const catPos = getPos(catId);
    const causes = orderedCauseIds.get(catId) ?? [];
    const isTop = FISHBONE_TOP.has(catId);
    causes.forEach((causeId, idx) => {
      const existing = byId.get(causeId);
      if (!existing) {
        return;
      }
      const rank = idx + 1;
      const x = catPos.x - 85 - idx * 38;
      const y = isTop ? catPos.y - rank * 50 : catPos.y + rank * 50;
      nextById.set(causeId, { ...existing, position: { x, y } });
    });
  }

  for (const n of nodes) {
    if (!nextById.has(n.id)) {
      nextById.set(n.id, n);
    }
  }
  return Array.from(nextById.values());
}

function buildFishboneEdges(nodes: Node[], edges: Edge[]): Edge[] {
  const nodeIds = new Set(nodes.map((n) => n.id));
  const out: Edge[] = [
    {
      id: "spine_main",
      source: FISHBONE_SPINE_START_ID,
      target: FISHBONE_SPINE_END_ID,
      type: "straight",
      style: { strokeWidth: 4 },
      selectable: false,
      focusable: false,
    },
  ];

  const uniqueEdge = new Set<string>(["spine_main"]);
  for (const catId of FISHBONE_CATEGORIES) {
    const anchorId = `spine_${catId}`;
    if (!nodeIds.has(catId) || !nodeIds.has(anchorId)) {
      continue;
    }
    const id = `rib_${catId}`;
    out.push({
      id,
      source: catId,
      target: anchorId,
      type: "straight",
      style: { strokeWidth: 2 },
    });
    uniqueEdge.add(id);
  }

  for (const e of edges) {
    if (!nodeIds.has(e.source) || !nodeIds.has(e.target)) {
      continue;
    }
    if (e.target === FISHBONE_EFFECT_ID) {
      continue;
    }
    if (isFishboneHelperNode(e.source) || isFishboneHelperNode(e.target)) {
      continue;
    }
    const id = `cause_${e.source}_${e.target}`;
    if (uniqueEdge.has(id)) {
      continue;
    }
    out.push({
      ...e,
      id,
      type: "straight",
      style: { strokeWidth: 1.6 },
    });
    uniqueEdge.add(id);
  }
  return out;
}

function buildDefaultFlow(t: (k: string) => string): FlowPersist {
  const problem: Node = {
    id: FISHBONE_EFFECT_ID,
    type: "default",
    position: { x: 980, y: 260 },
    data: { label: t("governance.effect") },
  };
  const cats: { id: string; label: string; x: number; y: number }[] = [
    { id: "cat_machine", label: t("governance.categories.machine"), x: 280, y: 130 },
    { id: "cat_method", label: t("governance.categories.method"), x: 500, y: 130 },
    { id: "cat_material", label: t("governance.categories.material"), x: 720, y: 130 },
    { id: "cat_manpower", label: t("governance.categories.manpower"), x: 280, y: 390 },
    { id: "cat_measurement", label: t("governance.categories.measurement"), x: 500, y: 390 },
    { id: "cat_nature", label: t("governance.categories.motherNature"), x: 720, y: 390 },
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
    type: "straight",
  }));
  const fishNodes = applyFishboneLayout(nodes, edges, t);
  return { nodes: fishNodes, edges: buildFishboneEdges(fishNodes, edges) };
}

/** A change the analyst made on the canvas that only "Save diagram" stores (moves, links, deletions). */
function isUnsavedNodeChange(c: NodeChange): boolean {
  return (c.type === "position" && c.dragging === false) || c.type === "remove";
}

function IshikawaDiagramInner({
  equipmentId,
  onPromoteToFmeca,
  onDirtyChange,
}: {
  equipmentId: number;
  onPromoteToFmeca?: (promotion: IshikawaFmecaPromotion) => void;
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const { t, i18n } = useTranslation("reliability");
  const { info } = useSession();
  const [nodes, setNodes, applyNodeChanges] = useNodesState<Node>([]);
  const [edges, setEdges, applyEdgeChanges] = useEdgesState<Edge>([]);
  /** Moves, links or deletions not stored yet ("Save diagram" stores them). */
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  // Unmounting (equipment switch, leaving the study) clears the host's flag.
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  const onNodesChange = useCallback(
    (changes: NodeChange[]) => {
      applyNodeChanges(changes);
      if (changes.some(isUnsavedNodeChange)) setDirty(true);
    },
    [applyNodeChanges],
  );
  const onEdgesChange = useCallback(
    (changes: EdgeChange[]) => {
      applyEdgeChanges(changes);
      if (changes.some((c) => c.type === "remove")) setDirty(true);
    },
    [applyEdgeChanges],
  );
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
  const [promoteWoType, setPromoteWoType] = useState("");
  /** Set when the stored diagram could not be read: nothing may be saved over it. */
  const [loadError, setLoadError] = useState<unknown>(null);
  const [reloading, setReloading] = useState(false);

  const woTypes = useWorkOrderTypesCatalog((st) => st.types);
  const loadWoTypes = useWorkOrderTypesCatalog((st) => st.load);
  useEffect(() => {
    const { types: loaded, loading } = useWorkOrderTypesCatalog.getState();
    if (loaded.length === 0 && !loading) void loadWoTypes();
  }, [loadWoTypes]);
  const correctiveTypes = useMemo(
    () =>
      woTypes.filter(
        (wt) => wt.is_active && wt.nature != null && CORRECTIVE_NATURES.has(wt.nature),
      ),
    [woTypes],
  );

  const load = useCallback(async () => {
    setErr(null);
    setLoadError(null);
    setReloading(true);
    setDirty(false);
    try {
      const rows = await listRamIshikawaDiagrams({ equipment_id: equipmentId, limit: 1 });
      const row = rows[0];
      if (row?.flow_json) {
        try {
          const parsed = JSON.parse(row.flow_json) as { spec_version?: number } & FlowPersist;
          if (parsed.nodes?.length) {
            const sourceEdges = parsed.edges ?? [];
            const nextNodes = applyFishboneLayout(parsed.nodes, sourceEdges, t);
            const nextEdges = buildFishboneEdges(nextNodes, sourceEdges);
            setNodes(nextNodes);
            setEdges(nextEdges);
            const vp = parsed.viewport;
            setLoadedViewport(isViewport(vp) ? vp : null);
            viewportRef.current = isViewport(vp) ? vp : { x: 0, y: 0, zoom: 1 };
            setFlowMountKey((k) => k + 1);
            setMeta(row);
            return;
          }
        } catch {
          // The stored diagram cannot be read: never replace it with an empty one on save.
          setMeta(row);
          setLoadError(t("governance.unreadable"));
          return;
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
    } catch (e) {
      // Without the stored diagram a save would create a second one: show the failure instead.
      setLoadError(e);
    } finally {
      setReloading(false);
    }
  }, [equipmentId, setEdges, setNodes, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const onConnect = useCallback(
    (c: Connection) => {
      setEdges((eds) => addEdge({ ...c, type: "straight", style: { strokeWidth: 1.6 } }, eds));
      setDirty(true);
    },
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
      nextEdges = [...edges, { id: `e_${id}`, source: categoryId, target: id, type: "straight" }];
    }
    nextNodes = applyFishboneLayout(nextNodes, nextEdges, t);
    nextEdges = buildFishboneEdges(nextNodes, nextEdges);
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
    } catch {
      // The global IPC toast reports the failure; the cause stays on the canvas, not stored yet.
      setDirty(true);
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
        title: t("governance.diagramTitle"),
        flow_json,
      });
    },
    [nodes, edges, meta, equipmentId, t],
  );

  const onSave = async () => {
    setSaving(true);
    setErr(null);
    try {
      const saved = await persistFlow();
      setMeta(saved);
      setDirty(false);
      pushAppToast({ title: t("governance.saved"), variant: "success" });
    } catch {
      // The global IPC toast reports the failure.
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
    setPromoteWoType("");
    setPromoteOpen(true);
    setCtxMenu(null);
  };

  const onPromoteSubmit = async () => {
    const cause = promoteCause.trim();
    if (cause === "" || promoteNodeId == null) {
      return;
    }
    if (promoteTarget === "wo" && (info?.user_id == null || promoteWoType === "")) {
      return;
    }
    setPromoteBusy(true);
    setErr(null);
    try {
      const saved = await persistFlow();
      setMeta(saved);
      setDirty(false);
      if (promoteTarget === "wo" && info?.user_id != null) {
        await createWo({
          type_code: promoteWoType,
          equipment_id: equipmentId,
          title: t("governance.promoteWoTitle", { cause }),
          description: cause,
          creator_id: info.user_id,
          source_ram_ishikawa_diagram_id: saved.id,
          source_ishikawa_flow_node_id: promoteNodeId,
          source_rca_cause_text: cause,
        });
        pushAppToast({ title: t("governance.promoteSuccessWo"), variant: "success" });
      } else if (onPromoteToFmeca) {
        // The study's FMECA form opens: the analyst chooses the mode and every rating.
        onPromoteToFmeca({ cause, diagramId: saved.id, nodeId: promoteNodeId });
      }
      setPromoteOpen(false);
      setPromoteNodeId(null);
    } catch {
      // The global IPC toast reports the failure; the dialog stays open.
    } finally {
      setPromoteBusy(false);
    }
  };

  if (loadError != null) {
    return (
      <ErrorState
        error={loadError}
        title={t("governance.loadError")}
        onRetry={() => void load()}
        retrying={reloading}
      />
    );
  }

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
          {saving ? t("governance.promoteBusy") : t("governance.save")}
        </button>
        {dirty ? <StatusChip tone="warning" dot label={t("governance.unsaved")} /> : null}
      </div>
      <UnsavedChangesGuard
        when={dirty}
        title={t("governance.unsavedTitle")}
        description={t("governance.unsavedBody")}
        confirmLabel={t("governance.unsavedDiscard")}
      />
      <div className={cn(mfCard.insetCanvas, "h-[860px] min-h-[860px]")}>
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
          <div
            className="flex flex-wrap gap-2"
            role="radiogroup"
            aria-label={t("governance.promoteTarget")}
          >
            <button
              type="button"
              role="radio"
              aria-checked={promoteTarget === "wo"}
              className={cn(
                "min-h-[44px] rounded-md border px-3 py-1.5 text-xs",
                promoteTarget === "wo"
                  ? "border-primary/40 bg-primary/10 text-text-primary"
                  : "border-surface-border bg-surface-2 text-text-secondary",
              )}
              onClick={() => setPromoteTarget("wo")}
            >
              {t("governance.promoteTargetWo")}
            </button>
            {onPromoteToFmeca ? (
              <button
                type="button"
                role="radio"
                aria-checked={promoteTarget === "fmeca"}
                className={cn(
                  "min-h-[44px] rounded-md border px-3 py-1.5 text-xs",
                  promoteTarget === "fmeca"
                    ? "border-primary/40 bg-primary/10 text-text-primary"
                    : "border-surface-border bg-surface-2 text-text-secondary",
                )}
                onClick={() => setPromoteTarget("fmeca")}
              >
                {t("governance.promoteTargetFmeca")}
              </button>
            ) : null}
          </div>
          {promoteTarget === "wo" ? (
            <label className="grid gap-1.5 text-sm">
              <span className="text-text-muted">{t("governance.promoteWoType")}</span>
              <Select value={promoteWoType} onValueChange={setPromoteWoType}>
                <SelectTrigger className="h-10" aria-label={t("governance.promoteWoType")}>
                  <SelectValue placeholder={t("governance.promoteWoTypePlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {correctiveTypes.map((wt) => (
                    <SelectItem key={wt.code} value={wt.code}>
                      {woTypeLabel(wt, i18n.language)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
          ) : (
            <p className="text-xs text-text-muted">{t("governance.promoteFmecaHint")}</p>
          )}
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
              disabled={
                promoteBusy ||
                promoteCause.trim() === "" ||
                (promoteTarget === "wo" && (promoteWoType === "" || info?.user_id == null))
              }
              className="min-h-[44px] rounded-md border border-primary/35 bg-primary/10 px-3 py-1.5 text-xs font-medium text-text-primary disabled:opacity-50"
              onClick={() => void onPromoteSubmit()}
            >
              {promoteBusy
                ? t("governance.promoteBusy")
                : promoteTarget === "fmeca"
                  ? t("governance.promoteOpenFmeca")
                  : t("governance.promoteSubmit")}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

export function IshikawaDiagramCanvas({
  equipmentId,
  onPromoteToFmeca,
  onDirtyChange,
}: {
  equipmentId: number;
  /** Hands a cause to the study's FMECA form; without it a cause can only become a work order. */
  onPromoteToFmeca?: (promotion: IshikawaFmecaPromotion) => void;
  /** Told when the canvas holds changes "Save diagram" has not stored yet. */
  onDirtyChange?: (dirty: boolean) => void;
}) {
  return (
    <ReactFlowProvider>
      <IshikawaDiagramInner
        equipmentId={equipmentId}
        {...(onPromoteToFmeca ? { onPromoteToFmeca } : {})}
        {...(onDirtyChange ? { onDirtyChange } : {})}
      />
    </ReactFlowProvider>
  );
}
