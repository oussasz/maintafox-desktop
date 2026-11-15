// Dev server of the field app design preview (mobile-preview.html) ONLY; no build uses it.
// The preview runs the real screens on a stand-in engine. Inside the app WebView (emulator
// captures) Tauri defines its IPC bridge read-only, so here the Tauri library reads the preview
// bridge first when one is installed (src/mobile/preview/fake-engine.ts).
import { type Plugin, mergeConfig } from "vite";

import base from "./vite.config";

const previewBridge: Plugin = {
  name: "maintafox-mobile-preview-bridge",
  enforce: "pre",
  transform(code, id) {
    if (!/@tauri-apps[\/]api[\/]/.test(id)) return null;
    return code
      .replaceAll(
        "window.__TAURI_INTERNALS__",
        "(window.__MFX_PREVIEW_INTERNALS__ ?? window.__TAURI_INTERNALS__)",
      )
      .replaceAll(
        "window.__TAURI_EVENT_PLUGIN_INTERNALS__",
        "(window.__MFX_PREVIEW_EVENT_INTERNALS__ ?? window.__TAURI_EVENT_PLUGIN_INTERNALS__)",
      );
  },
};

export default mergeConfig(base, {
  plugins: [previewBridge],
  optimizeDeps: { exclude: ["@tauri-apps/api"] },
});
