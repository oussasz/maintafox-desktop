import { Handle, type NodeProps, Position } from "@xyflow/react";
import type { CSSProperties, ReactNode } from "react";

import { cn } from "@/lib/utils";

import { formatSci, ftaBasicEventDisplay, rbdBlockDisplay } from "./ramsLabDisplayMetrics";
import {
  IconAndGate,
  IconBasicEvent,
  IconOrGate,
  IconRbdParallel,
  IconRbdSeries,
} from "./ramsLabSymbols";
import {
  BE_SYM_PX,
  GATE_SYM_PX,
  RBD_AUX_SYM_PX,
  andHandlePct,
  beHandlePct,
  orHandlePct,
  rbdAuxHandlePct,
} from "./ramsPortGeometry";

/** Pins: invisible at rest; `.rams-lab-flow` CSS highlights valid targets while dragging. */
const portCls =
  "rams-lab-port nodrag !h-2 !w-2 !min-h-0 !min-w-0 !border-0 !bg-transparent !opacity-0";

function portStyle(p: { left: string; top: string }): CSSProperties {
  return {
    position: "absolute",
    left: p.left,
    top: p.top,
    transform: "translate(-50%, -50%)",
  };
}

const meta = "font-mono text-[7px] leading-[1.2] tabular-nums tracking-wide";
const metaLabel = cn(meta, "text-slate-600 dark:text-slate-400");
const metaVal = cn(meta, "text-slate-800 dark:text-slate-200");

function ParamGrid({
  rows,
  className,
}: {
  rows: { label: string; value: string }[];
  className?: string;
}) {
  return (
    <div
      className={cn(
        "rams-lab-annotation grid w-full grid-cols-[minmax(1.5rem,auto)_1fr] gap-x-1 gap-y-px border-t border-dashed border-slate-500/90 pt-1 dark:border-slate-500",
        className,
      )}
    >
      {rows.map((r) => (
        <div key={r.label} className="contents">
          <span className={metaLabel}>{r.label}</span>
          <span className={cn(metaVal, "min-w-0 break-words text-right")}>{r.value}</span>
        </div>
      ))}
    </div>
  );
}

function GateBody({
  gate,
  symbol,
  footer,
}: {
  gate: "and" | "or";
  symbol: ReactNode;
  footer: ReactNode;
}) {
  const h = gate === "and" ? andHandlePct() : orHandlePct();
  return (
    <div className="flex w-min flex-col items-center bg-transparent">
      <div className="relative shrink-0" style={{ width: GATE_SYM_PX.w, height: GATE_SYM_PX.h }}>
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          {symbol}
        </div>
        <Handle
          type="target"
          position={Position.Left}
          id="in1"
          className={portCls}
          style={portStyle(h.inTop)}
        />
        <Handle
          type="target"
          position={Position.Left}
          id="in2"
          className={portCls}
          style={portStyle(h.inBot)}
        />
        <Handle
          type="source"
          position={Position.Right}
          id="out"
          className={portCls}
          style={portStyle(h.out)}
        />
      </div>
      <div className="mt-1 w-full min-w-[7rem] max-w-[10rem]">{footer}</div>
    </div>
  );
}

function BeBody({ symbol, footer }: { symbol: ReactNode; footer: ReactNode }) {
  const h = beHandlePct();
  return (
    <div className="flex w-min flex-col items-center bg-transparent">
      <div className="relative shrink-0" style={{ width: BE_SYM_PX.w, height: BE_SYM_PX.h }}>
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          {symbol}
        </div>
        <Handle
          type="source"
          position={Position.Top}
          id="out"
          className={portCls}
          style={portStyle(h.out)}
        />
      </div>
      <div className="mt-1 w-full min-w-[7rem] max-w-[10rem]">{footer}</div>
    </div>
  );
}

function RbdAuxBody({
  which,
  symbol,
  footer,
}: {
  which: "series" | "parallel";
  symbol: ReactNode;
  footer: ReactNode;
}) {
  const h = rbdAuxHandlePct(which);
  return (
    <div className="flex w-min flex-col items-center bg-transparent">
      <div
        className="relative shrink-0"
        style={{ width: RBD_AUX_SYM_PX.w, height: RBD_AUX_SYM_PX.h }}
      >
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          {symbol}
        </div>
        <Handle
          type="target"
          position={Position.Left}
          id="in"
          className={portCls}
          style={portStyle(h.in)}
        />
        <Handle
          type="source"
          position={Position.Right}
          id="out"
          className={portCls}
          style={portStyle(h.out)}
        />
      </div>
      <div className="mt-1 w-full min-w-[7rem] max-w-[10rem]">{footer}</div>
    </div>
  );
}

