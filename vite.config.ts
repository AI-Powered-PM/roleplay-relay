import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Vite must not watch Rust build output: it crashes with EBUSY on the locked .exe.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    rollupOptions: {
      input: {
        settings: "settings.html",
        overlay: "overlay.html",
        compose: "compose.html",
      },
    },
  },
});
