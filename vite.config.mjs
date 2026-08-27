// Vite 构建配置：管理面板是 Vue 3 + Element Plus 单页应用，源码在 ui/src，
// 产物输出到 ui/dist（tauri.conf.json 的 frontendDist 指向它）。
// `tauri dev` 经 beforeDevCommand 起 dev server（devUrl 5173），
// `tauri build` 经 beforeBuildCommand 跑 `vite build`。
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  root: 'ui',
  plugins: [vue()],
  // Tauri 用 tauri:// 自定义协议加载产物，资源引用必须是相对路径。
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // 管理面板全量引入 Element Plus（本地加载的桌面壳，303 KB gzip 可接受），
    // 按包分块后单块必然超过 500 KB 默认线，把警告阈值提到与事实匹配。
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        // 桌面应用本地加载，拆块只为让缓存与增量构建更友好。
        manualChunks: {
          vue: ['vue'],
          'element-plus': ['element-plus', '@element-plus/icons-vue'],
        },
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
  },
});
