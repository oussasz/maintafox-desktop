import React from "react";
import ReactDOM from "react-dom/client";

import { initI18n } from "@/i18n";

import { App } from "./App";
import "./styles/globals.css";

// Initialize i18n before rendering — must be called before any useTranslation hook.
initI18n();

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Root element not found");

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
