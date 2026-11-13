# 官网截图复现

基线：`ccb6250f8cc6f35ac9611d45441b15788d8c3329`，界面版本 `0.7.6-rc.3`。

使用真实 `ui/src/main.js` 和 Vue 组件；Tauri 桥、运行状态、用量、插件及记忆均为公开虚构样例，不读取用户账户或记忆文件。Project Helper 是虚构自装插件。截图采用 macOS UI 用户代理，来自 Windows 上的 Edge 浏览器渲染，**不是 macOS 原生实机截图，也不是后端运行验收**。

已有项目依赖与 Edge 后，在仓库根目录执行：

```sh
node node_modules/vite/bin/vite.js build --config scripts/website-preview/vite.config.mjs
node scripts/website-preview/capture.mjs "theme=light" website/assets/overview-light.webp
node scripts/website-preview/capture.mjs "theme=dark" website/assets/overview-dark.webp
node scripts/website-preview/capture.mjs "theme=dark&family=mcode&mcodeRunning=1" website/assets/minimax-dark.webp
node scripts/website-preview/capture.mjs "theme=light&panel=插件" website/assets/plugins-light.webp
node scripts/website-preview/capture.mjs "theme=dark&panel=插件" website/assets/plugins-dark.webp
node scripts/website-preview/capture.mjs "theme=light&panel=记忆" website/assets/memory-light.webp
node scripts/website-preview/capture.mjs "theme=dark&panel=记忆" website/assets/memory-dark.webp
```

输出为 1040 × 748 CSS 像素、2x WebP。按顺序执行，脚本占用本地 38123 / 39333 端口。可用 `BROWSER_EXECUTABLE` 指定已有 Chromium 系浏览器，`PREVIEW_CDP_PORT` 更换调试端口。不要指向个人浏览器 profile。初次样例变更应检查截图和控制台，不把通过视觉截图解释为模型、原生窗口或插件能力验证。

Windows Edge 验证：网站375、768、1440宽度无横向溢出；图片加载正常；明暗图片切换、原图链接同步、放大、Escape关闭、焦点恢复和恢复滚动通过。网站5项单元测试通过。未做 macOS 原生运行或真实模型请求。
