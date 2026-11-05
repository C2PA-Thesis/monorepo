import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    target: "es2022",
    rollupOptions: {
      // The capture page for the phone, and the presenter page for the laptop.
      input: { capture: resolve(import.meta.dirname, "index.html"), screen: resolve(import.meta.dirname, "screen.html") },
    },
  },
  server: {
    // `npm run dev` talks to a local `provenance serve` for the API.
    proxy: { "/api": "http://127.0.0.1:8791" },
  },
  test: { environment: "node" },
});
