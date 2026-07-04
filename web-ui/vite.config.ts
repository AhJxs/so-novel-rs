import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': path.resolve(__dirname, './src') } },
  server: { proxy: { '/api': 'http://localhost:8080' } },
  build: {
    outDir: 'dist',
    // 单 chunk ~720kB（gzip 224kB），主体是 react + react-aria + heroui 全量
    // 组件 + i18next。传输体积尚可接受，暂时不拆 manualChunks：拆完后单
    // 次首屏体积不降（要下的 vendor 总量一样），但要复杂化部署/缓存策略。
    // 真优化路径是路由级 React.lazy —— 后续按需引入。
    chunkSizeWarningLimit: 800,
  },
})
