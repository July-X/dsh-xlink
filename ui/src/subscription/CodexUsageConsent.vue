<script setup>
import { onMounted, onUnmounted } from 'vue';
import { listen } from '../shell/bridge.js';
import { isLoading } from '../shell/loading.js';
import { toastActionError } from '../shell/notify.js';
import {
  CODEX_USAGE_CONSENT_TIP,
  subscription,
  setCodexUsageEnabled,
  syncCodexUsageConsent,
  isProviderRefreshing,
} from './subscription.js';

/**
 * `inline`：只渲染开关本身，说明句交给调用方。
 *
 * 概览「套餐用量」把开关放进**卡头右侧**（设计稿 `.card-head` 的位置：标题在左，
 * 开关与「刷新 / 查看详情」同排），于是说明句必须由那张卡自己画在卡头下面
 * 整宽一行——组件若还包着它，开关与它的说明会被塞进卡头右半栏挤成一团。
 * 独立套餐用量窗口保持默认竖排（一列宽，开关与说明本来就是上下两行）。
 */
const props = defineProps({ inline: { type: Boolean, default: false } });

let disposed = false;
let unlisten;
onMounted(() => {
  listen('codex-usage-consent-changed', ({ payload }) => {
    if (!isLoading('codexUsageConsent')) syncCodexUsageConsent(payload);
  }).then((stop) => {
    if (disposed) stop?.();
    else unlisten = stop;
  }).catch((error) => toastActionError('额度开关跨窗同步不可用', error, '下次刷新会重新读取磁盘许可'));
});
onUnmounted(() => { disposed = true; unlisten?.(); });
</script>

<template>
  <div class="codex-usage-consent" :class="{ 'codex-usage-consent--inline': props.inline }">
    <el-switch
      :model-value="subscription.codexUsageEnabled"
      :loading="isLoading('codexUsageConsent')"
      :disabled="subscription.loading || isProviderRefreshing('openai_codex') || isLoading('codexUsageConsent')"
      active-text="使用本机 Codex 登录查询额度"
      aria-label="使用本机 Codex 登录查询额度"
      @change="setCodexUsageEnabled"
    />
    <p v-if="!props.inline" class="muted">{{ CODEX_USAGE_CONSENT_TIP }}</p>
  </div>
</template>

<style scoped>
.codex-usage-consent p { margin: 2px 0 6px; font-size: 11px; }
/* 卡头里那一档：开关与它的 inline 文案同行，不换行、不占额外高度——卡头高度
   由标题那枚 24px 方块决定，开关一旦折行就会把整行卡头撑高。 */
.codex-usage-consent--inline { display: inline-flex; align-items: center; white-space: nowrap; }
.codex-usage-consent--inline :deep(.el-switch__label) { margin-left: 6px; }
</style>
