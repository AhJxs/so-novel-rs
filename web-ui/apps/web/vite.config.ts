import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: { "/api": "http://localhost:8080" },
  },
  build: {
    // 生产 sourcemap: 'hidden' —— 生成 .map 但不带 //# sourceMappingURL，
    // 给 Sentry / 用户报错用，公开文件不暴露源码结构。
    sourcemap: "hidden",
    chunkSizeWarningLimit: 800,
  },
})
