/// <reference types="vitest" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The Python server (bin/cashcast web) serves dist/ and the /api routes.
// For `pnpm dev`, run `bin/cashcast web --port 8765` and the dev server proxies the API to it.
export default defineConfig({
  plugins: [react()],
  server: { proxy: { "/api": "http://127.0.0.1:8765" } },
  test: { environment: "jsdom", globals: true },
});
