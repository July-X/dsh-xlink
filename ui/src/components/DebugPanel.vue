<script setup>
// dev 调试浮按钮 + 弹层。
//
// 仅在 dev 构建里渲染（`store.view.dev_build`），**release 预览打开后浮按钮仍然
// 保留**——它是切回 dev 外观的唯一入口，藏掉它就只能重启应用了。它提供「在 dev
// 里模拟正式版外观」的开关：
//   - 打开开关会把 rel-build 类挂到 body 上，复用 theme.css 那条 body.rel-build
//     背景渐变规则（sidebar 在 release 期透明、让 body 的绿贯穿整窗），让 dev 期
//     即可看到 release 版的顶部 50% 绿色背景渐变（背景层、不糊内容、整窗连贯）；
//   - 同时收起 dev 专属入口：设置页的「模拟一次任务完成」、概览页版本号的
//     「（dev）」后缀（都读 `store.devUi`），浮按钮自身按状态换色以示区分；
//   - 预览期间面板只留这个开关：钩子速查之类的参考信息属于 dev 外观，不跟着进去。
//   - 真正的状态变更走 store.setReleasePreview()，非 dev 构建会被拒绝（不污染
//     正式版）。预览只改界面，不改 Rust 侧行为（`dev_build` 仍是 true）。
//
// 设计原则：不依赖 Tauri 桥、不发起 IO、不写本地文件；纯 UI 状态切换。
import { computed, ref } from 'vue';
import { store, setReleasePreview } from '../store.js';
import { toast } from '../notify.js';

const open = ref(false);

// 浮按钮只看 dev 构建（不看 store.devUi）：release 预览里也必须留着一个能切回去
// 的入口。正式版 store.view.dev_build===false，整块不渲染。
const isDevBuild = computed(() => !!store.view?.dev_build || store.devUi);
const previewing = computed(() => !!store.releasePreview);

const fabTitle = computed(() =>
  previewing.value
    ? 'dev 调试面板 · release 预览中（打开可切回 dev 外观）'
    : 'dev 调试面板 · 仅 dev 构建可见'
);

// release 预览开关：通过 store 动作改，poll 同步类不会冲掉用户的覆盖。
const previewRel = computed({
  get: () => !!store.releasePreview,
  set: (v) => setReleasePreview(v),
});

/// 切换预览：结果直接由浮按钮配色与页面底色体现，只补一条轻提示说明去向。
///
/// **不要在这里（或它触发的 watch 里）收起面板**：Element Plus 的 switch 在
/// `handleChange` 里先 `emit(CHANGE_EVENT)`、再 `nextTick(() => input.value.checked
/// = checked.value)`，而那个 `input` 是组件模板 ref——同步卸载开关会让下一 tick 的
/// 回调拿到 `null`，抛出的 TypeError 落在 Promise 里，表现为控制台一条
/// `Unhandled Promise Rejection`（chunk-2FGMZUWD.js 的 `input.value.checked`）。
/// 面板保持展开既躲开了这个上游 bug，也让用户能立刻看到开关状态、随手切回去。
function togglePreview(value) {
  setReleasePreview(value);
  toast(value ? '已进入 release 预览（右下角浮按钮可切回）' : '已退出 release 预览');
}

function toggle() {
  open.value = !open.value;
}

function close() {
  open.value = false;
}
</script>

<template>
  <div v-if="isDevBuild" class="debug-fab-wrap">
    <button
      class="debug-fab"
      :class="{ open, preview: previewing }"
      type="button"
      :aria-expanded="open"
      :aria-label="fabTitle"
      :title="fabTitle"
      @click="toggle"
    >
      <span aria-hidden="true">{{ open ? '×' : '🪛' }}</span>
    </button>

    <section
      v-if="open"
      class="debug-panel"
      role="dialog"
      aria-label="dev 调试面板"
      @click.stop
    >
      <header>
        <h3>dev 调试</h3>
        <span class="debug-hint">{{ previewing ? 'release 预览中' : '仅 dev 构建可见' }}</span>
        <button
          class="debug-close"
          type="button"
          aria-label="关闭"
          title="关闭"
          @click="close"
        >
          ×
        </button>
      </header>

      <div class="debug-row">
        <div class="debug-row-text">
          <strong>模拟正式版外观</strong>
          <small class="debug-meta">
            绿渐变底色，并隐藏「模拟一次任务完成」与版本号的「（dev）」后缀。
          </small>
        </div>
        <el-switch :model-value="previewRel" size="small" @change="togglePreview" />
      </div>

      <!-- 参考信息属于 dev 外观：预览期间面板只留上面那个开关，避免把它当成
           release 里也存在的东西。 -->
      <template v-if="!previewing">
        <details class="debug-hooks">
          <summary>release-only 钩子速查</summary>
          <ul>
            <li>
              <code>body.rel-build</code>
              <span>绿只画 body；sidebar 透明 → 整窗一片连贯</span>
            </li>
            <li>
              <code>store.devUi</code>
              <span>dev_build=false 或开了预览 → dev 专属入口一起收起</span>
            </li>
            <li>
              <code>OverviewPanel.vue</code>
              <span>桌面端版本号去掉「（dev）」后缀</span>
            </li>
            <li>
              <code>SettingsPanel.vue</code>
              <span>隐藏「模拟一次任务完成」自检按钮</span>
            </li>
          </ul>
        </details>

        <footer class="debug-foot">
          <span>本面板仅本地调试用，不会读写文件、不触发 IO；预览只改界面，不改外壳行为。</span>
        </footer>
      </template>
    </section>
  </div>
</template>
