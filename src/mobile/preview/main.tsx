// DESIGN PREVIEW ONLY (mobile-preview.html, `vite --config vite.mobile-preview.config.ts`; never built into the app):
// every screen of the field app, real screens on a stand-in engine with sample data, and the
// screens of features not built yet as labelled mockups. Pick one with the URL hash:
//   /mobile-preview.html#s=wo-list&lang=ar&theme=dark

import { AlertTriangle, ClipboardList, RefreshCw, ScanLine, Settings } from "lucide-react";
import React, { type ReactNode, useState } from "react";
import ReactDOM from "react-dom/client";
import { I18nextProvider } from "react-i18next";

import { MobileApp } from "../MobileApp";
import { AppPinMockup } from "../field/AppPin";
import { ScanEquipmentMockup } from "../field/ScanEquipment";
import { type ExecutionStage, WorkOrderExecutionMockup } from "../field/WorkOrderExecution";
import { type MobileLanguage, createMobileI18n, isMobileLanguage, useMobileT } from "../i18n";
import { applyAppearance } from "../lib/appearance";
import { TabBar } from "../ui/kit";
import "../mobile.css";

import { SCENARIO_ANSWERS, installFakeEngine, signedInAnswers } from "./fake-engine";
import { SAMPLE_EXECUTION, SAMPLE_SCAN, SAMPLE_TECHNICIAN } from "./sample-data";

/** One interaction played after the screen renders (language-independent selectors). */
type Step = { fill: string; value: string } | { click: string; index?: number } | { wait: number };

interface Scenario {
  /** Real screens (the field app on the stand-in engine) or a mockup. */
  view: "app" | "pin" | "exec" | "scan";
  stage?: ExecutionStage;
  found?: boolean;
  steps?: Step[];
}

const TAB = ".mf-tabbar .mf-tab";
const SUBMIT = "button[type=submit]";

export const SCENARIOS: Record<string, Scenario> = {
  "security-refused": { view: "app" },
  startup: { view: "app" },
  "activation-key": {
    view: "app",
    steps: [{ fill: ".mf-cell__input", value: "MFX-7Q2K-91DA-4C3E" }],
  },
  "activation-unreachable": {
    view: "app",
    steps: [{ fill: ".mf-cell__input", value: "MFX-7Q2K-91DA-4C3E" }, { click: SUBMIT }],
  },
  "activation-confirm": {
    view: "app",
    steps: [{ fill: ".mf-cell__input", value: "MFX-7Q2K-91DA-4C3E" }, { click: SUBMIT }],
  },
  "plant-joining": { view: "app" },
  signin: { view: "app", steps: [{ fill: ".mf-cell__input", value: SAMPLE_TECHNICIAN.username }] },
  "signin-refused": {
    view: "app",
    steps: [
      { fill: ".mf-cell__input", value: SAMPLE_TECHNICIAN.username },
      { fill: "input[type=password]", value: "********" },
      { click: SUBMIT },
    ],
  },
  "app-pin": { view: "pin" },
  "wo-list": { view: "app" },
  "wo-empty": { view: "app" },
  "wo-detail": { view: "app", steps: [{ click: ".mf-wo" }] },
  "exec-ready": { view: "exec", stage: "ready" },
  "exec-running": { view: "exec", stage: "running" },
  "exec-finish": { view: "exec", stage: "finish" },
  "exec-failure-picker": {
    view: "exec",
    stage: "finish",
    steps: [{ click: ".mf-sheet [aria-haspopup=dialog]" }],
  },
  scan: { view: "scan", found: false },
  "scan-found": { view: "scan", found: true },
  report: { view: "app", steps: [{ click: TAB, index: 1 }] },
  "report-equipment": {
    view: "app",
    steps: [{ click: TAB, index: 1 }, { click: "form .mf-row" }],
  },
  "report-urgency": {
    view: "app",
    steps: [
      { click: TAB, index: 1 },
      { click: "form [aria-haspopup=dialog]", index: 3 },
    ],
  },
  "report-missing": { view: "app", steps: [{ click: TAB, index: 1 }, { click: SUBMIT }] },
  sync: { view: "app", steps: [{ click: TAB, index: 2 }] },
  "sync-offline": { view: "app", steps: [{ click: TAB, index: 2 }] },
  settings: { view: "app", steps: [{ click: TAB, index: 3 }] },
  "settings-signout": {
    view: "app",
    steps: [{ click: TAB, index: 3 }, { click: ".mf-row--destructive" }],
  },
};

const STEP_DELAY_MS = 450;

