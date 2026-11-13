import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import { I18nextProvider } from "react-i18next";

import { type MobileLanguage, createMobileI18n } from "../i18n";

/** Renders a mobile screen inside its own i18n instance (never the desktop one). */
export function renderMobile(ui: ReactElement, language: MobileLanguage = "fr") {
  const i18n = createMobileI18n(language);
  return { i18n, ...render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>) };
}

/** A dispatch table for the IPC boundary: command name to its answer (or a thrown refusal). */
export type IpcHandlers = Record<string, (args?: Record<string, unknown>) => unknown>;

export function ipcAnswering(handlers: IpcHandlers) {
  return async (cmd: string, args?: unknown): Promise<unknown> => {
    const handler = handlers[cmd];
    if (!handler) throw new Error(`unexpected IPC command in test: ${cmd}`);
    return handler(args as Record<string, unknown> | undefined);
  };
}
