import type { Edge, Node } from "@xyflow/react";

export type FtaNodeSpec =
  | { kind: "basic"; p: number }
  | { kind: "and"; inputs: string[] }
  | { kind: "or"; inputs: string[] };

export type RbdNodeSpec =
  | { kind: "block"; r: number }
  | { kind: "series"; children: string[] }
  | { kind: "parallel"; children: string[] };

function place(idx: number) {
  const col = idx % 5;
  const row = Math.floor(idx / 5);
  return { x: col * 120, y: row * 78 };
}

const stepEdge = { type: "ramsStep" as const, style: { strokeWidth: 1 } };

function ftaIncomingOrdered(edges: Edge[], targetId: string): Edge[] {
  return edges
    .filter((e) => e.target === targetId)
    .sort((a, b) => {
      const rank = (h: string | null | undefined) =>
        h === "in1" ? 0 : h === "in2" ? 1 : h === "in3" ? 2 : 50;
      return rank(a.targetHandle) - rank(b.targetHandle);
    });
}

/** Build React Flow nodes/edges from persisted FTA graph_json (backend shape). */
export function ftaJsonToFlow(graphJson: string): { nodes: Node[]; edges: Edge[]; topId: string } {
  let parsed: { top_id?: string; nodes?: Record<string, FtaNodeSpec> };
  try {
    parsed = JSON.parse(graphJson) as { top_id?: string; nodes?: Record<string, FtaNodeSpec> };
  } catch {
    return { nodes: [], edges: [], topId: "top" };
  }
  const topId = parsed.top_id ?? "top";
  const raw = parsed.nodes ?? {};
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  let i = 0;
  for (const [id, spec] of Object.entries(raw)) {
    const pos = place(i);
    i += 1;
    if (!spec || typeof spec !== "object" || !("kind" in spec)) {
      continue;
    }
    if (spec.kind === "basic") {
      const p = typeof spec.p === "number" ? spec.p : 0.01;
      nodes.push({
        id,
        type: "ftaBe",
        position: pos,
        data: { label: id, p },
      });
    } else if (spec.kind === "and") {
      nodes.push({ id, type: "ftaAnd", position: pos, data: { label: id } });
      (spec.inputs ?? []).forEach((inp, idx) => {
        edges.push({
          id: `e-${inp}-${id}`,
          source: inp,
          target: id,
          sourceHandle: "out",
          targetHandle: idx === 0 ? "in1" : "in2",
          ...stepEdge,
        });
      });
    } else if (spec.kind === "or") {
      nodes.push({ id, type: "ftaOr", position: pos, data: { label: id } });
      (spec.inputs ?? []).forEach((inp, idx) => {
        edges.push({
          id: `e-${inp}-${id}`,
          source: inp,
          target: id,
          sourceHandle: "out",
          targetHandle: idx === 0 ? "in1" : "in2",
          ...stepEdge,
        });
      });
    }
  }
  return { nodes, edges, topId };
}

export function flowToFtaJson(nodes: Node[], edges: Edge[], topId: string): string {
  const map: Record<string, FtaNodeSpec> = {};
  for (const n of nodes) {
    if (n.type === "ftaBe") {
      const d = (n.data ?? {}) as Record<string, unknown>;
      const p = typeof d["p"] === "number" ? d["p"] : Number(d["p"]) || 0.01;
      map[n.id] = { kind: "basic", p };
    } else if (n.type === "ftaAnd") {
      const inputs = ftaIncomingOrdered(edges, n.id).map((e) => e.source);
      map[n.id] = { kind: "and", inputs };
    } else if (n.type === "ftaOr") {
      const inputs = ftaIncomingOrdered(edges, n.id).map((e) => e.source);
      map[n.id] = { kind: "or", inputs };
    }
  }
  return JSON.stringify({ spec_version: 1, top_id: topId, nodes: map });
}

/** Build React Flow from RBD graph_json. */
export function rbdJsonToFlow(graphJson: string): { nodes: Node[]; edges: Edge[]; rootId: string } {
  let parsed: { root_id?: string; nodes?: Record<string, RbdNodeSpec> };
  try {
    parsed = JSON.parse(graphJson) as { root_id?: string; nodes?: Record<string, RbdNodeSpec> };
  } catch {
    return { nodes: [], edges: [], rootId: "root" };
  }
  const rootId = parsed.root_id ?? "root";
  const raw = parsed.nodes ?? {};
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  let i = 0;
  for (const [id, spec] of Object.entries(raw)) {
    const pos = place(i);
    i += 1;
    if (!spec || typeof spec !== "object" || !("kind" in spec)) {
      continue;
    }
    if (spec.kind === "block") {
      const r = typeof spec.r === "number" ? spec.r : 0.99;
      nodes.push({ id, type: "rbdBlock", position: pos, data: { label: id, r } });
    } else if (spec.kind === "series") {
      nodes.push({ id, type: "rbdSeries", position: pos, data: { label: id } });
      for (const ch of spec.children ?? []) {
        edges.push({
          id: `e-${ch}-${id}`,
          source: ch,
          target: id,
          sourceHandle: "out",
          targetHandle: "in",
          ...stepEdge,
        });
      }
    } else if (spec.kind === "parallel") {
      nodes.push({ id, type: "rbdParallel", position: pos, data: { label: id } });
      for (const ch of spec.children ?? []) {
        edges.push({
          id: `e-${ch}-${id}`,
          source: ch,
          target: id,
          sourceHandle: "out",
          targetHandle: "in",
          ...stepEdge,
        });
      }
    }
  }
  return { nodes, edges, rootId };
}

export function flowToRbdJson(nodes: Node[], edges: Edge[], rootId: string): string {
  const map: Record<string, RbdNodeSpec> = {};
  for (const n of nodes) {
    if (n.type === "rbdBlock") {
      const d = (n.data ?? {}) as Record<string, unknown>;
      const r = typeof d["r"] === "number" ? d["r"] : Number(d["r"]) || 0.99;
      map[n.id] = { kind: "block", r };
    } else if (n.type === "rbdSeries") {
      const children = edges
        .filter((e) => e.target === n.id)
        .sort((a, b) => a.source.localeCompare(b.source))
        .map((e) => e.source);
      map[n.id] = { kind: "series", children };
    } else if (n.type === "rbdParallel") {
      const children = edges
        .filter((e) => e.target === n.id)
        .sort((a, b) => a.source.localeCompare(b.source))
        .map((e) => e.source);
      map[n.id] = { kind: "parallel", children };
    }
  }
  return JSON.stringify({ spec_version: 1, root_id: rootId, nodes: map });
}
