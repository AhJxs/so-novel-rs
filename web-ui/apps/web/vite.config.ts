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
    // 'hidden'：生成 .map 但不带 sourceMappingURL，给报错追踪用又不公开暴露源码结构
    sourcemap: "hidden",
    chunkSizeWarningLimit: 800,
  },
})