const gateIconCls = "h-full w-full max-h-full max-w-full text-slate-900 dark:text-slate-100";

export function FtaAndNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const footer = (
    <>
      <div className="text-center font-mono text-[7px] font-bold uppercase leading-none tracking-[0.12em] text-slate-600 dark:text-slate-400">
        AND
      </div>
      {d["label"] ? (
        <div className="truncate px-0.5 text-center font-mono text-[8px] leading-tight text-slate-600 dark:text-slate-500">
          {String(d["label"])}
        </div>
      ) : null}
      <ParamGrid
        rows={[
          { label: "∩", value: "≥2" },
          { label: "λ", value: "—" },
          { label: "τ", value: "—" },
          { label: "MTTR", value: "—" },
          { label: "U", value: "—" },
        ]}
      />
    </>
  );
  return <GateBody gate="and" symbol={<IconAndGate className={gateIconCls} />} footer={footer} />;
}

export function FtaOrNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const footer = (
    <>
      <div className="text-center font-mono text-[7px] font-bold uppercase leading-none tracking-[0.12em] text-slate-600 dark:text-slate-400">
        OR
      </div>
      {d["label"] ? (
        <div className="truncate px-0.5 text-center font-mono text-[8px] leading-tight text-slate-600 dark:text-slate-500">
          {String(d["label"])}
        </div>
      ) : null}
      <ParamGrid
        rows={[
          { label: "∪", value: "≥2" },
          { label: "λ", value: "—" },
          { label: "τ", value: "—" },
          { label: "MTTR", value: "—" },
          { label: "U", value: "—" },
        ]}
      />
    </>
  );
  return <GateBody gate="or" symbol={<IconOrGate className={gateIconCls} />} footer={footer} />;
}

export function FtaBeNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const p = typeof d["p"] === "number" ? d["p"] : Number(d["p"]) || 0;
  const tauH =
    typeof d["tau_h"] === "number" && d["tau_h"] > 0
      ? d["tau_h"]
      : typeof d["tauH"] === "number" && d["tauH"] > 0
        ? d["tauH"]
        : undefined;
  const mttr =
    typeof d["mttr_h"] === "number"
      ? d["mttr_h"]
      : typeof d["mttrH"] === "number"
        ? d["mttrH"]
        : null;
  const disp = ftaBasicEventDisplay(p, tauH);
  const footer = (
    <>
      <div className="text-center font-mono text-[7px] font-bold uppercase leading-none tracking-[0.12em] text-slate-600 dark:text-slate-400">
        BE
      </div>
      {d["label"] ? (
        <div className="truncate px-0.5 text-center font-mono text-[8px] leading-tight text-slate-600 dark:text-slate-500">
          {String(d["label"])}
        </div>
      ) : null}
      <ParamGrid
        rows={[
          { label: "P", value: disp.p.toFixed(4) },
          { label: "λ", value: `${formatSci(disp.lambdaPerH, 4)}/h` },
          { label: "τ", value: `${Math.round(disp.tauH)}h` },
          { label: "MTTR", value: mttr != null && mttr > 0 ? `${mttr.toFixed(1)}h` : "—" },
          { label: "U", value: disp.u.toFixed(4) },
        ]}
      />
    </>
  );
  return <BeBody symbol={<IconBasicEvent className={gateIconCls} />} footer={footer} />;
}

