import type { ReactNode } from "react";

import { useMobileT } from "../i18n";

import { LanguageSwitch } from "./LanguageSwitch";

/** One screen: header with the app name and the language switch, then the screen content. */
export function Shell({ title, children }: { title: string; children: ReactNode }) {
  const { t } = useMobileT();
  return (
    <div className="mf-shell">
      <header className="mf-header">
        <span className="mf-header__app">{t("app.name")}</span>
        <LanguageSwitch />
      </header>
      <main className="mf-main">
        <h1 className="mf-title">{title}</h1>
        {children}
      </main>
    </div>
  );
}

/** An engine error shown as it is, under a translated title. */
export function ErrorBox({ title, message }: { title: string; message: string }) {
  return (
    <div className="mf-error" role="alert">
      <strong>{title}</strong>
      <p>{message}</p>
    </div>
  );
}
