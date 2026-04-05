import { useEffect } from "react";
import { RouterProvider } from "react-router-dom";

import { LocaleHtmlDir } from "@/components/locale-html-dir";
import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { router } from "@/router";
import { useSettingsStore } from "@/stores/settings-store";

export function App() {
  useEffect(() => {
    void useSettingsStore.getState().loadSessionPolicy();
  }, []);

  return (
    <ThemeProvider>
      <LocaleHtmlDir />
      <RouterProvider
        router={router}
        future={{
          v7_startTransition: true,
        }}
      />
    </ThemeProvider>
  );
}
