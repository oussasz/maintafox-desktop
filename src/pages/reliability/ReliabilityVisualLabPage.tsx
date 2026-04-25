import { Graph } from "@antv/x6";
import { Selection } from "@antv/x6-plugin-selection";
import { Snapline } from "@antv/x6-plugin-snapline";
import { Stencil } from "@antv/x6-plugin-stencil";
import { Portal, register } from "@antv/x6-react-shape";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import "@antv/x6-plugin-stencil/dist/index.css";
import "@/components/reliability/flow/rams-x6-lab.css";
import {
  flowToFtaJson,
  flowToRbdJson,
  ftaJsonToFlow,
  rbdJsonToFlow,
} from "@/components/reliability/flow/ftaRbdConverters";
import { useTheme } from "@/components/ui/ThemeProvider";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  createFtaModel,
  createRbdModel,
  deleteFtaModel,
  deleteRbdModel,
  evaluateFtaModel,
  evaluateRbdModel,
  listFtaModels,
  listRbdModels,
  updateFtaModel,
  updateRbdModel,
} from "@/services/reliability-service";
import type { FtaModel, RbdModel } from "@shared/ipc-types";

import { useRequiredRamsEquipmentId } from "./rams-equipment-context";

const DEFAULT_FTA =
  '{"spec_version":1,"top_id":"top","nodes":{"top":{"kind":"or","inputs":["a","b"]},"a":{"kind":"basic","p":0.01},"b":{"kind":"basic","p":0.02}}}';
const DEFAULT_RBD =
  '{"spec_version":1,"root_id":"root","nodes":{"root":{"kind":"series","children":["x","y"]},"x":{"kind":"block","r":0.99},"y":{"kind":"block","r":0.95}}}';

type LabMode = "fta" | "rbd";

type FlowNodeLite = {
  id: string;
  type: string;
  position: { x: number; y: number };
  data?: Record<string, unknown>;
};
type FlowEdgeLite = {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string | null;
  targetHandle?: string | null;
};

type X6CellData = Record<string, unknown>;
type X6GraphPayload = {
  cells: X6CellData[];
  ramMeta?: { mode?: LabMode; topOrRoot?: string };
};

const SHAPE = {
  ftaAnd: "rams-fta-and",
  ftaOr: "rams-fta-or",
  ftaBe: "rams-fta-be",
  rbdBlock: "rams-rbd-block",
  rbdSeries: "rams-rbd-series",
  rbdParallel: "rams-rbd-parallel",
} as const;

const shapeToFlowType: Record<string, FlowNodeLite["type"]> = {
  [SHAPE.ftaAnd]: "ftaAnd",
  [SHAPE.ftaOr]: "ftaOr",
  [SHAPE.ftaBe]: "ftaBe",
  [SHAPE.rbdBlock]: "rbdBlock",
  [SHAPE.rbdSeries]: "rbdSeries",
  [SHAPE.rbdParallel]: "rbdParallel",
};
const flowTypeToShape: Record<string, string> = {
  ftaAnd: SHAPE.ftaAnd,
  ftaOr: SHAPE.ftaOr,
  ftaBe: SHAPE.ftaBe,
  rbdBlock: SHAPE.rbdBlock,
  rbdSeries: SHAPE.rbdSeries,
  rbdParallel: SHAPE.rbdParallel,
};

function readCellX(cell: X6CellData): number {
  const p = cell["position"];
  if (p && typeof p === "object" && p !== null && "x" in p) {
    return Number((p as { x: unknown }).x) || 0;
  }
  return Number(cell["x"]) || 0;
}
function readCellY(cell: X6CellData): number {
  const p = cell["position"];
  if (p && typeof p === "object" && p !== null && "y" in p) {
    return Number((p as { y: unknown }).y) || 0;
  }
  return Number(cell["y"]) || 0;
}

