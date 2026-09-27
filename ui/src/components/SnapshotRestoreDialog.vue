<script setup>
// 恢复预览弹窗：把「将要失去什么」先摆出来，用户确认后才动手。
//
// 两条命令刻意分开（snapshot_preview_restore / snapshot_restore）：合一
// 意味着要点一次「恢复」才知道后果，而那时已经点了。这正是安全网最容易
// 被用户绕过的地方——他怕的不是恢复失败，是恢复完才发现丢错了东西。
//
// 动不了的条目必须在**确认之前**就标出来。中央库已经删掉的插件、未装回
// 的内核版本、补丁方向都不是这里能自动做的，事后才说等于让用户在不知情
// 的情况下拿到一次不完整的还原。
import { computed } from 'vue';
import { WarningFilled } from '@element-plus/icons-vue';
import {
  snapshotStore,
  closeRestorePreview,
  runRestore,
  diffKindLabel,
  diffHeadline,
  diffBlockedNote,
} from '../snapshots.js';
import { globalBusy, isLoading, withLoading } from '../loading.js';

const diff = computed(() => snapshotStore.pendingDiff);
const changes = computed(() => (diff.value && diff.value.changes) || []);
const restorable = computed(() => changes.value.filter((item) => item.restorable));
const blocked = computed(() => changes.value.filter((item) => !item.restorable));
const nothingToDo = computed(() => changes.value.length === 0);

function confirm() {
  const id = diff.value && diff.value.snapshot_id;
  if (!id) return;
  closeRestorePreview();
  return runRestore(id);
}

// 「仅恢复能恢复的部分」在有动不了的条目时才出现：全都能恢复时，多一个
// 按钮只是让人犹豫。
const canPartial = computed(() => blocked.value.length > 0 && restorable.value.length > 0);

const partial = async () => {
  const id = diff.value && diff.value.snapshot_id;
  if (!id) return;
  await withLoading('snapshotPartialRestore', () => runRestore(id));
};
</script>

<template>
  <el-dialog
    v-model="snapshotStore.restoreVisible"
    title="恢复到选中的回退点"
    width="min(700px, 92vw)"
    append-to-body
    @close="closeRestorePreview"
  >
    <div v-if="diff" class="restore">
      <p class="restore-headline">{{ diffHeadline(diff) }}</p>

      <el-alert
        v-if="diff.current_drifted"
        title="当前环境与任何回退点都不同（可能手工改过插件目录）。下面的差异按你确认的时刻计算。"
        type="info"
        :closable="false"
        show-icon
        class="restore-alert"
      />
      <p v-if="diffBlockedNote(diff)" class="restore-blocked-note">
        {{ diffBlockedNote(diff) }}
      </p>

      <template v-if="!nothingToDo">
        <p v-if="restorable.length" class="restore-section-title">
          将自动完成（{{ restorable.length }}）
        </p>
        <ul v-if="restorable.length" class="restore-list">
          <li v-for="(item, index) in restorable" :key="'r' + index" class="restore-item">
            <el-tag size="small" type="success" effect="plain">
              {{ diffKindLabel(item.kind) }}
            </el-tag>
            <span class="restore-item-detail">{{ item.detail }}</span>
          </li>
        </ul>

        <p v-if="blocked.length" class="restore-section-title">
          不会自动完成（{{ blocked.length }}）
        </p>
        <ul v-if="blocked.length" class="restore-list">
          <li v-for="(item, index) in blocked" :key="'b' + index" class="restore-item">
            <el-tag size="small" type="warning" effect="plain">
              <el-icon><WarningFilled /></el-icon>
              {{ diffKindLabel(item.kind) }}
            </el-tag>
            <span class="restore-item-detail">{{ item.detail }}</span>
          </li>
        </ul>

        <p class="restore-note">
          动手前会自动把当前环境存成一个新的回退点；插件与技能只会被
          <b>停用</b>，不会卸载或删除，随时可以再启用回来。
        </p>
      </template>
    </div>

    <template #footer>
      <el-button @click="closeRestorePreview" :disabled="globalBusy">取消</el-button>
      <el-button
        v-if="canPartial"
        type="primary"
        :loading="isLoading('snapshotPartialRestore')"
        @click="partial"
      >
        只恢复可恢复的 {{ restorable.length }} 项
      </el-button>
      <el-button
        v-else
        type="primary"
        :disabled="nothingToDo || globalBusy"
        @click="confirm"
      >
        确认恢复
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.restore-headline {
  margin: 0 0 12px;
  line-height: 1.7;
  color: var(--text);
}

.restore-alert {
  margin-bottom: 10px;
}

.restore-blocked-note {
  margin: 0 0 12px;
  font-size: 13px;
  color: var(--warn, var(--text-muted));
}

.restore-section-title {
  margin: 14px 0 7px;
  font-size: 13px;
  color: var(--text-muted);
}

.restore-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.restore-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 7px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-soft);
}

.restore-item-detail {
  font-size: 13px;
  line-height: 1.6;
  color: var(--text);
}

.restore-note {
  margin: 16px 0 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--text-muted);
}
</style>
