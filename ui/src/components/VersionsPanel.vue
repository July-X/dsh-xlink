<script setup>
// 内核版本：左列已安装（切换 / 删除），右列 npm 发布（仅安装）。
// 「切换」必须基于本地已安装版本，避免误把尚未安装的远端版本当成可立刻启用的内核。
// 「检查更新」从 npm registry 拉取版本列表。
//
// 面板挂载时主动调一次 refreshAll()，让「已安装」列表在用户进到这一页时就是最新的，
// 而不是要等启动阶段的 get_status，或者「检查更新」之后才看到本地版本。
import { computed, onMounted, reactive } from 'vue';
import { Refresh, Download, Promotion, Delete, InfoFilled } from '@element-plus/icons-vue';
import {
  store,
  refreshAll,
  checkUpdates,
  installVersion,
  activateVersion,
  removeVersion,
} from '../store.js';
import { invoke } from '../bridge.js';
import { globalBusy, isLoading } from '../loading.js';
import VersionPluginsTip from './VersionPluginsTip.vue';

const kernel = computed(() => store.view && store.view.kernel);

// 每个已安装内核的插件快照只在 Tooltip 即将显示时读取，避免页面初次
// 渲染就为所有内核发起 IPC。已成功读取的版本会复用缓存。
const versionPlugins = reactive({});

function versionPluginSlot(version) {
  if (!versionPlugins[version]) {
    versionPlugins[version] = {
      loading: false,
      loaded: false,
      error: null,
      rows: [],
    };
  }
  return versionPlugins[version];
}

async function loadVersionPlugins(version) {
  const slot = versionPluginSlot(version);
  if (slot.loaded || slot.loading) return;

  slot.loading = true;
  slot.error = null;
  try {
    slot.rows = (await invoke('kernel_plugin_list', { version })) || [];
    slot.loaded = true;
  } catch (e) {
    slot.error = e && e.message ? e.message : String(e);
    slot.loaded = true;
  } finally {
    slot.loading = false;
  }
}

const emptyPluginSnapshot = Object.freeze({
  loading: false,
  loaded: false,
  error: null,
  rows: [],
});

function pluginSnapshot(version) {
  return versionPlugins[version] || emptyPluginSnapshot;
}

const installedVersions = computed(() => {
  const set = new Set();
  if (kernel.value) {
    kernel.value.installed.forEach((v) => set.add(v.version));
  }
  return set;
});

// 进版本面板就重新扫描本地内核列表，与 npm 发布列解耦——
onMounted(() => {
  refreshAll();
});
</script>

<template>
  <section class="panel">
    <div class="card">
      <div class="card-head">
        <h2>内核版本</h2>
        <span class="head-meta">
          <span class="muted">已安装 {{ kernel ? kernel.installed.length : '—' }} 个</span>
        </span>
      </div>

      <el-alert v-if="store.releaseWarning" :title="store.releaseWarning" type="warning" :closable="false" show-icon />

      <div class="updates-lists">
        <div class="list-group">
          <h3>已安装</h3>
          <div class="installed-list">
            <el-empty v-if="!kernel || kernel.installed.length === 0" description="尚未安装任何内核。" :image-size="64" />
            <div v-for="v in kernel ? kernel.installed : []" :key="v.version" class="installed-row">
              <span class="release-ver">{{ v.version }}</span>
              <span class="release-actions">
                <el-tooltip
                  effect="dark"
                  popper-class="kernel-plugin-tooltip"
                  placement="right-start"
                  :fallback-placements="['left-start', 'bottom-start', 'top-start']"
                  :boundaries-padding="12"
                  trigger="hover"
                  :show-after="160"
                  :hide-after="120"
                  :offset="8"
                  :show-arrow="true"
                  @before-show="loadVersionPlugins(v.version)"
                >
                  <button
                    type="button"
                    class="installed-tip-trigger"
                    :aria-label="'查看 ' + v.version + ' 的插件'"
                  >
                    <el-icon class="installed-tip-icon"><InfoFilled /></el-icon>
                  </button>
                  <template #content>
                    <VersionPluginsTip :snapshot="pluginSnapshot(v.version)" :version="v.version" />
                  </template>
                </el-tooltip>
                <el-tag v-if="v.active" type="success" size="small" effect="dark">当前使用</el-tag>
                <template v-else>
                  <el-button
                    size="small"
                    :icon="Promotion"
                    :loading="isLoading('activate:' + v.version)"
                    :disabled="globalBusy"
                    @click="activateVersion(v.version)"
                  >
                    切换
                  </el-button>
                  <el-popconfirm
                    title="确认删除该版本？"
                    confirm-button-text="删除"
                    cancel-button-text="取消"
                    width="200"
                    @confirm="removeVersion(v.version)"
                  >
                    <template #reference>
                      <el-button
                        size="small"
                        type="danger"
                        plain
                        :icon="Delete"
                        :loading="isLoading('remove:' + v.version)"
                        :disabled="globalBusy"
                      >
                        删除
                      </el-button>
                    </template>
                  </el-popconfirm>
                </template>
              </span>
            </div>
          </div>
        </div>

        <div class="list-group">
          <h3 class="list-head-with-logo">
            <img class="brand-logo" src="https://avatars.githubusercontent.com/u/6078720?s=200&v=4" alt="npm" />
            <span>npm 发布</span>
            <el-button class="release-check-button" text :icon="Refresh" :loading="isLoading('checkUpdates')" :disabled="globalBusy" @click="checkUpdates">
              检查更新
            </el-button>
          </h3>
          <div class="release-list">
            <p v-if="store.releases.length === 0" class="muted" style="margin: 0">
              点击「检查更新」获取官方发布列表。
            </p>
            <div v-for="r in store.releases" :key="r.version" class="release-row">
              <span class="release-ver">{{ r.version }}</span>
              <span class="release-actions">
                <el-tag v-if="installedVersions.has(r.version)" size="small" effect="plain">已安装</el-tag>
                <el-button
                  v-if="!installedVersions.has(r.version)"
                  size="small"
                  type="primary"
                  :icon="Download"
                  :disabled="globalBusy"
                  @click="installVersion(r.version)"
                >
                  安装
                </el-button>
                <el-tag v-if="r.prerelease" type="info" size="small" effect="plain">预发布</el-tag>
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
