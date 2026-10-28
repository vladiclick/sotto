import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

// Tauri dev server: fixed port, no auto-clear so cargo logs stay visible.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "safari15",
    outDir: "dist",
    // three windows: menu-bar panel, recording overlay, Settings
    rollupOptions: {
      input: {
        panel: resolve(__dirname, "panel.html"),
        overlay: resolve(__dirname, "overlay.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },
});
