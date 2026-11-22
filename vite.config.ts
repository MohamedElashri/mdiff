import { diffsWorkerEntry } from "./vendor/diffle/scripts/diffs-worker-plugin";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";
export default defineConfig({
  plugins: [diffsWorkerEntry(), react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./vendor/diffle/src", import.meta.url)),
    },
    dedupe: ["react", "react-dom", "zustand", "@pierre/diffs"],
  },
  define: { __DIFFLE_VERSION__: JSON.stringify("0.3.6") },
  worker: { format: "es", plugins: () => [diffsWorkerEntry()] },
  build: { target: "es2022" },
});
