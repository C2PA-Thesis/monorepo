import { defineConfig } from "vite";

export default defineConfig({
  build: { target: "es2022" },
  server: {
    // `npm run dev` talks to a local `provenance serve` for the API.
    proxy: { "/api": "http://127.0.0.1:8791" },
  },
  test: { environment: "node" },
});
