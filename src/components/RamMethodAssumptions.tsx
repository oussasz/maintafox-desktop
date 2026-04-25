import { useState } from "react";

const BLOCKS: { id: string; title: string; lines: string[] }[] = [
  {
    id: "weibull",
    title: "Weibull",
    lines: [
      "Événements indépendants dans le temps ; inter-arrivées i.i.d. pour l’ajustement.",
      "MLE 2 paramètres ; IC asymptotiques — grands échantillons.",
    ],
  },
  {
    id: "fmeca",
    title: "FMECA",
    lines: [
      "S, O, D sur échelle 1–10 ; RPN = S×O×D ; pas de corrélation explicite entre facteurs.",
    ],
  },
  {
    id: "fta",
    title: "FTA",
    lines: [
      "Portes ET/OU ; probabilités indépendantes entre entrées ; pas de dépendances communes modélisées.",
    ],
  },
  {
    id: "rbd",
    title: "RBD",
    lines: ["Blocs indépendants ; série = produit des fiabilités ; parallèle = 1−Π(1−Ri)."],
  },
  {
    id: "eta",
    title: "Arbre d’événements",
    lines: [
      "Graphe orienté sans recombinaison (usage typique) ; probabilités de branche conditionnelles données.",
    ],
  },
  {
    id: "mc",
    title: "Monte Carlo",
    lines: [
      "Échantillonnage avec graine fixée pour reproductibilité ; bornes uniformes et Bernoulli supportées.",
    ],
  },
  {
    id: "markov",
    title: "Markov (DTMC)",
    lines: [
      "Chaîne discrète temps homogène ; matrice stochastique ; régime permanent par itération.",
    ],
  },
];

export function RamMethodAssumptions() {
  const [open, setOpen] = useState<string | null>("weibull");
  return (
    <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
      <h2 className="mb-2 font-medium">Hypothèses par méthode (RAM avancé)</h2>
      <p className="mb-3 text-xs text-fg-2">
        Résumé local pour revue — ne remplace pas la norme applicable ni la validation métier.
      </p>
      <ul className="space-y-1">
        {BLOCKS.map((b) => (
          <li key={b.id} className="rounded border border-border-1/60 bg-bg-0">
            <button
              type="button"
              className="flex w-full items-center justify-between px-2 py-1.5 text-left text-xs font-medium text-fg-1"
              onClick={() => setOpen(open === b.id ? null : b.id)}
            >
              {b.title}
              <span className="text-fg-2">{open === b.id ? "−" : "+"}</span>
            </button>
            {open === b.id ? (
              <ul className="list-inside list-disc px-2 pb-2 text-[11px] text-fg-2">
                {b.lines.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
