// Vite 构建配置：管理面板是 Vue 3 + Element Plus 单页应用，源码在 ui/src，
// 产物输出到 ui/dist（tauri.conf.json 的 frontendDist 指向它）。
// `tauri dev` 经 beforeDevCommand 起 dev server（devUrl 5174，端口由
// `scripts/dev.mjs` 统一），`tauri build` 经 beforeBuildCommand 跑 `vite build`。
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
        //
        // vite 8 的打包器换成了 rolldown，`manualChunks` 的**对象写法已被移除**
        // （函数写法仍可用但已废弃），官方给的新选项是 `output.codeSplitting.groups`，
        // 见 vite docs/guide/migration.md 的 "Removed object form … manualChunks"。
        //
        // `test` 用 `[\\/]` 而不是 `/node_modules/foo/`：rolldown 的类型定义明确
        // 提醒 Windows 上别用后者（路径分隔符会漏匹配），本仓 dev 与 CI 都在
        // Windows 上跑过。
        //
        // **vue 这一组目前不产出独立块**，这不是写错了：组的
        // `includeDependenciesRecursively` 默认 true，即「连同依赖一起收」，而
        // element-plus 的源码 import 了 vue，所以 vue 整个被并进 element-plus
        // 块（element-plus 403 KB / index 252 KB）。两条路都实测过，都不成立：
        // 给 vue 组 `priority: 2` 并排在前面，产物哈希一字不差；给 element-plus
        // 组关掉 `includeDependenciesRecursively`，vue 块仍然不出现，反而让
        // element-plus 的依赖散进 index（index 涨到 410 KB）。留着这一组是为了
        // 上游哪天改了依赖收集语义时 vue 能自己落回独立块；**别把它当成「现在
        // 有一个 vue 块」**。
        codeSplitting: {
          groups: [
            {
              name: 'element-plus',
              test: /node_modules[\\/](?:element-plus|@element-plus)[\\/]/,
            },
            {
              name: 'vue',
              test: /node_modules[\\/]vue[\\/]/,
            },
          ],
        },
      },
    },
  },
  server: {
    // 端口由 `scripts/dev.mjs` 统一决定（`DSH_DEV_PORT`，`pnpm run dev 5190`
    // 或 `DSH_DEV_PORT=5190 pnpm run dev`），默认 5174。**这个数字必须与
    // `scripts/dev.mjs` 的 `DEFAULT_PORT` 和 `tauri.conf.json` 的 `devUrl`
    // 一致**，`scripts/dev-port.test.mjs` 会盯住。
    port: Number(process.env.DSH_DEV_PORT) || 5174,
    // 保持 true：端口被别人占着时宁可报错退出，也不要静默顺延——顺延之后
    // tauri 的 devUrl 会指向一个并不提供本项目的页面，症状比冲突本身难查。
    strictPort: true,
  },
});
