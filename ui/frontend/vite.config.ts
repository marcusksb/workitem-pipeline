import type { ClientRequest } from "node:http";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

const backend = "http://127.0.0.1:7373";

// The backend only accepts its own Host and Origin, so the dev proxy rewrites both.
export default defineConfig({
  // Vitest must not pick up the Playwright files in e2e/.
  test: { include: ["src/**/*.test.ts"] },
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    proxy: {
      "/api": { target: backend, changeOrigin: true },
      "/ws": {
        target: backend,
        changeOrigin: true,
        ws: true,
        configure: (proxy) => {
          const events = proxy as unknown as NodeJS.EventEmitter;
          events.on("proxyReqWs", (request: ClientRequest) => request.setHeader("origin", backend));
        },
      },
    },
  },
});
