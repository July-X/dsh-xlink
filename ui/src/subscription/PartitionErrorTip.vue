<script setup>
import { computed } from 'vue';
import { InfoFilled } from '@element-plus/icons-vue';
import { providerShortState, providerStateText, errorLivesInPartitionTip } from './subscription.js';

/**
 * 分区短状态（「查询异常」等）+ 完整原因的点击 tooltip。
 *
 * 为什么独立成组件（2026-10-10）：OpenAI 的失败几乎全是「为什么查不到 + 下一步
 * 怎么开」的长解释，用户要求不再每次刷新都顶一整条横幅，而是收进「查询异常」
 * 后方的 ⓘ、点击弹出。概览卡与独立套餐用量窗口两处同一形态，模板与样式收在
 * 这里各渲染一份，避免两处漂移；错误归属分区的判据是
 * [`errorLivesInPartitionTip`]（openai_codex），其余 provider 的完整文案仍走
 * 顶部横幅，调用方自行渲染不带 ⓘ 的短状态。
 */
const props = defineProps({ provider: { type: Object, default: null } });

const short = computed(() => providerShortState(props.provider));
const detail = computed(() =>
  errorLivesInPartitionTip(props.provider?.id) ? providerStateText(props.provider) : null
);
</script>

<template>
  <p
    v-if="short"
    class="partition-error"
    :class="{ 'partition-error--bad': provider.fetch_error || provider.credential_status === 'expired' || provider.error }"
  >
    {{ short }}<el-tooltip v-if="detail" trigger="click" placement="top-start" :show-after="80">
      <template #content>
        <div class="card-info-tooltip">{{ detail }}</div>
      </template>
      <el-icon class="partition-error__more" role="button" aria-label="查看具体原因"><InfoFilled /></el-icon>
    </el-tooltip>
  </p>
</template>

<style scoped>
.partition-error {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
.partition-error--bad {
  color: var(--el-color-danger);
}
/* 「查询异常」后的 ⓘ：完整原因收进点击 tooltip。 */
.partition-error__more {
  margin-left: 4px;
  font-size: 12px;
  vertical-align: -2px;
  cursor: pointer;
  opacity: 0.75;
}
.partition-error__more:hover {
  opacity: 1;
}
</style>
