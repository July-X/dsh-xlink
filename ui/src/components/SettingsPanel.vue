<script setup>
// 设置：Web UI 端口、插件接线 profile 名、Node 检测。
// 轮询每 2.5s 刷新 store.view，但用户正在编辑的输入框不被回写（focus 守卫）。
import { computed, ref, watch } from 'vue';
import { Check, Monitor } from '@element-plus/icons-vue';
import { store, detectNode, saveSettings } from '../store.js';
import { isLoading } from '../loading.js';

const port = ref(undefined);
const profile = ref('');
const editing = ref(false);
const nodeHint = ref('');

// 初始与外部变化时回写输入框；用户正在编辑（editing）时跳过，避免输入被回滚。
watch(
  () => store.view && store.view.settings,
  (settings) => {
    if (!settings || editing.value) return;
    port.value = settings.port;
    profile.value = settings.profile || '';
  },
  { immediate: true }
);

const defaultNodeHint = computed(() => {
  const n = store.view && store.view.node;
  if (!n) return '';
  return n.ok ? 'node ' + n.version + ' 满足 dsh 要求（^22.19 || >=24）' : n.reason;
});

const hintText = computed(() => nodeHint.value || defaultNodeHint.value);

async function onDetectNode() {
  const info = await detectNode();
  if (info) {
    nodeHint.value = info.ok ? '检测结果：' + info.path + '  ' + info.version : info.reason;
  }
}

function onSave() {
  saveSettings(port.value, profile.value);
}
</script>

<template>
  <section class="panel">
    <div class="card">
      <h2>设置</h2>
      <el-form label-width="140px" label-position="left" @focusin="editing = true" @focusout="editing = false">
        <el-form-item label="Web UI 端口">
          <el-input-number v-model="port" :min="1024" :max="65535" :precision="0" controls-position="right" />
        </el-form-item>
        <el-form-item label="插件接线 profile 名">
          <!-- 固定值，不允许修改：保存设置时原样回传当前配置（默认 web）。 -->
          <code class="profile-fixed">{{ profile || 'web' }}</code>
        </el-form-item>
      </el-form>
      <div class="btn-row">
        <el-button type="primary" :icon="Check" :loading="isLoading('saveSettings')" @click="onSave">
          保存设置
        </el-button>
        <el-button text :icon="Monitor" :loading="isLoading('detectNode')" @click="onDetectNode">
          检测 Node.js
        </el-button>
      </div>
      <p class="muted" style="margin: 0">{{ hintText }}</p>
    </div>
  </section>
</template>
