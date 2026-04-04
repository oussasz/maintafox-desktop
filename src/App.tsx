import { RouterProvider } from "react-router-dom";

import { LocaleHtmlDir } from "@/components/locale-html-dir";
import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { router } from "@/router";

export function App() {
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
