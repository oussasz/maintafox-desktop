import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  IconAndGate,
  IconBasicEvent,
  IconOrGate,
  IconRbdBlock,
  IconRbdParallel,
  IconRbdSeries,
} from "@/components/reliability/flow/ramsLabSymbols";
import { cn } from "@/lib/utils";

type PaletteTool = {
  kind: string;
  Icon: (p: { className?: string }) => ReactElement;
  titleKey: string;
  descKey: string;
};

const FTA_TOOLS: PaletteTool[] = [
  {
    kind: "ftaAnd",
    Icon: IconAndGate,
    titleKey: "lab.palette.and.title",
    descKey: "lab.palette.and.desc",
  },
  {
    kind: "ftaOr",
    Icon: IconOrGate,
    titleKey: "lab.palette.or.title",
    descKey: "lab.palette.or.desc",
  },
  {
    kind: "ftaBe",
    Icon: IconBasicEvent,
    titleKey: "lab.palette.be.title",
    descKey: "lab.palette.be.desc",
  },
];

const RBD_TOOLS: PaletteTool[] = [
  {
    kind: "rbdBlock",
    Icon: IconRbdBlock,
    titleKey: "lab.palette.block.title",
    descKey: "lab.palette.block.desc",
  },
  {
    kind: "rbdSeries",
    Icon: IconRbdSeries,
    titleKey: "lab.palette.series.title",
    descKey: "lab.palette.series.desc",
  },
  {
    kind: "rbdParallel",
    Icon: IconRbdParallel,
    titleKey: "lab.palette.parallel.title",
    descKey: "lab.palette.parallel.desc",
  },
];

export function RamsLabPalette({
  mode,
  onPick,
  disabled,
  orientation = "vertical",
}: {
  mode: "fta" | "rbd";
  onPick: (kind: string) => void;
  disabled?: boolean;
  orientation?: "vertical" | "horizontal";
}) {
  const { t } = useTranslation("reliability");
  const tools = mode === "fta" ? FTA_TOOLS : RBD_TOOLS;

  return (
    <div
      className={cn(
        "z-20 flex select-none rounded-sm border border-slate-400/80 bg-white/95 p-1 shadow-none backdrop-blur-sm dark:border-slate-600 dark:bg-slate-900/95",
        orientation === "vertical" ? "flex-col gap-0.5" : "flex-row flex-wrap items-center gap-0.5",
      )}
      role="toolbar"
      aria-label={t("lab.palette.toolbarLabel")}
    >
      {tools.map(({ kind, Icon, titleKey, descKey }) => (
        <div key={kind} className="group relative">
          <button
            type="button"
            disabled={disabled}
            onClick={() => onPick(kind)}
            className={cn(
              "flex h-9 w-9 items-center justify-center rounded-[2px] border border-transparent text-slate-800 transition-colors dark:text-slate-100",
              "hover:border-slate-400 hover:bg-slate-100 dark:hover:border-slate-500 dark:hover:bg-slate-800",
              "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-slate-500",
              disabled && "pointer-events-none opacity-40",
            )}
            aria-label={t(titleKey)}
          >
            <Icon className="h-7 w-7 shrink-0" />
          </button>
          <div
            className={cn(
              "pointer-events-none absolute z-50 hidden w-max max-w-[220px] rounded-sm border border-slate-400 bg-white px-2.5 py-2 text-left text-[10px] shadow-sm dark:border-slate-600 dark:bg-slate-900",
              "group-hover:block",
              orientation === "vertical"
                ? "left-full top-1/2 ml-2 -translate-y-1/2"
                : "bottom-full left-1/2 mb-2 -translate-x-1/2",
            )}
            role="tooltip"
          >
            <p className="font-semibold text-slate-800 dark:text-slate-100">{t(titleKey)}</p>
            <p className="mt-1 leading-snug text-slate-600 dark:text-slate-400">{t(descKey)}</p>
          </div>
        </div>
      ))}
    </div>
  );
}