function setNativeValue(el: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/** The element a step acts on, once the screen shows it (a slow phone renders later). */
async function target(selector: string, index = 0): Promise<HTMLElement | null> {
  for (let i = 0; i < 60; i++) {
    const el = document.querySelectorAll<HTMLElement>(selector)[index];
    if (el && !(el as HTMLButtonElement).disabled) return el;
    await new Promise((r) => setTimeout(r, 200));
  }
  console.warn(`[preview] step target not found: ${selector}`);
  return null;
}

async function play(steps: Step[]) {
  for (const step of steps) {
    await new Promise((r) => setTimeout(r, STEP_DELAY_MS));
    if ("wait" in step) {
      await new Promise((r) => setTimeout(r, step.wait));
    } else if ("fill" in step) {
      const el = (await target(step.fill)) as HTMLInputElement | null;
      if (el) setNativeValue(el, step.value);
    } else {
      (await target(step.click, step.index ?? 0))?.click();
    }
  }
  await new Promise((r) => setTimeout(r, STEP_DELAY_MS * 2));
  document.body.dataset["previewReady"] = "1";
}

/** The five-tab bar of field version 1 (with Scan), under the mockup screens. */
function FutureTabBar({ active }: { active: string }) {
  const { t } = useMobileT();
  return (
    <TabBar
      label={t("tabs.label")}
      active={active}
      onSelect={() => undefined}
      items={[
        { key: "work", label: t("tabs.work"), icon: <ClipboardList aria-hidden /> },
        { key: "report", label: t("tabs.report"), icon: <AlertTriangle aria-hidden /> },
        { key: "scan", label: t("tabs.scan"), icon: <ScanLine aria-hidden /> },
        { key: "sync", label: t("tabs.sync"), icon: <RefreshCw aria-hidden /> },
        { key: "settings", label: t("tabs.settings"), icon: <Settings aria-hidden /> },
      ]}
    />
  );
}

function PreviewCaption({ mockup }: { mockup: boolean }) {
  const { language } = useMobileT();
  const text = {
    fr: mockup
      ? "Maquette de conception — fonction de l'étape 1, données d'exemple"
      : "Aperçu de conception — écran réel, données d'exemple",
    en: mockup
      ? "Design mockup — step 1 feature, sample data"
      : "Design preview — real screen, sample data",
    ar: mockup
      ? "نموذج تصميم — ميزة الخطوة 1، بيانات توضيحية"
      : "معاينة التصميم — شاشة حقيقية، بيانات توضيحية",
  }[language];
  return (
    <>
      {/* The caption takes the bottom 18 px; the tab bar and toasts sit above it. */}
      <style>{`.mf-tabbar{inset-block-end:18px}.mf-toast{margin-block-end:18px}.mf-sheet{padding-block-end:18px}`}</style>
      <div
        style={{
          position: "fixed",
          insetInline: 0,
          insetBlockEnd: 0,
          zIndex: 60,
          padding: "3px 8px",
          textAlign: "center",
          font: "500 11px/1.3 var(--mf-font)",
          background: mockup ? "#f68714" : "#0b1f3a",
          color: mockup ? "#000" : "#fff",
        }}
      >
        {text}
      </div>
    </>
  );
}

function Preview({ scenario }: { scenario: Scenario }) {
  const [, rerender] = useState(0);
  let body: ReactNode;
  switch (scenario.view) {
    case "pin":
      body = <AppPinMockup name={SAMPLE_TECHNICIAN.display_name} filled={2} />;
      break;
    case "exec":
      body = (
        <WorkOrderExecutionMockup
          data={SAMPLE_EXECUTION}
          stage={scenario.stage ?? "running"}
          onBack={() => rerender((n) => n + 1)}
        />
      );
      break;
    case "scan":
      body = (
        <>
          <ScanEquipmentMockup found={scenario.found ? SAMPLE_SCAN : null} />
          <FutureTabBar active="scan" />
        </>
      );
      break;
    default:
      body = <MobileApp />;
  }
  return (
    <>
      {body}
      <PreviewCaption mockup={scenario.view !== "app"} />
    </>
  );
}

const params = new URLSearchParams(window.location.hash.slice(1));
const name = params.get("s") ?? "wo-list";
const scenario = SCENARIOS[name] ?? SCENARIOS["wo-list"];
const langParam = params.get("lang");
const language: MobileLanguage = isMobileLanguage(langParam) ? langParam : "fr";
const theme = params.get("theme") === "dark" ? "dark" : "light";

// The emulator captures load this page over plain HTTP, where browsers withhold randomUUID (the
// app itself runs on a secure origin): give the preview a stand-in.
if (typeof crypto.randomUUID !== "function") {
  Object.assign(crypto, {
    randomUUID: () => "00000000-0000-4000-8000-" + String(Date.now()).padStart(12, "0").slice(-12),
  });
}
installFakeEngine({ ...signedInAnswers(), ...(SCENARIO_ANSWERS[name] ?? {}) });
applyAppearance(theme);
const i18n = createMobileI18n(language);

const rootElement = document.getElementById("root");
if (!rootElement || !scenario) throw new Error("preview root or scenario missing");
ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <I18nextProvider i18n={i18n}>
      <Preview scenario={scenario} />
    </I18nextProvider>
  </React.StrictMode>,
);
void play(scenario.steps ?? []);
window.addEventListener("hashchange", () => window.location.reload());
