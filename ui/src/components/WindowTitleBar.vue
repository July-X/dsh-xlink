<script setup>
// macOS 与 Windows 主窗口共用自绘标题栏。Linux 暂不启用，继续沿用系统窗口装饰。
// 标题栏按 394×78 参考图比例定 32px 高，承担「拖拽区域 + 交通灯 + 毛笔纹理」职责；
// 标题文字、菜单入口等放在侧栏的卡片里，不混进标题栏。
const isCustomTitlebarPlatform = /Macintosh|Mac OS X|Windows NT/.test(navigator.userAgent);
const windowApi = window.__TAURI__ && window.__TAURI__.window;
const appWindow = windowApi && windowApi.getCurrentWindow ? windowApi.getCurrentWindow() : null;

function callWindow(method) {
  if (!appWindow || typeof appWindow[method] !== 'function') return;
  appWindow[method]().catch(() => {});
}

function closeWindow() {
  // 仍然走 Tauri close()，让 Rust 侧已有的退出确认逻辑继续生效。
  callWindow('close');
}

function minimizeWindow() {
  callWindow('minimize');
}
</script>

<template>
  <header v-if="isCustomTitlebarPlatform" class="mac-titlebar" data-tauri-drag-region>
    <div class="mac-titlebar__controls" aria-label="窗口控制">
      <button
        type="button"
        class="mac-titlebar__light mac-titlebar__light--close"
        aria-label="关闭窗口"
        title="关闭"
        @click.stop="closeWindow"
      ></button>
      <button
        type="button"
        class="mac-titlebar__light mac-titlebar__light--minimize"
        aria-label="最小化窗口"
        title="最小化"
        @click.stop="minimizeWindow"
      ></button>
      <button
        type="button"
        class="mac-titlebar__light mac-titlebar__light--zoom"
        aria-label="窗口不可缩放"
        title="窗口不可缩放"
        disabled
      ></button>
    </div>

    <span class="mac-titlebar__brush mac-titlebar__brush--light" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--ink" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--dry" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--tip" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--ridge" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--broken" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--bristle" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--drip" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--smear" aria-hidden="true"></span>
    <span class="mac-titlebar__brush mac-titlebar__brush--streak" aria-hidden="true"></span>
  </header>
</template>
