import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./vendor/diffle/src", import.meta.url)),
    },
    dedupe: ["react", "react-dom", "zustand", "@pierre/diffs"],
  },
  define: { __DIFFLE_VERSION__: JSON.stringify("0.3.6") },
  worker: { format: "es" },
  build: { target: "es2022" },
});
