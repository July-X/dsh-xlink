<script setup>
import { onMounted, onUnmounted } from 'vue';
import { listen } from '../shell/bridge.js';
import { isLoading } from '../shell/loading.js';
import { toastActionError } from '../shell/notify.js';
import { subscription, setCodexUsageEnabled, syncCodexUsageConsent, isProviderRefreshing } from './subscription.js';

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
  <div class="codex-usage-consent">
    <el-switch
      :model-value="subscription.codexUsageEnabled"
      :loading="isLoading('codexUsageConsent')"
      :disabled="subscription.loading || isProviderRefreshing('openai_codex') || isLoading('codexUsageConsent')"
      active-text="使用本机 Codex 登录查询额度"
      aria-label="使用本机 Codex 登录查询额度"
      @change="setCodexUsageEnabled"
    />
    <p class="muted">默认关闭。开启后只读 Codex 登录文件并校验同一账号，展示 Codex 额度；不修改或刷新凭据，也不改变 DSH 模型登录。</p>
  </div>
</template>

<style scoped>
.codex-usage-consent p { margin: 2px 0 6px; font-size: 11px; }
</style>