function parseLegacyToX6(
  mode: LabMode,
  graphJson: string,
): { payload: X6GraphPayload; topOrRoot: string } {
  const topOrRoot =
    mode === "fta" ? ftaJsonToFlow(graphJson).topId : rbdJsonToFlow(graphJson).rootId;
  const flow = mode === "fta" ? ftaJsonToFlow(graphJson) : rbdJsonToFlow(graphJson);
  const nodes = flow.nodes as unknown as FlowNodeLite[];
  const edges = flow.edges as unknown as FlowEdgeLite[];
  const targetOrder = new Map<string, number>();
  const nodeById = new Map(nodes.map((n) => [n.id, n]));
  const cells: X6CellData[] = [];

  for (const n of nodes) {
    const shape = flowTypeToShape[n.type];
    if (!shape) {
      continue;
    }
    cells.push({
      id: n.id,
      shape,
      x: n.position.x,
      y: n.position.y,
      data: n.data ?? {},
    });
  }

  for (const e of edges) {
    const tgtNode = nodeById.get(e.target);
    let targetPort = e.targetHandle ?? "in";
    if (
      tgtNode != null &&
      (tgtNode.type === "ftaAnd" || tgtNode.type === "ftaOr") &&
      !e.targetHandle
    ) {
      const ord = targetOrder.get(e.target) ?? 0;
      targetOrder.set(e.target, ord + 1);
      const gatePorts = ["in1", "in2", "in3", "in4", "in5"];
      targetPort = gatePorts[Math.min(ord, gatePorts.length - 1)] ?? "in5";
    }
    cells.push({
      id: e.id,
      shape: "edge",
      source: { cell: e.source, port: e.sourceHandle ?? "out" },
      target: { cell: e.target, port: targetPort },
      router: { name: "manhattan", args: { padding: 8 } },
      connector: { name: "normal" },
      attrs: { line: { strokeWidth: 1 } },
    });
  }

  return { payload: { cells, ramMeta: { mode, topOrRoot } }, topOrRoot };
}

function parseStoredToX6(
  mode: LabMode,
  graphJson: string,
  fallbackTopOrRoot: string,
): { payload: X6GraphPayload; topOrRoot: string } {
  try {
    const parsed = JSON.parse(graphJson) as Record<string, unknown>;
    if (Array.isArray(parsed["cells"])) {
      const payload = parsed as unknown as X6GraphPayload;
      const topOrRoot = payload.ramMeta?.topOrRoot ?? fallbackTopOrRoot;
      return { payload, topOrRoot };
    }
  } catch {
    // fallback to legacy decode
  }
  return parseLegacyToX6(mode, graphJson);
}

function x6ToLegacyGraphJson(mode: LabMode, graphData: X6GraphPayload, topOrRoot: string): string {
  const nodes: FlowNodeLite[] = [];
  const edges: FlowEdgeLite[] = [];
  for (const cell of graphData.cells) {
    const id = String(cell["id"] ?? "");
    const shape = String(cell["shape"] ?? "");
    if (!id || !shape) {
      continue;
    }
    if (shape === "edge") {
      const src = cell["source"];
      const tgt = cell["target"];
      const source =
        typeof src === "string"
          ? src
          : src && typeof src === "object"
            ? String((src as { cell?: unknown }).cell ?? "")
            : "";
      const target =
        typeof tgt === "string"
          ? tgt
          : tgt && typeof tgt === "object"
            ? String((tgt as { cell?: unknown }).cell ?? "")
            : "";
      if (!source || !target) {
        continue;
      }
      const sourceHandle =
        src && typeof src === "object"
          ? ((src as { port?: unknown }).port as string | undefined)
          : undefined;
      const targetHandle =
        tgt && typeof tgt === "object"
          ? ((tgt as { port?: unknown }).port as string | undefined)
          : undefined;
      edges.push({
        id,
        source,
        target,
        ...(sourceHandle != null ? { sourceHandle } : {}),
        ...(targetHandle != null ? { targetHandle } : {}),
      });
      continue;
    }
    const type = shapeToFlowType[shape];
    if (!type) {
      continue;
    }
    const data = (cell["data"] ?? {}) as Record<string, unknown>;
    nodes.push({
      id,
      type,
      position: { x: readCellX(cell), y: readCellY(cell) },
      data,
    });
  }
  if (mode === "fta") {
    return flowToFtaJson(nodes as never, edges as never, topOrRoot.trim() || "top");
  }
  return flowToRbdJson(nodes as never, edges as never, topOrRoot.trim() || "root");
}

