import { useState } from "react";
import { useTranslation } from "react-i18next";

const BLOCKS: { id: string; titleKey: string; lineKeys: string[] }[] = [
  {
    id: "weibull",
    titleKey: "ramAssumptions.weibull.title",
    lineKeys: ["ramAssumptions.weibull.line1", "ramAssumptions.weibull.line2"],
  },
  {
    id: "fmeca",
    titleKey: "ramAssumptions.fmeca.title",
    lineKeys: ["ramAssumptions.fmeca.line1"],
  },
  {
    id: "fta",
    titleKey: "ramAssumptions.fta.title",
    lineKeys: ["ramAssumptions.fta.line1"],
  },
  {
    id: "rbd",
    titleKey: "ramAssumptions.rbd.title",
    lineKeys: ["ramAssumptions.rbd.line1"],
  },
  {
    id: "eta",
    titleKey: "ramAssumptions.eta.title",
    lineKeys: ["ramAssumptions.eta.line1"],
  },
  {
    id: "mc",
    titleKey: "ramAssumptions.mc.title",
    lineKeys: ["ramAssumptions.mc.line1"],
  },
  {
    id: "markov",
    titleKey: "ramAssumptions.markov.title",
    lineKeys: ["ramAssumptions.markov.line1"],
  },
];

export function RamMethodAssumptions() {
  const { t } = useTranslation("reliability");
  const [open, setOpen] = useState<string | null>("weibull");
  return (
    <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
      <h2 className="mb-2 font-medium">{t("ramAssumptions.title")}</h2>
      <p className="mb-3 text-xs text-fg-2">{t("ramAssumptions.disclaimer")}</p>
      <ul className="space-y-1">
        {BLOCKS.map((b) => (
          <li key={b.id} className="rounded border border-surface-border/60 bg-bg-0">
            <button
              type="button"
              className="flex w-full items-center justify-between px-2 py-1.5 text-left text-xs font-medium text-fg-1"
              onClick={() => setOpen(open === b.id ? null : b.id)}
            >
              {t(b.titleKey)}
              <span className="text-fg-2">{open === b.id ? "−" : "+"}</span>
            </button>
            {open === b.id ? (
              <ul className="list-inside list-disc px-2 pb-2 text-[11px] text-fg-2">
                {b.lineKeys.map((lineKey) => (
                  <li key={lineKey}>{t(lineKey)}</li>
                ))}
              </ul>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
