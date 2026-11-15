import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
// 官网仅构建真实前端，桌面桥由 index.html 中的公开样例替代。
export default defineConfig({ root: here, publicDir: path.resolve(here, '../../ui/public'), base: './', plugins: [vue()], build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 5000 } });
