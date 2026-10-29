import React from "react";
import ReactDOM from "react-dom/client";
import { I18nextProvider } from "react-i18next";

import { MobileApp } from "./MobileApp";
import { mobileI18n } from "./i18n";
import "./mobile.css";

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Root element not found");

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <I18nextProvider i18n={mobileI18n}>
      <MobileApp />
    </I18nextProvider>
  </React.StrictMode>,
);
