import { listen } from "@tauri-apps/api/event";
import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./App";
import "./styles/globals.css";

// Log startup events from the Rust startup sequence to the DevTools console.
// This listener is registered before React mounts so no events are missed.
listen<Record<string, unknown>>("startup_event", (event) => {
  // eslint-disable-next-line no-console -- Startup diagnostics visible only in DevTools
  console.log("[startup_event]", event.payload);
}).catch((err: unknown) => {
  console.warn("Failed to register startup_event listener:", err);
});

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Root element not found");

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
