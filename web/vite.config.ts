import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// Vite config. The dev server proxies /api/v1/* and /health straight to the
// substrate (default http://localhost:8080) so the dashboard can run on
// :5173 without CORS surgery on the Rust server. `VITE_API_BASE_URL` lets
// CI / production override the upstream target.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const apiBaseUrl = env.VITE_API_BASE_URL || "http://localhost:8080";

  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "./src"),
      },
    },
    server: {
      port: 5173,
      proxy: {
        "/api": {
          target: apiBaseUrl,
          changeOrigin: true,
        },
        "/health": {
          target: apiBaseUrl,
          changeOrigin: true,
        },
      },
    },
    build: {
      target: "es2022",
      sourcemap: true,
      // Single-binary distribution embeds dist/ via rust-embed (v0.2.1).
      // Keep the output deterministic so the binary hash is stable across
      // identical source.
      rollupOptions: {
        output: {
          // Stable chunk names — helps with HTTP caching when the substrate
          // serves the static assets directly.
          entryFileNames: "assets/[name]-[hash].js",
          chunkFileNames: "assets/[name]-[hash].js",
          assetFileNames: "assets/[name]-[hash][extname]",
        },
      },
    },
  };
});
