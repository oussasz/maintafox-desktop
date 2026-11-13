import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import { resolve } from "path";

const mobile = !!/android|ios/.exec(process.env.TAURI_ENV_PLATFORM ?? "");

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@shared": resolve(__dirname, "./shared"),
    },
  },
  // The Android field app has its own page (mobile.html → src/mobile/); desktop builds keep index.html.
  build: mobile ? { rollupOptions: { input: resolve(__dirname, "mobile.html") } } : undefined,
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: mobile ? "0.0.0.0" : false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
