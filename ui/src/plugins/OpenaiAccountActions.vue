<script setup>
import { computed, onMounted, onUnmounted } from 'vue';
import { accountStore, accountText, followAccount, refreshAccount, runAccountAction } from './openaiAccount.js';
import { isLoading } from '../shell/loading.js';

defineProps({ pluginView: { type: Object, required: true } });
const phase = computed(() => accountStore.status?.phase);
const busy = computed(() => isLoading('openaiAccountAction'));
let dispose;
onMounted(() => { dispose = followAccount(); });
onUnmounted(() => dispose?.());
</script>

<template>
  <div class="account-actions">
    <span class="account-status" role="status" aria-live="polite">{{ accountText() }}</span>
    <span class="account-buttons">
      <el-button v-if="!accountStore.status || accountStore.readError" size="small" :loading="isLoading('openaiAccountRefresh')" :disabled="busy" @click="refreshAccount">重试状态</el-button>
      <el-button v-if="phase === 'authorizing'" size="small" :loading="busy" @click="runAccountAction('openai_authorize_cancel')">取消登录</el-button>
      <template v-else-if="accountStore.status">
        <el-button v-if="phase !== 'authorized'" size="small" :loading="busy" :disabled="!!accountStore.readError || !pluginView.pluginSourceAvailable || !!pluginView.stateError" @click="runAccountAction('openai_authorize_start')">登录 ChatGPT</el-button>
        <el-button v-if="phase === 'authorized'" size="small" :loading="busy" :disabled="!!accountStore.readError" @click="runAccountAction('openai_catalog_refresh')">刷新模型列表</el-button>
        <el-button v-if="phase === 'authorized' || phase === 'reauth-required'" size="small" :loading="busy" :disabled="!!accountStore.readError" @click="runAccountAction('openai_logout')">退出登录</el-button>
      </template>
    </span>
    <span v-if="accountStore.readError || accountStore.error || accountStore.status?.lastError" class="account-error" role="alert">{{ accountStore.readError || accountStore.error || accountStore.status.lastError }}</span>
  </div>
</template>

<style scoped>
.account-actions { display: grid; gap: 4px; max-width: 330px; }
.account-status { color: var(--text-secondary); font-size: 12px; overflow-wrap: anywhere; }
.account-buttons { display: flex; flex-wrap: wrap; gap: 6px; }
.account-buttons :deep(.el-button + .el-button) { margin-left: 0; }
.account-error { color: var(--danger); font-size: 12px; overflow-wrap: anywhere; }
</style>
