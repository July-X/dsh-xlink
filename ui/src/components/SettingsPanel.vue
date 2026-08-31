<script setup>
// 设置：Web UI 端口、插件接线 profile 名、Node 检测，以及内置补丁（内核补丁 / 小插件）
// 的应用与撤销。轮询每 2.5s 刷新 store.view，但用户正在编辑的输入框不被回写（focus 守卫）。
import { computed, reactive, ref, watch } from 'vue';
import { ArrowDown, ArrowUp, Check, Monitor, Refresh } from '@element-plus/icons-vue';
import { store, detectNode, saveSettings } from '../store.js';
import { patchStore, refreshPatches, applyPatch, revertPatch } from '../patches.js';
import { isLoading, withLoading } from '../loading.js';

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

// 进入设置页时刷新补丁状态（内核激活版本可能已变化）。
watch(
  () => store.activePanel,
  (panel) => {
    if (panel === 'settings') refreshPatches();
  },
  { immediate: true }
);

const defaultNodeHint = computed(() => {
  const n = store.view && store.view.node;
  if (!n) return '';
  return n.ok ? 'node ' + n.version + ' 满足 dsh 要求（^22.19 || >=24）' : n.reason;
});

const hintText = computed(() => nodeHint.value || defaultNodeHint.value);

// 工作台运行期间禁止应用 / 撤销补丁（会写入内核目录）。
const workbenchRunning = computed(() => !!(store.view && store.view.kernel && store.view.kernel.running));

const patchRows = computed(() => (patchStore.view && patchStore.view.patches) || []);

// 已并入官方内核的补丁默认折叠。用户在卡片上点击「展开查看」后把 id 加进 Set 里；
// 切换内核或刷新时仍保持原折叠态（除非用户主动点回「收起」），避免误点展开影响阅读。
const obsoleteExpanded = reactive(new Set());
function toggleObsolete(id) {
  if (obsoleteExpanded.has(id)) obsoleteExpanded.delete(id);
  else obsoleteExpanded.add(id);
}
function isObsoleteCollapsed(row) {
  return Boolean(row.superseded) && !obsoleteExpanded.has(row.id);
}

function rangeText(row) {
  if (row.min_kernel_version && row.max_kernel_version) {
    return 'v' + row.min_kernel_version + ' ~ v' + row.max_kernel_version;
  }
  if (row.min_kernel_version) return 'v' + row.min_kernel_version + ' 及以上';
  if (row.max_kernel_version) return 'v' + row.max_kernel_version + ' 及以下';
  return '任意内核版本';
}

// 状态徽标配色：未应用 / 不适用 / 无内核 → info；已应用 → success；
// 文件未命中 → warning；文件被改动 → danger。
function stateTag(state) {
  if (state === 'applied') return 'success';
  if (state === 'partial') return 'warning';
  if (state === 'dirty') return 'danger';
  return 'info';
}

// 状态决定主操作：已应用 / 部分应用 / 文件被改动 → 撤销；其余 → 应用。
function primaryAction(row) {
  return row.state === 'applied' || row.state === 'partial' || row.state === 'dirty'
    ? 'revert'
    : 'apply';
}

function onDetectNode() {
  const info = detectNode();
  if (info) {
    info.then((result) => {
      if (result) {
        nodeHint.value = result.ok ? '检测结果：' + result.path + '  ' + result.version : result.reason;
      }
    });
  }
}

function onRefreshPatches() {
  withLoading('patchRefresh', () => refreshPatches());
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

    <div class="card">
      <div class="card-head">
        <h2>内核补丁（内置）</h2>
        <el-button text size="small" :icon="Refresh" :loading="isLoading('patchRefresh')"
          @click="onRefreshPatches">
          刷新
        </el-button>
      </div>
      <p class="muted" style="margin: 0">
        随 dsh-xlink 发布包内置的自研内核补丁与小插件，默认不生效；由你选择是否应用到当前内核，
        应用前自动备份原文件，可随时撤销。仅影响当前激活的内核版本。
      </p>
      <div v-if="!patchStore.loaded" class="patch-empty">补丁状态加载中…</div>
      <div v-else-if="!patchRows.length" class="patch-empty">此版本的 dsh-xlink 未携带任何内置补丁。</div>
      <div v-else class="patch-list">
        <div v-for="row in patchRows" :key="row.id" class="patch-item"
             :class="{ 'patch-item-obsolete': row.superseded, 'patch-item-collapsed': isObsoleteCollapsed(row) }">
          <div class="patch-item-main">
            <div class="patch-item-title">
              <strong :class="{ 'patch-name-obsolete': row.superseded }">{{ row.name }}</strong>
              <el-tag v-if="row.kind === 'plugin'" size="small" type="success">内置插件</el-tag>
              <el-tag v-else size="small" type="primary">补丁</el-tag>
              <el-tag size="small" type="info" effect="plain">v{{ row.version }}</el-tag>
              <el-tag v-if="row.superseded && row.superseded_since_kernel_version"
                       size="small" type="info" effect="plain" class="patch-superseded-tag">
                已并入官方内核 v{{ row.superseded_since_kernel_version }} 起
              </el-tag>
              <el-tag :type="stateTag(row.state)" size="small" effect="dark" class="patch-state">
                {{ row.state_text }}
              </el-tag>
              <el-button v-if="row.superseded" link size="small" class="patch-toggle"
                          @click="toggleObsolete(row.id)">
                <el-icon><component :is="isObsoleteCollapsed(row) ? ArrowDown : ArrowUp" /></el-icon>
                {{ isObsoleteCollapsed(row) ? '展开查看' : '收起' }}
              </el-button>
            </div>
            <p v-if="isObsoleteCollapsed(row) && row.superseded_since_kernel_version"
               class="muted patch-desc patch-obsolete-summary">
              官方内核 v{{ row.superseded_since_kernel_version }} 起已包含本补丁的修复，无需手动应用。
            </p>
            <template v-else>
              <p class="muted patch-desc">{{ row.description }}</p>
              <p class="patch-meta">
                <span>适用范围：{{ rangeText(row) }}</span>
                <span v-if="row.applied_at">应用时间：{{ row.applied_at }}</span>
              </p>
              <p v-if="row.note" class="patch-note">{{ row.note }}</p>
            </template>
          </div>
          <div v-if="!isObsoleteCollapsed(row)" class="patch-item-actions">
            <el-button
              v-if="primaryAction(row) === 'apply'"
              type="primary"
              size="small"
              :disabled="!row.enabled || workbenchRunning"
              :loading="isLoading('patchApply:' + row.id)"
              @click="applyPatch(row.id, row.name)"
            >
              应用到当前内核
            </el-button>
            <el-button
              v-else
              type="danger"
              plain
              size="small"
              :disabled="!row.enabled || workbenchRunning"
              :loading="isLoading('patchRevert:' + row.id)"
              @click="revertPatch(row.id, row.name)"
            >
              撤销补丁
            </el-button>
          </div>
        </div>
      </div>
      <p v-if="workbenchRunning" class="patch-note">
        工作台运行期间不能应用或撤销补丁，请先关闭工作台后再操作。
      </p>
    </div>
  </section>
</template>