function x6NativeGraphJson(mode: LabMode, graphData: X6GraphPayload, topOrRoot: string): string {
  return JSON.stringify({
    ...graphData,
    ramMeta: {
      mode,
      topOrRoot: topOrRoot.trim() || (mode === "fta" ? "top" : "root"),
    },
  });
}

function AndNodeBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const label = String(d["label"] ?? "");
  return (
    <div className="rams-x6-node">
      <svg viewBox="0 0 88 56" className="rams-x6-symbol" aria-hidden>
        <path
          d="M14 10L40 10A18 18 0 0 1 58 28A18 18 0 0 1 40 46L14 46L14 10z"
          className="rams-x6-stroke"
        />
        <path d="M6 21h8M6 35h8" className="rams-x6-stroke rams-x6-round" />
      </svg>
      <div className="rams-x6-k">AND {label}</div>
      <div className="rams-x6-meta">
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>MTTR</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

function OrNodeBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const label = String(d["label"] ?? "");
  return (
    <div className="rams-x6-node">
      <svg viewBox="0 0 88 56" className="rams-x6-symbol" aria-hidden>
        <path
          d="M12 10Q34 10 46 28Q34 46 12 46M12 10L12 46M4 20H12M4 36H12M46 28H74"
          className="rams-x6-stroke rams-x6-round"
        />
      </svg>
      <div className="rams-x6-k">OR {label}</div>
      <div className="rams-x6-meta">
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>MTTR</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

function BeNodeBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const label = String(d["label"] ?? "");
  return (
    <div className="rams-x6-node">
      <svg viewBox="0 0 88 72" className="rams-x6-symbol" aria-hidden>
        <path d="M44 4L44 18" className="rams-x6-stroke rams-x6-round" />
        <circle cx="44" cy="38" r="14" className="rams-x6-stroke" />
      </svg>
      <div className="rams-x6-k">BE {label}</div>
      <div className="rams-x6-meta">
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>MTTR</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

function RbdBlockBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const r = typeof d["r"] === "number" ? d["r"] : Number(d["r"]) || 0.99;
  const label = String(d["label"] ?? "BLK");
  return (
    <div className="rams-x6-block">
      <div className="rams-x6-block-h">BLK</div>
      <div className="rams-x6-block-label" title={label}>
        {label}
      </div>
      <div className="rams-x6-block-r">R = {r.toFixed(4)}</div>
      <div className="rams-x6-meta rams-x6-meta-in">
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>MTTR</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

function RbdSeriesBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const label = String(d["label"] ?? "");
  return (
    <div className="rams-x6-node">
      <svg viewBox="0 0 88 56" className="rams-x6-symbol" aria-hidden>
        <rect x="10" y="18" width="18" height="20" className="rams-x6-stroke" />
        <rect x="34" y="18" width="18" height="20" className="rams-x6-stroke" />
        <rect x="58" y="18" width="18" height="20" className="rams-x6-stroke" />
        <path d="M4 28h6M82 28h-6" className="rams-x6-stroke" />
      </svg>
      <div className="rams-x6-k">SER {label}</div>
      <div className="rams-x6-meta">
        <span>R</span>
        <span>ΠRi</span>
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

function RbdParallelBody({ node }: { node: { getData: () => Record<string, unknown> } }) {
  const d = node.getData();
  const label = String(d["label"] ?? "");
  return (
    <div className="rams-x6-node">
      <svg viewBox="0 0 88 56" className="rams-x6-symbol" aria-hidden>
        <path d="M12 10v36M12 28h12l6-10v20l6-10h12" className="rams-x6-stroke" />
        <path d="M4 20h8M4 36h8M66 20h8M66 36h8" className="rams-x6-stroke" />
      </svg>
      <div className="rams-x6-k">PAR {label}</div>
      <div className="rams-x6-meta">
        <span>R</span>
        <span>1-Π(1-Ri)</span>
        <span>λ</span>
        <span>—</span>
        <span>τ</span>
        <span>—</span>
        <span>U</span>
        <span>—</span>
      </div>
    </div>
  );
}

let x6ShapesReady = false;
const ReactShapeProvider = Portal.getProvider();
function ensureX6Shapes() {
  if (x6ShapesReady) {
    return;
  }

  const common = {
    inherit: "react-shape",
    attrs: { body: { fill: "transparent", stroke: "transparent" } },
  } as const;
  const ports = {
    groups: {
      p: {
        position: "absolute",
        attrs: { circle: { r: 3, magnet: true, stroke: "transparent", fill: "transparent" } },
      },
    },
  };

  register({
    ...common,
    shape: SHAPE.ftaAnd,
    width: 72,
    height: 68,
    component: AndNodeBody,
    ports: {
      ...ports,
      items: [
        { id: "in1", group: "p", args: { x: 12, y: 18 } },
        { id: "in2", group: "p", args: { x: 12, y: 30 } },
        { id: "in3", group: "p", args: { x: 12, y: 24 } },
        { id: "in4", group: "p", args: { x: 12, y: 36 } },
        { id: "in5", group: "p", args: { x: 12, y: 42 } },
        { id: "out", group: "p", args: { x: 49, y: 24 } },
      ],
    },
  } as never);

  register({
    ...common,
    shape: SHAPE.ftaOr,
    width: 72,
    height: 68,
    component: OrNodeBody,
    ports: {
      ...ports,
      items: [
        { id: "in1", group: "p", args: { x: 10, y: 18 } },
        { id: "in2", group: "p", args: { x: 10, y: 30 } },
        { id: "in3", group: "p", args: { x: 10, y: 24 } },
        { id: "in4", group: "p", args: { x: 10, y: 36 } },
        { id: "in5", group: "p", args: { x: 10, y: 42 } },
        { id: "out", group: "p", args: { x: 59, y: 24 } },
      ],
    },
  } as never);

  register({
    ...common,
    shape: SHAPE.ftaBe,
    width: 72,
    height: 78,
    component: BeNodeBody,
    ports: {
      ...ports,
      items: [{ id: "out", group: "p", args: { x: 36, y: 6 } }],
    },
  } as never);

  register({
    ...common,
    shape: SHAPE.rbdBlock,
    width: 80,
    height: 56,
    component: RbdBlockBody,
    ports: {
      ...ports,
      items: [
        { id: "in", group: "p", args: { x: 0, y: 28 } },
        { id: "out", group: "p", args: { x: 80, y: 28 } },
      ],
    },
  } as never);

  register({
    ...common,
    shape: SHAPE.rbdSeries,
    width: 72,
    height: 66,
    component: RbdSeriesBody,
    ports: {
      ...ports,
      items: [
        { id: "in", group: "p", args: { x: 6, y: 20 } },
        { id: "out", group: "p", args: { x: 66, y: 20 } },
      ],
    },
  } as never);

  register({
    ...common,
    shape: SHAPE.rbdParallel,
    width: 72,
    height: 66,
    component: RbdParallelBody,
    ports: {
      ...ports,
      items: [
        { id: "in", group: "p", args: { x: 6, y: 20 } },
        { id: "out", group: "p", args: { x: 66, y: 20 } },
      ],
    },
  } as never);

  x6ShapesReady = true;
}

function VisualLabCanvas({ mode, equipmentId }: { mode: LabMode; equipmentId: number }) {
  const { t } = useTranslation("reliability");
  const { theme } = useTheme();
  const { can } = usePermissions();
  const canManage = can("ram.manage");
  const canAnalyze = can("ram.analyze");

  const graphHostRef = useRef<HTMLDivElement | null>(null);
  const stencilHostRef = useRef<HTMLDivElement | null>(null);
  const graphRef = useRef<Graph | null>(null);

  const [ftaRows, setFtaRows] = useState<FtaModel[]>([]);
  const [rbdRows, setRbdRows] = useState<RbdModel[]>([]);
  const [selFta, setSelFta] = useState<FtaModel | null>(null);
  const [selRbd, setSelRbd] = useState<RbdModel | null>(null);
  const [topOrRoot, setTopOrRoot] = useState("top");
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);

  const loadList = useCallback(async () => {
    setErr(null);
    try {
      if (mode === "fta") {
        const rows = await listFtaModels({ equipment_id: equipmentId, limit: 80 });
        setFtaRows(rows);
      } else {
        const rows = await listRbdModels({ equipment_id: equipmentId, limit: 80 });
        setRbdRows(rows);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setErr(`${t("lab.loadError")}: ${msg}`);
      setFtaRows([]);
      setRbdRows([]);
    }
  }, [equipmentId, mode, t]);

  useEffect(() => {
    void loadList();
  }, [loadList]);

  const applyGraph = useCallback(
    (json: string, top: string) => {
      const graph = graphRef.current;
      if (graph == null) {
        return;
      }
      const decoded = parseStoredToX6(mode, json, top);
      graph.fromJSON(decoded.payload as never);
      // Keep loaded graphs centered in the viewport (prevents "floating top-left" effect).
      requestAnimationFrame(() => {
        graph.centerContent();
      });
      setTopOrRoot(decoded.topOrRoot);
    },
    [mode],
  );

  useEffect(() => {
    ensureX6Shapes();
    const host = graphHostRef.current;
    if (host == null) {
      return;
    }

    const graph = new Graph({
      container: host,
      background: { color: theme === "dark" ? "#0f172a" : "#e2e8f0" },
      grid: {
        visible: true,
        type: "mesh",
        args: {
          color: theme === "dark" ? "rgba(148,163,184,0.18)" : "rgba(71,85,105,0.24)",
          thickness: 1,
        },
        size: 20,
      },
      panning: true,
      mousewheel: {
        enabled: true,
        minScale: 0.2,
        maxScale: 3,
        modifiers: ["ctrl", "meta"],
      },
      interacting: () => canManage,
      connecting: {
        snap: true,
        allowBlank: false,
        allowNode: false,
        allowLoop: false,
        highlight: true,
        connector: { name: "normal" },
        router: { name: "manhattan", args: { padding: 8 } },
        createEdge() {
          return this.createEdge({
            attrs: { line: { stroke: "#475569", strokeWidth: 1 } },
            connector: { name: "normal" },
            router: { name: "manhattan", args: { padding: 8 } },
            zIndex: 0,
          });
        },
        validateConnection({ sourceCell, targetCell, sourcePort, targetPort }) {
          if (!sourceCell || !targetCell || sourceCell.id === targetCell.id) {
            return false;
          }
          if (!sourcePort || !targetPort) {
            return false;
          }
          return true;
        },
      },
      highlighting: {
        magnetAvailable: {
          name: "stroke",
          args: {
            attrs: { stroke: "#3b82f6", strokeWidth: 1 },
            padding: 4,
          },
        },
      },
    });

    const edgeVerticesTool = [
      {
        name: "vertices",
        args: {
          precision: 0,
          attrs: {
            fill: "#475569",
            stroke: "#0f172a",
            "stroke-width": 1,
            r: 3,
          },
        },
      },
    ];

    const applyRotation = (deltaDeg: number) => {
      const selected = graph.getSelectedCells();
      selected.forEach((cell) => {
        if ((cell as { isNode?: () => boolean }).isNode?.()) {
          const n = cell as unknown as {
            getAngle?: () => number;
            rotate: (angle: number, opts?: { absolute?: boolean }) => void;
          };
          const current = n.getAngle?.() ?? 0;
          n.rotate(current + deltaDeg, { absolute: true });
        }
      });
    };

    const onRotateKey = (evt: KeyboardEvent) => {
      if (evt.key.toLowerCase() !== "r") {
        return;
      }
      if (evt.repeat) {
        return;
      }
      applyRotation(evt.shiftKey ? -90 : 90);
      evt.preventDefault();
    };
    window.addEventListener("keydown", onRotateKey);

    graph.on("edge:mouseenter", ({ edge }) => {
      edge.addTools(edgeVerticesTool as never);
    });
    graph.on("edge:mouseleave", ({ edge }) => {
      edge.removeTools();
    });
    graph.on("edge:click", ({ edge }) => {
      edge.addTools(edgeVerticesTool as never);
    });
    // Built-in vertices tool supports add/drag; keep an explicit remove on vertex dblclick.
    graph.on(
      "edge:vertex:dblclick",
      ({ edge, index }: { edge: { removeVertexAt?: (idx: number) => void }; index: unknown }) => {
        if (typeof index === "number" && Number.isInteger(index)) {
          edge.removeVertexAt?.(index);
        }
      },
    );

    graph.use(
      new Selection({
        enabled: true,
        multiple: true,
        rubberNode: false,
        rubberEdge: false,
        showNodeSelectionBox: false,
        showEdgeSelectionBox: false,
      }),
    );
    graph.use(new Snapline({ enabled: true }));
    graphRef.current = graph;

    const stencilHost = stencilHostRef.current;
    if (stencilHost != null) {
      stencilHost.innerHTML = "";
      const stencil = new Stencil({
        target: graph,
        title: t("lab.palette.toolbarLabel"),
        stencilGraphWidth: 180,
        stencilGraphHeight: 0,
        groups: [{ name: "symbols", title: mode === "fta" ? "FTA" : "RBD" }],
        collapsable: false,
      });
      stencilHost.appendChild(stencil.container);
      const symbols =
        mode === "fta"
          ? [
              graph.createNode({ shape: SHAPE.ftaAnd, data: { label: "AND", tau_h: 8760 } }),
              graph.createNode({ shape: SHAPE.ftaOr, data: { label: "OR", tau_h: 8760 } }),
              graph.createNode({ shape: SHAPE.ftaBe, data: { label: "BE", p: 0.01, tau_h: 8760 } }),
            ]
          : [
              graph.createNode({
                shape: SHAPE.rbdBlock,
                data: { label: "BLK", r: 0.99, tau_h: 8760 },
              }),
              graph.createNode({ shape: SHAPE.rbdSeries, data: { label: "SER" } }),
              graph.createNode({ shape: SHAPE.rbdParallel, data: { label: "PAR" } }),
            ];
      stencil.load(symbols, "symbols");
    }

    setSelFta(null);
    setSelRbd(null);
    setResult(null);
    if (mode === "fta") {
      applyGraph(DEFAULT_FTA, "top");
    } else {
      applyGraph(DEFAULT_RBD, "root");
    }

    return () => {
      window.removeEventListener("keydown", onRotateKey);
      graph.dispose();
      graphRef.current = null;
    };
  }, [mode, theme, canManage, applyGraph, t]);

  const onSelectFta = (m: FtaModel) => {
    setSelFta(m);
    setSelRbd(null);
    applyGraph(m.graph_json, "top");
    setResult(m.result_json);
  };

  const onSelectRbd = (m: RbdModel) => {
    setSelRbd(m);
    setSelFta(null);
    applyGraph(m.graph_json, "root");
    setResult(m.result_json);
  };

  const onNew = async () => {
    if (!canManage) {
      return;
    }
    setErr(null);
    try {
      if (mode === "fta") {
        const m = await createFtaModel({
          equipment_id: equipmentId,
          title: t("lab.placeholderTitle"),
          graph_json: DEFAULT_FTA,
        });
        setSelFta(m);
        setSelRbd(null);
        applyGraph(m.graph_json, "top");
        await loadList();
      } else {
        const m = await createRbdModel({
          equipment_id: equipmentId,
          title: t("lab.placeholderTitle"),
          graph_json: DEFAULT_RBD,
        });
        setSelRbd(m);
        setSelFta(null);
        applyGraph(m.graph_json, "root");
        await loadList();
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const onSave = async () => {
    if (!canManage) {
      return;
    }
    const graph = graphRef.current;
    if (graph == null) {
      return;
    }
    setErr(null);
    try {
      const payload = graph.toJSON() as unknown as X6GraphPayload;
      const graph_json = x6NativeGraphJson(mode, payload, topOrRoot);
      if (mode === "fta") {
        if (selFta == null) {
          setErr(t("lab.needModel"));
          return;
        }
        const m = await updateFtaModel({
          id: selFta.id,
          expected_row_version: selFta.row_version,
          graph_json,
        });
        setSelFta(m);
        await loadList();
      } else {
        if (selRbd == null) {
          setErr(t("lab.needModel"));
          return;
        }
        const m = await updateRbdModel({
          id: selRbd.id,
          expected_row_version: selRbd.row_version,
          graph_json,
        });
        setSelRbd(m);
        await loadList();
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const onEval = async () => {
    if (!canAnalyze) {
      return;
    }
    const graph = graphRef.current;
    if (graph == null) {
      return;
    }
    setErr(null);
    try {
      const payload = graph.toJSON() as unknown as X6GraphPayload;
      const graph_json_native = x6NativeGraphJson(mode, payload, topOrRoot);
      const graph_json_legacy = x6ToLegacyGraphJson(mode, payload, topOrRoot);

      if (mode === "fta") {
        if (selFta == null) {
          setErr(t("lab.needModel"));
          return;
        }
        let m = selFta;
        if (canManage) {
          m = await updateFtaModel({
            id: selFta.id,
            expected_row_version: selFta.row_version,
            graph_json: graph_json_legacy,
          });
        }
        m = await evaluateFtaModel(m.id);
        if (canManage) {
          m = await updateFtaModel({
            id: m.id,
            expected_row_version: m.row_version,
            graph_json: graph_json_native,
          });
        }
        setSelFta(m);
        setResult(m.result_json);
      } else {
        if (selRbd == null) {
          setErr(t("lab.needModel"));
          return;
        }
        let m = selRbd;
        if (canManage) {
          m = await updateRbdModel({
            id: selRbd.id,
            expected_row_version: selRbd.row_version,
            graph_json: graph_json_legacy,
          });
        }
        m = await evaluateRbdModel(m.id);
        if (canManage) {
          m = await updateRbdModel({
            id: m.id,
            expected_row_version: m.row_version,
            graph_json: graph_json_native,
          });
        }
        setSelRbd(m);
        setResult(m.result_json);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const onDelete = async () => {
    if (!canManage) {
      return;
    }
    setErr(null);
    try {
      if (mode === "fta" && selFta) {
        await deleteFtaModel(selFta.id);
        setSelFta(null);
      } else if (mode === "rbd" && selRbd) {
        await deleteRbdModel(selRbd.id);
        setSelRbd(null);
      }
      applyGraph(mode === "fta" ? DEFAULT_FTA : DEFAULT_RBD, mode === "fta" ? "top" : "root");
      await loadList();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="flex h-[min(720px,calc(100vh-12rem))] min-h-[480px] flex-col gap-2 p-4">
      {err ? <p className="text-xs text-text-danger">{err}</p> : null}
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="font-mono text-[11px] text-text-muted" title={t("lab.equipHint")}>
          #{equipmentId}
        </span>
        <button
          type="button"
          className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs hover:bg-surface-3"
          onClick={() => void loadList()}
        >
          {t("lab.refresh")}
        </button>
        <select
          className="max-w-[14rem] rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs"
          value={mode === "fta" ? (selFta?.id ?? "") : (selRbd?.id ?? "")}
          onChange={(e) => {
            const id = Number(e.target.value);
            if (Number.isNaN(id)) {
              return;
            }
            if (mode === "fta") {
              const m = ftaRows.find((r) => r.id === id);
              if (m) {
                onSelectFta(m);
              }
            } else {
              const m = rbdRows.find((r) => r.id === id);
              if (m) {
                onSelectRbd(m);
              }
            }
          }}
        >
          <option value="">{t("lab.model")}</option>
          {mode === "fta"
            ? ftaRows.map((r) => (
                <option key={r.id} value={r.id}>
                  #{r.id} {r.title}
                </option>
              ))
            : rbdRows.map((r) => (
                <option key={r.id} value={r.id}>
                  #{r.id} {r.title}
                </option>
              ))}
        </select>
        <button
          type="button"
          disabled={!canManage}
          className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
          onClick={() => void onNew()}
        >
          {t("lab.newModel")}
        </button>
        <button
          type="button"
          disabled={!canManage}
          className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
          onClick={() => void onSave()}
        >
          {t("lab.save")}
        </button>
        <button
          type="button"
          disabled={!canAnalyze}
          className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
          onClick={() => void onEval()}
        >
          {t("lab.evaluate")}
        </button>
        <button
          type="button"
          disabled={!canManage}
          className="rounded-md border border-surface-border px-2 py-1 text-xs text-text-danger disabled:opacity-40"
          onClick={() => void onDelete()}
        >
          {t("lab.delete")}
        </button>
        <label className="flex items-center gap-1 text-text-muted">
          {t("lab.topRoot")}
          <input
            className="w-24 rounded-md border border-surface-border bg-surface-2 px-1.5 py-0.5 font-mono text-sm"
            value={topOrRoot}
            onChange={(e) => setTopOrRoot(e.target.value)}
          />
        </label>
      </div>

      <div className="flex min-h-0 w-full flex-1 flex-col gap-2 sm:flex-row">
        <div className="hidden h-full w-[220px] shrink-0 sm:block">
          <div
            ref={stencilHostRef}
            className="rams-x6-stencil-host relative h-full w-full overflow-auto rounded-sm border border-slate-400/70 bg-white/90 p-1 dark:border-slate-600 dark:bg-slate-900/90"
          />
        </div>
        <div
          className={cn(
            "relative flex h-full min-h-0 min-w-0 w-full flex-1 flex-col overflow-hidden rounded-sm border border-slate-400/70 bg-slate-200/90 shadow-none",
            "dark:border-slate-600 dark:bg-slate-950",
            "min-h-[min(520px,calc(100vh-16rem))]",
          )}
        >
          <div className="shrink-0 border-b border-slate-400/60 bg-white/90 px-2 py-1 text-[11px] text-slate-600 dark:border-slate-600 dark:bg-slate-900/90 dark:text-slate-300 sm:hidden">
            Utilisez la bibliothèque de symboles en mode desktop (stencil X6).
          </div>
          <div className="relative h-full w-full min-h-0 flex-1 overflow-hidden">
            <div ref={graphHostRef} className="rams-x6-canvas relative h-full w-full min-h-0" />
            {((mode === "fta" && ftaRows.length === 0 && selFta == null) ||
              (mode === "rbd" && rbdRows.length === 0 && selRbd == null)) &&
            !err ? (
              <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
                <div className="rounded-sm border border-slate-400/70 bg-white/90 px-3 py-2 text-[11px] font-mono text-slate-700 dark:border-slate-600 dark:bg-slate-900/90 dark:text-slate-200">
                  {mode === "fta"
                    ? "FTA empty canvas — create a model to begin."
                    : "RBD empty canvas — create a model to begin."}
                </div>
              </div>
            ) : null}
          </div>
          <div className="pointer-events-none absolute right-3 top-3 rounded border border-slate-400/60 bg-white/90 px-2 py-1 text-[10px] font-mono text-slate-600 dark:border-slate-600 dark:bg-slate-900/90 dark:text-slate-300">
            X6 • Manhattan • Snapline • Ports
          </div>
        </div>
      </div>

      <ReactShapeProvider />
      {result ? (
        <div>
          <p className="mb-1 text-[11px] font-medium text-text-muted">{t("lab.result")}</p>
          <pre className="max-h-32 overflow-auto rounded-lg border border-surface-border bg-surface-2 p-2 font-mono text-[10px] text-text-secondary">
            {result}
          </pre>
        </div>
      ) : null}
    </div>
  );
}

export function ReliabilityVisualLabPage() {
  const { t } = useTranslation("reliability");
  const [mode, setMode] = useState<LabMode>("fta");
  const equipmentId = useRequiredRamsEquipmentId();

  return (
    <div className="text-sm text-text-primary">
      <div className="border-b border-surface-border px-4 py-3">
        <h2 className="text-lg font-semibold text-text-primary">{t("lab.title")}</h2>
        <div className="mt-2 flex gap-2">
          <button
            type="button"
            className={cn(
              "rounded-lg border px-3 py-1.5 text-xs",
              mode === "fta"
                ? "border-primary/35 bg-surface-2 text-text-primary"
                : "border-surface-border text-text-muted hover:border-surface-border hover:bg-surface-2",
            )}
            onClick={() => setMode("fta")}
          >
            {t("lab.modeFta")}
          </button>
          <button
            type="button"
            className={cn(
              "rounded-lg border px-3 py-1.5 text-xs",
              mode === "rbd"
                ? "border-primary/35 bg-surface-2 text-text-primary"
                : "border-surface-border text-text-muted hover:border-surface-border hover:bg-surface-2",
            )}
            onClick={() => setMode("rbd")}
          >
            {t("lab.modeRbd")}
          </button>
        </div>
      </div>
      <VisualLabCanvas key={`${mode}-${equipmentId}`} mode={mode} equipmentId={equipmentId} />
    </div>
  );
}
