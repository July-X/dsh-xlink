<script setup>
import { computed, onMounted, ref, watch } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { openExternalLink } from '../shell/notify.js';
import { detectMcode, installMcode, mcode, refreshMcodeStatus, saveMcodeInstallation } from './mcode.js';

const busy = computed(() => globalBusy.value || mcode.loading || isLoading('detectMcode') || isLoading('saveMcodeInstallation'));
const packagePath = ref('');
const nodePath = ref('');
const editing = ref(false);
watch(() => mcode.view, view => {
  if (!view || editing.value) return;
  packagePath.value = view.configured_package_path || '';
  nodePath.value = view.configured_node_path || '';
}, { immediate: true });
async function save() {
  if (await saveMcodeInstallation(packagePath.value, nodePath.value)) editing.value = false;
}
onMounted(() => refreshMcodeStatus());
</script>

<template>
  <div class="card">
    <div class="card-head"><h2>安装与运行环境</h2></div>
    <p>MiniMax Code：{{ mcode.view?.package_version || '未检测到' }} · Node.js：{{ mcode.view?.nodeVersion || '未检测到' }}</p>
    <p v-if="mcode.error || mcode.view?.error" role="alert">{{ mcode.error || mcode.view?.error }}</p>
    <el-button :disabled="busy" :loading="isLoading('detectMcode')" @click="detectMcode">重新检测</el-button>
    <el-button type="primary" :disabled="busy || !!mcode.view?.stop_required" @click="installMcode">安装 MiniMax 0.1.7</el-button>
    <details>
      <summary>手动设置安装位置</summary>
      <label for="mcode-package-path">MiniMax npm 包目录</label>
      <el-input id="mcode-package-path" v-model="packagePath" :disabled="busy || !mcode.view || mcode.view.stop_required" @input="editing = true" />
      <label for="mcode-node-path">Node 可执行文件</label>
      <el-input id="mcode-node-path" v-model="nodePath" :disabled="busy || !mcode.view || mcode.view.stop_required" @input="editing = true" />
      <el-button :disabled="busy || !mcode.view || mcode.view.stop_required" :loading="isLoading('saveMcodeInstallation')" @click="save">保存位置</el-button>
    </details>
    <p>可使用壳内安装或选择已有安装。安装过程会检查运行环境和原生依赖。</p>
    <el-button text @click="openExternalLink('https://code.minimax.cn/docs/cli/webui', 'MiniMax WebUI 安装说明')">查看安装说明</el-button>
  </div>
</template>

<style scoped>
details { margin-top: 12px; }
label { display: block; margin: 10px 0 5px; }
</style>
