<script setup>
import { Trash2 as Trash2Icon } from '@lucide/vue';
import { isLoading } from '../shell/loading.js';
import { removeDiagnosedPlugin } from './diagnostic-actions.js';

const props = defineProps({
  pluginId: { type: String, required: true },
});

const emit = defineEmits(['cleaned']);

async function remove() {
  const ok = await removeDiagnosedPlugin(props.pluginId);
  if (ok) emit('cleaned');
}
</script>

<template>
  <el-popconfirm
    title="确认移除并清理这个插件？"
    confirm-button-text="移除并清理"
    cancel-button-text="取消"
    width="260"
    @confirm="remove"
  >
    <template #reference>
      <el-button
        type="danger"
        plain
        :icon="Trash2Icon"
        :loading="isLoading('diagnosticPluginCleanup')"
      >
        移除并清理
      </el-button>
    </template>
  </el-popconfirm>
</template>
