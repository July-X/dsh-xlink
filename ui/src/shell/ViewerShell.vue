<script setup>
// 三扇壳自有副窗（日志 / 模型用量 / 套餐用量）共用的外层：自绘标题栏 + 圆角容器。
//
// 独立成组件而不是三处各写一遍（2026-10-08）。起因是这四件事**必须同时成立**，
// 而它们的理由分散在三个不同的文件里：
//   1. Rust 建窗时 `decorations(false)`，所以这四扇窗都没有系统标题栏，交通灯
//      必须由前端画（与主壳同一份视觉）；
//   2. 建窗同时开了 `transparent`，所以窗口底色必须**在这个容器里**而不是留在
//      body——留在 body 上圆角外的四角会是实色方块；
//   3. `overflow: hidden` + 圆角让内容按圆角裁切；
//   4. 投影画在容器自己的 `::after` 上，因为 `overflow: hidden` 会把画在容器
//      本身的阴影一起裁掉。
//
// 少做其中任何一件的**症状都不报错**：窗口只是「方角」「没有交通灯」或「圆角
// 看不见」，dev server / typecheck / build 全绿。集中在这里是为了让下一次改动
// 只改一处。
//
// 官网页签栏（`?chatstrip=1`）**不用**这个壳：它承载 `chat.deepseek.com` 等
// 别人的页面，Rust 侧保留了原生装饰。
import WindowTitleBar from './WindowTitleBar.vue';

defineProps({
  /** 根容器的额外类名：各副窗自己的布局类挂在这里（`logwin` / `usagewin` …）。 */
  shellClass: { type: String, required: true },
  /** 窗口标题栏正中显示的字（「日志」/「模型用量」/「套餐用量」）。 */
  title: { type: String, required: true },
});
</script>

<template>
  <div class="viewer-shell" :class="shellClass">
    <WindowTitleBar :title="title" />
    <slot />
  </div>
</template>

<style scoped>
/* 圆角 + 裁切在这一层，不在各自的根类里：三扇窗的根类只管内部布局，
   让它们各自声明圆角就等于三份各写一遍、且与 #app 的圆角口径迟早漂。 */
.viewer-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-height: 0;
  overflow: hidden;
  border-radius: var(--window-radius);
  background-color: var(--window);
}
</style>