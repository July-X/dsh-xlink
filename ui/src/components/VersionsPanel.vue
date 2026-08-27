<script setup>
// 内核版本：左列已安装（切换 / 删除），右列 npm 发布（安装 / 切换）。
// 「检查更新」从 npm registry 拉取版本列表。
import { computed } from 'vue';
import { Refresh, Download, Promotion, Delete } from '@element-plus/icons-vue';
import { store, checkUpdates, installVersion, activateVersion, removeVersion } from '../store.js';
import { globalBusy, isLoading } from '../loading.js';

const kernel = computed(() => store.view && store.view.kernel);

const installedVersions = computed(() => {
  const set = new Set();
  if (kernel.value) {
    kernel.value.installed.forEach((v) => set.add(v.version));
  }
  return set;
});
</script>

<template>
  <section class="panel">
    <div class="card">
      <div class="card-head">
        <h2>内核版本</h2>
        <span class="head-meta">
          <span class="muted">已安装 {{ kernel ? kernel.installed.length : '—' }} 个</span>
          <el-button text :icon="Refresh" :loading="isLoading('checkUpdates')" @click="checkUpdates">
            检查更新
          </el-button>
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
                <el-tag v-if="v.active" type="success" size="small" effect="dark">当前使用</el-tag>
                <el-popconfirm
                  v-else
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
              </span>
            </div>
          </div>
        </div>

        <div class="list-group">
          <h3 class="list-head-with-logo">
            <img class="brand-logo" src="https://avatars.githubusercontent.com/u/6078720?s=200&v=4" alt="npm" />
            <span>npm 发布</span>
          </h3>
          <div class="release-list">
            <p v-if="store.releases.length === 0" class="muted" style="margin: 0">
              点击「检查更新」获取官方发布列表。
            </p>
            <div v-for="r in store.releases" :key="r.version" class="release-row">
              <span class="release-ver">{{ r.version }}</span>
              <span class="release-actions">
                <el-tag v-if="kernel && kernel.active === r.version" type="success" size="small" effect="dark">
                  当前使用
                </el-tag>
                <el-tag v-else-if="installedVersions.has(r.version)" size="small" effect="plain">已安装</el-tag>
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
                <el-button
                  v-else-if="!kernel || kernel.active !== r.version"
                  size="small"
                  :icon="Promotion"
                  :loading="isLoading('activate:' + r.version)"
                  :disabled="globalBusy"
                  @click="activateVersion(r.version)"
                >
                  切换
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