/** Functional RBD block: rectangle is the asset; primary R inside; λ/τ/MTTR/U in footer band. */
export function RbdBlockNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const r = typeof d["r"] === "number" ? d["r"] : Number(d["r"]) || 0;
  const tauH =
    typeof d["tau_h"] === "number" && d["tau_h"] > 0
      ? d["tau_h"]
      : typeof d["tauH"] === "number" && d["tauH"] > 0
        ? d["tauH"]
        : undefined;
  const mttr =
    typeof d["mttr_h"] === "number"
      ? d["mttr_h"]
      : typeof d["mttrH"] === "number"
        ? d["mttrH"]
        : null;
  const disp = rbdBlockDisplay(r, tauH);
  const label = String(d["label"] ?? "—");

  return (
    <div className="flex flex-col items-stretch bg-transparent">
      <div
        className="relative w-[132px] rounded-sm border-2 border-slate-700 bg-slate-100/35 dark:border-slate-400 dark:bg-slate-950/55"
        style={{ minHeight: 92 }}
      >
        <Handle
          type="target"
          position={Position.Left}
          id="in"
          className={portCls}
          style={{
            position: "absolute",
            left: 0,
            top: "50%",
            transform: "translate(-50%, -50%)",
          }}
        />
        <Handle
          type="source"
          position={Position.Right}
          id="out"
          className={portCls}
          style={{
            position: "absolute",
            right: 0,
            top: "50%",
            transform: "translate(50%, -50%)",
          }}
        />
        <div className="flex h-full flex-col px-2 pb-1 pt-1.5">
          <div className="font-mono text-[7px] font-bold uppercase tracking-[0.14em] text-slate-500 dark:text-slate-400">
            BLK
          </div>
          <div
            className="truncate font-mono text-[9px] leading-tight text-slate-800 dark:text-slate-100"
            title={label}
          >
            {label}
          </div>
          <div className="mt-1 text-center font-mono text-sm font-semibold tabular-nums text-slate-900 dark:text-slate-50">
            R = {disp.r.toFixed(4)}
          </div>
          <div className="mt-auto border-t border-slate-500/70 pt-1 dark:border-slate-600">
            <ParamGrid
              className="border-t-0 pt-0"
              rows={[
                { label: "λ", value: `${formatSci(disp.lambdaPerH, 4)}/h` },
                { label: "τ", value: `${Math.round(disp.tauH)}h` },
                { label: "MTTR", value: mttr != null && mttr > 0 ? `${mttr.toFixed(1)}h` : "—" },
                { label: "U", value: disp.u.toFixed(4) },
              ]}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export function RbdSeriesNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const footer = (
    <>
      <div className="text-center font-mono text-[7px] font-bold uppercase leading-none tracking-[0.12em] text-slate-600 dark:text-slate-400">
        SER
      </div>
      {d["label"] ? (
        <div className="truncate px-0.5 text-center font-mono text-[8px] leading-tight text-slate-600 dark:text-slate-500">
          {String(d["label"])}
        </div>
      ) : null}
      <ParamGrid
        rows={[
          { label: "R", value: "ΠRi" },
          { label: "λ", value: "—" },
          { label: "τ", value: "—" },
          { label: "MTTR", value: "—" },
          { label: "U", value: "—" },
        ]}
      />
    </>
  );
  return (
    <RbdAuxBody which="series" symbol={<IconRbdSeries className={gateIconCls} />} footer={footer} />
  );
}

export function RbdParallelNode({ data }: NodeProps) {
  const d = (data ?? {}) as Record<string, unknown>;
  const footer = (
    <>
      <div className="text-center font-mono text-[7px] font-bold uppercase leading-none tracking-[0.12em] text-slate-600 dark:text-slate-400">
        PAR
      </div>
      {d["label"] ? (
        <div className="truncate px-0.5 text-center font-mono text-[8px] leading-tight text-slate-600 dark:text-slate-500">
          {String(d["label"])}
        </div>
      ) : null}
      <ParamGrid
        rows={[
          { label: "R", value: "1−Π(1−Ri)" },
          { label: "λ", value: "—" },
          { label: "τ", value: "—" },
          { label: "MTTR", value: "—" },
          { label: "U", value: "—" },
        ]}
      />
    </>
  );
  return (
    <RbdAuxBody
      which="parallel"
      symbol={<IconRbdParallel className={gateIconCls} />}
      footer={footer}
    />
  );
}

export const ramsFtaNodeTypes = {
  ftaAnd: FtaAndNode,
  ftaOr: FtaOrNode,
  ftaBe: FtaBeNode,
};

export const ramsRbdNodeTypes = {
  rbdBlock: RbdBlockNode,
  rbdSeries: RbdSeriesNode,
  rbdParallel: RbdParallelNode,
};
