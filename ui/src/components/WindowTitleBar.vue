<script setup>
// macOS 与 Windows 主窗口共用这一个自绘标题栏组件（Linux 继续沿用系统装饰）。
//
// 两套 chrome 是分开的，不做「一套外观两端妥协」：
//   · macOS：左上角红黄绿交通灯（原生位置与语义）。
//   · Windows：右侧的最小化 / 关闭按钮，尺寸与反馈对齐 Windows 11
//     标准标题栏——46×32 命中区、10px 细线字形、hover 一层浅色底、
//     关闭 hover 变系统红。交通灯在 Windows 上既不是用户肌肉记忆里的
//     位置，14px 的圆形命中区也远小于 Windows 的标题栏按钮。
//
// Windows 上这两个按钮都只把窗口收进通知区域（托盘常驻，程序继续在后台
// 运行）：关闭走窗口 close 请求、由 Rust 侧统一改写为「收起 + 移除任务栏
// 按钮」，最小化走 minimize_shell。真正退出只在托盘菜单的「退出」。
import { hasWindowControls, invoke, windowAction } from '../bridge.js';
import { toastError } from '../notify.js';

const isMacTitlebar = /Macintosh|Mac OS X/.test(navigator.userAgent);
const isWindowsTitlebar = /Windows NT/.test(navigator.userAgent);
const hasControls = hasWindowControls();

// 失败必须让用户看见：旧实现 `.catch(() => {})` 让标题栏按钮"点了没反应"，
// 而用户此时唯一的出路是系统快捷键 —— 提示里要写出来（P2-39）。
function callWindow(method, label, hint) {
  windowAction(method).catch((error) => {
    const detail = error && error.message ? error.message : String(error);
    toastError(`${label}失败：${detail}。${hint}`);
  });
}

// 仍然走 Tauri close()，让 Rust 侧已有的窗口关闭处理继续生效：Windows 上
// 它会拦截这次关闭并收进通知区域（内核与工作台继续后台运行，重新打开与
// 退出都在托盘菜单里），macOS 上维持原有语义。「已收起」的提示由 Rust 的
// shell-hidden-to-tray 事件驱动（见 App.vue），这里不重复提示——否则窗口
// 已经隐藏，toast 也没人看得见。
function closeWindow() {
  const hint = isWindowsTitlebar
    ? '可改用系统快捷键（Alt+F4），或用右下角托盘图标的右键菜单退出'
    : '可改用系统快捷键（Cmd+W / Cmd+M）';
  callWindow('close', '关闭窗口', hint);
}

function minimizeWindow() {
  // Windows：最小化与关闭语义一致——都收进通知区域并从任务栏移除按钮，
  // 只有托盘图标能把窗口叫回来（macOS 保持系统原生最小化到 Dock）。
  // 失败提示里不能给 Win+↓：那条快捷键是收进任务栏，与这里的语义相反，
  // 会让人以为「窗口还在任务栏上」。
  if (isWindowsTitlebar) {
    invoke('minimize_shell').catch((error) => {
      const detail = error && error.message ? error.message : String(error);
      toastError(`最小化到通知区域失败：${detail}。可点右下角托盘图标确认程序是否仍在运行`);
    });
    return;
  }
  callWindow('minimize', '最小化窗口', '可改用系统快捷键（Cmd+M）');
}
</script>

<template>
  <header
    v-if="(isMacTitlebar || isWindowsTitlebar) && hasControls"
    class="mac-titlebar"
    :class="{ 'mac-titlebar--win': isWindowsTitlebar }"
    data-tauri-drag-region
  >
    <!-- macOS：左上角交通灯。 -->
    <div v-if="isMacTitlebar" class="mac-titlebar__controls" aria-label="窗口控制">
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

    <div class="mac-titlebar__caption" data-tauri-drag-region>
      <span class="mac-titlebar__caption-mark" aria-hidden="true"></span>
      <span>Dsh-Xlink</span>
    </div>

    <!-- Windows：右侧标准标题栏按钮（最小化 / 关闭）。窗口不可缩放
         （tauri.conf.json: resizable:false），所以不放最大化按钮——禁用
         的最大化按钮只会让用户反复点击一个永远不响应的地方。 -->
    <div v-if="isWindowsTitlebar" class="win-caption" aria-label="窗口控制">
      <button
        type="button"
        class="win-caption__btn"
        aria-label="最小化窗口"
        title="最小化"
        @click.stop="minimizeWindow"
      >
        <svg class="win-caption__glyph" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5h10" />
        </svg>
      </button>
      <button
        type="button"
        class="win-caption__btn win-caption__btn--close"
        aria-label="关闭窗口（收纳到通知区域）"
        title="关闭（收纳到通知区域）"
        @click.stop="closeWindow"
      >
        <svg class="win-caption__glyph" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0L0 10" />
        </svg>
      </button>
    </div>

    <!-- 拖拽热区：macOS 由左上角的按钮组在流内占位，Windows 上按钮是绝对
         定位的，需要这一层补上「非按钮区域都可拖拽」。 -->
    <span v-if="isWindowsTitlebar" class="mac-titlebar__drag" data-tauri-drag-region aria-hidden="true"></span>

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
