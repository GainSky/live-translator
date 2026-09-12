import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 双入口：主窗口 index.html + 悬浮窗 overlay.html
// 对应 src-tauri/tauri.conf.json 中 label 为 main / overlay 的两个窗口
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": "/src",
    },
  },
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        overlay: "overlay.html",
      },
    },
  },
});
