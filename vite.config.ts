import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// https://vite.dev/config/
export default defineConfig({
  server: {
    port: 15673,
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // 优化 lucide-react barrel import，避免 dev 模式加载全部图标 (bundle-barrel-imports)
  optimizeDeps: {
    include: ["lucide-react"],
  },
})