// WCAG contrast audit of the status-tone tokens and text tokens in globals.css.
// Usage: node scripts/check-tone-contrast.mjs   (exit 1 when a text pair is < 4.5:1)
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/styles/globals.css", import.meta.url), "utf8");

function block(selectorRe) {
  const m = css.match(new RegExp(selectorRe + String.raw`\s*\{([\s\S]*?)\r?\n\}`));
  if (!m) throw new Error(`block not found: ${selectorRe}`);
  const vars = {};
  for (const v of m[1].matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) vars[v[1]] = v[2];
  return vars;
}
const themes = { dark: block(String.raw`:root,\s*\.dark`), light: block(String.raw`\.light`) };

const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const lum = (hex) => {
  const n = parseInt(hex.slice(1), 16);
  return (
    0.2126 * lin((n >> 16) / 255) +
    0.7152 * lin(((n >> 8) & 255) / 255) +
    0.0722 * lin((n & 255) / 255)
  );
};
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

const tones = ["neutral", "info", "progress", "attention", "warning", "success", "danger"];
let failed = 0;
const rows = [];
for (const [name, v] of Object.entries(themes)) {
  for (const t of tones) {
    const bg = v[`tone-${t}-bg`],
      fg = v[`tone-${t}-fg`],
      bd = v[`tone-${t}-border`];
    rows.push([name, `tone-${t} fg/bg (text)`, fg, bg, ratio(fg, bg), 4.5]);
    rows.push([name, `tone-${t} border/bg (non-text)`, bd, bg, ratio(bd, bg), 3]);
  }
  for (const k of [
    "text-primary",
    "text-secondary",
    "text-muted",
    "text-danger",
    "text-success",
    "text-warning",
  ]) {
    for (const s of ["surface-0", "surface-1", "surface-2"]) {
      rows.push([name, `${k} on ${s}`, v[k], v[s], ratio(v[k], v[s]), 4.5]);
    }
  }
}
for (const [theme, pair, a, b, r, min] of rows) {
  const ok = r >= min;
  // Tone borders are decorative edges (the chip text carries the meaning); reported, not gating.
  const gating = !pair.includes("border/bg");
  if (!ok && gating) failed++;
  console.log(
    `${ok ? "PASS" : gating ? "FAIL" : "info"}  ${theme.padEnd(5)} ${pair.padEnd(34)} ${a} on ${b}  ${r.toFixed(2)} (min ${min})`,
  );
}
if (failed > 0) {
  console.error(`${failed} text pair(s) below WCAG AA`);
  process.exit(1);
}
