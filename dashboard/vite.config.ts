import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const DASHBOARD = process.env.RHEBOK_DASHBOARD ?? "http://127.0.0.1:7331";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      "/api": {
        target: DASHBOARD,
        changeOrigin: true,
        configure: (proxy) =>
          proxy.on("proxyReq", (out, req) => {
            if (req.headers.origin === `http://${req.headers.host}`) out.setHeader("origin", DASHBOARD);
          }),
      },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
