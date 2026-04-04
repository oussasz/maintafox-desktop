import React from "react";
import ReactDOM from "react-dom/client";

import { initI18n } from "@/i18n";

import { App } from "./App";
import "./styles/globals.css";

// Initialize i18n before anything else.
initI18n();

type TauriInvoke = <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

if (import.meta.env.DEV) {
  const w = window as Window & {
    __TAURI_INTERNALS__?: {
      invoke?: TauriInvoke;
    };
    __TAURI__?: {
      core?: {
        invoke: TauriInvoke;
      };
    };
    TAURI?: {
      core?: {
        invoke: TauriInvoke;
      };
    };
  };

  const waitForBridge = async (timeoutMs = 1500): Promise<TauriInvoke | undefined> => {
    const started = Date.now();
    while (Date.now() - started < timeoutMs) {
      if (w.__TAURI_INTERNALS__?.invoke) {
        return w.__TAURI_INTERNALS__.invoke;
      }
      await new Promise<void>((resolve) => {
        setTimeout(resolve, 50);
      });
    }
    return undefined;
  };

  const devInvoke = (async <T = unknown,>(
    cmd: string,
    args?: Record<string, unknown>,
  ): Promise<T> => {
    const bridgeInvoke = w.__TAURI_INTERNALS__?.invoke ?? (await waitForBridge());
    if (!bridgeInvoke) {
      throw new Error(
        "Tauri bridge unavailable. Open DevTools from the Tauri app window started by pnpm tauri dev.",
      );
    }

    return bridgeInvoke<T>(cmd, args);
  }) as TauriInvoke;

  w.__TAURI__ = w.__TAURI__ ?? {};
  w.__TAURI__.core = w.__TAURI__.core ?? { invoke: devInvoke };
  w.TAURI = w.TAURI ?? {};
  w.TAURI.core = w.TAURI.core ?? { invoke: devInvoke };
}

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Root element not found");

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
