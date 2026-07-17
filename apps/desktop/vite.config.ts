import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // Keep Vite's generated config/cache outside node_modules. This also makes
  // the debug app work in managed workspaces where node_modules is read-only.
  cacheDir: "../../.vite-cache/desktop",
  server: {
    strictPort: true,
    host: "127.0.0.1",
    port: 1420,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
