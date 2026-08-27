<script setup>
// 插件页：已安装列表（同步 / 接线 / 隔离状态徽章 + 更新 / 模式切换 / 卸载）、
// 手动安装（回车即装）、插件中心（分类筛选 + 搜索 + 排序 + 分页卡片）。
import { computed, ref, watch } from 'vue';
import { Refresh, Switch, Delete, TopRight, Download, RefreshLeft, ArrowDown } from '@element-plus/icons-vue';
import {
  pluginStore,
  CATALOG_CATEGORIES,
  CATALOG_PAGE,
  categoryLabel,
  formatCount,
  formatUpdated,
  installedKeys,
  isInstalled,
  filteredCatalog,
  loadCatalog,
  installPlugin,
  updatePlugin,
  setPluginMode,
  resolvePluginQuarantine,
  syncPlugins,
  uninstallPlugin,
  checkPluginUpdates,
  openExternal,
} from '../plugins.js';
import { globalBusy, isLoading } from '../loading.js';

const view = computed(() => pluginStore.view);

// --- 已安装列表 ---

function metaText(row) {
  const pinNote = row.pinned ? ' · 已锁定版本' : '';
  const installed = (row.origin === 'npm' ? 'npm' : 'git') + ' · ' + row.installed_version;
  const upgrade = row.latest_version ? ' → ' + row.latest_version : '';
  return installed + upgrade + pinNote;
}

function quarantineNote(row) {
  const reason = String(row.quarantined.reason || '');
  return '已隔离：' + (reason.length > 60 ? reason.slice(0, 57) + '…' : reason);
}

// --- 插件中心 ---

const keys = computed(() => installedKeys());
const items = computed(() => filteredCatalog(keys.value));
const shownItems = computed(() => items.value.slice(0, pluginStore.shown));
const hasMore = computed(() => items.value.length > pluginStore.shown);

const countText = computed(() => {
  const total = pluginStore.catalogItems.length;
  if (!total) return '';
  const verified = pluginStore.catalogItems.filter((i) => i.verified).length;
  return '结果 ' + items.value.length + ' 条 · 收录 ' + total + ' 款 · 已验证 ' + verified + ' 款';
});

const catChips = computed(() => {
  const counts = new Map();
  pluginStore.catalogItems.forEach((item) => counts.set(item.category, (counts.get(item.category) || 0) + 1));
  const chips = [{ id: 'all', label: '全部', count: pluginStore.catalogItems.length }];
  CATALOG_CATEGORIES.forEach(([id, label]) => {
    if (counts.get(id)) chips.push({ id, label, count: counts.get(id) });
  });
  counts.forEach((count, id) => {
    if (id && !CATALOG_CATEGORIES.some(([key]) => key === id)) chips.push({ id, label: id, count });
  });
  return chips;
});

function pickCategory(id) {
  pluginStore.category = id;
  pluginStore.shown = CATALOG_PAGE;
}

function showMore() {
  pluginStore.shown += CATALOG_PAGE;
}

// 搜索输入 150ms 防抖；排序 / 筛选变更立即重置分页。
let queryTimer = null;
watch(
  () => pluginStore.query,
  () => {
    clearTimeout(queryTimer);
    queryTimer = setTimeout(() => {
      pluginStore.shown = CATALOG_PAGE;
    }, 150);
  }
);
watch([() => pluginStore.sort, () => pluginStore.filter], () => {
  pluginStore.shown = CATALOG_PAGE;
});

function detailUrl(item) {
  return item.detail_url || (item.repo ? 'https://github.com/' + item.repo : '');
}

function descText(item) {
  const d = item.description || '';
  return d.length > 140 ? d.slice(0, 137) + '…' : d;
}

function statsText(item) {
  const parts = [];
  if (item.stars > 0) parts.push('★ ' + formatCount(item.stars));
  if (item.forks > 0) parts.push('Fork ' + formatCount(item.forks));
  const updated = formatUpdated(item.updated);
  if (updated) parts.push(updated);
  return parts.join(' · ');
}
</script>

<template>
  <section class="panel">
    <div class="card">
      <div class="card-head">
        <h2>已安装</h2>
        <span class="head-meta">
          <el-button text :icon="Switch" :disabled="globalBusy" @click="syncPlugins">同步到所有内核</el-button>
          <el-button
            text
            :icon="Refresh"
            :loading="isLoading('checkPluginUpdates')"
            @click="checkPluginUpdates({ busy: true, toastOnUpdates: true })"
          >
            检查更新
          </el-button>
        </span>
      </div>
      <p class="muted" style="margin: 0">
        插件统一存放于 <code>~/.dsh/plugins/</code>，切换内核无需重装；安装完成后自动校验是否符合
        dsh 插件规范，内核重启后生效。
      </p>
      <el-alert
        v-if="view && view.warning"
        :title="view.warning + '（可在「日志」侧查看 plugin-wiring.log）'"
        type="warning"
        :closable="false"
        show-icon
      />

      <div class="installed-list">
        <el-empty v-if="!view || !view.rows || view.rows.length === 0" description="尚未安装任何插件。" :image-size="64" />
        <div v-for="row in view ? view.rows : []" :key="row.id" class="installed-row plugin-row">
          <span class="plugin-info">
            <span class="release-ver">{{ row.name }}</span>
            <span class="plugin-meta">{{ metaText(row) }}</span>
            <span v-if="row.quarantined" class="plugin-meta quarantine-note">{{ quarantineNote(row) }}</span>
          </span>
          <span class="release-actions plugin-actions">
            <el-tag v-if="row.actual_mode === 'copy'" size="small" effect="plain">复制</el-tag>
            <el-tag v-else-if="row.actual_mode === 'link'" type="success" size="small" effect="plain">链接</el-tag>
            <el-tag v-if="row.synced && row.wired" type="success" size="small" effect="plain">已同步</el-tag>
            <el-tag v-else-if="view && view.active_kernel && !row.synced" type="warning" size="small" effect="plain">
              待同步
            </el-tag>
            <el-tag v-if="!row.wired && view && view.active_kernel" type="warning" size="small" effect="plain">
              待接线
            </el-tag>
            <el-tag v-if="!view || !view.active_kernel" type="warning" size="small" effect="plain">无活动内核</el-tag>
            <template v-if="row.quarantined">
              <el-tag type="warning" size="small" effect="dark">已停用</el-tag>
              <el-button size="small" text :icon="RefreshLeft" :disabled="globalBusy" @click="resolvePluginQuarantine(row.id, 'enable')">
                恢复启用
              </el-button>
            </template>
            <template v-if="row.latest_version">
              <el-tag type="warning" size="small" effect="plain">有更新 {{ row.latest_version }}</el-tag>
              <el-button v-if="!row.pinned" size="small" type="primary" :icon="Download" :disabled="globalBusy" @click="updatePlugin(row.id)">
                更新
              </el-button>
            </template>
            <!-- 物化模式开关：开=复制、关=链接；切换走 plugin_set_mode 长任务，
                 状态以 row.desired_mode 为准，命令完成刷新后才翻转。 -->
            <el-switch
              :model-value="row.desired_mode === 'copy'"
              inline-prompt
              active-text="复制"
              inactive-text="链接"
              :disabled="globalBusy"
              style="width: 58px; flex-shrink: 0"
              @change="(v) => setPluginMode(row.id, v ? 'copy' : 'link')"
            />
            <el-button v-if="row.repo_url" size="small" text :icon="TopRight" @click="openExternal(row.repo_url)">仓库</el-button>
            <el-popconfirm
              title="确认卸载该插件？"
              confirm-button-text="卸载"
              cancel-button-text="取消"
              width="200"
              @confirm="uninstallPlugin(row.id)"
            >
              <template #reference>
                <el-button size="small" type="danger" plain :icon="Delete" :disabled="globalBusy">卸载</el-button>
              </template>
            </el-popconfirm>
          </span>
        </div>
      </div>

      <h3 class="section-divider">手动安装</h3>
      <div class="install-row">
        <el-input
          v-model="pluginStore.spec"
          placeholder="npm i @scope/pkg · 也支持 owner/repo、dsh add"
          spellcheck="false"
          clearable
          @keyup.enter="installPlugin('')"
        >
          <template #suffix>
            <span class="muted" title="按 Enter 开始安装">↵</span>
          </template>
        </el-input>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <h2 class="card-head-with-logo">
          <img class="brand-logo" src="https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png" alt="GitHub" />
          <span>插件中心</span>
        </h2>
        <span class="head-meta">
          <span class="muted">{{ countText }}</span>
          <el-button text :icon="Refresh" :loading="isLoading('catalogReload')" @click="loadCatalog(true)">
            刷新目录
          </el-button>
        </span>
      </div>
      <p class="muted" style="margin: 0">
        来自 <a href="https://dsh-plugin.org" target="_blank" rel="noreferrer">dsh-plugin.org</a>
        社区目录，点击「安装」即可装到本机插件库并接入所有内核。
      </p>
      <div class="install-row">
        <el-input v-model="pluginStore.query" placeholder="搜索插件名称、描述、标签…" spellcheck="false" clearable />
        <el-select v-model="pluginStore.sort" style="max-width: 130px" title="排序">
          <el-option value="stars" label="Star 最多" />
          <el-option value="updated" label="最近更新" />
        </el-select>
        <el-select v-model="pluginStore.filter" style="max-width: 120px" title="安装状态">
          <el-option value="all" label="全部" />
          <el-option value="installed" label="已安装" />
          <el-option value="not-installed" label="未安装" />
        </el-select>
      </div>

      <div class="catalog-cats">
        <button
          v-for="chip in catChips"
          :key="chip.id"
          type="button"
          class="cat-chip"
          :class="{ active: pluginStore.category === chip.id }"
          @click="pickCategory(chip.id)"
        >
          {{ chip.label }}
          <span v-if="chip.count" class="cat-count">{{ chip.count }}</span>
        </button>
      </div>

      <div v-if="!pluginStore.catalogLoaded" v-loading="true" style="min-height: 120px" element-loading-text="目录加载中…"></div>
      <p v-else-if="items.length === 0" class="muted" style="margin: 0">
        {{ pluginStore.catalogItems.length ? '没有匹配的插件，换个关键词或分类试试。' : '目录为空或加载失败，点「刷新目录」重试。' }}
      </p>
      <TransitionGroup v-else name="catalog" tag="div" class="catalog-list">
        <div
          v-for="(item, index) in shownItems"
          :key="item.spec || item.name"
          class="catalog-card"
          :style="{ '--i': index }"
        >
          <div class="catalog-card-head">
            <span class="catalog-title">
              <span class="catalog-name">{{ item.name }}</span>
              <el-tag v-if="item.version" size="small" effect="plain">{{ item.version }}</el-tag>
              <el-tag v-if="item.category" type="info" size="small" effect="plain">{{ categoryLabel(item.category) }}</el-tag>
              <el-tag v-if="item.verified" type="success" size="small" effect="plain">已验证</el-tag>
            </span>
            <span class="catalog-stats">{{ statsText(item) }}</span>
          </div>
          <p v-if="item.description" class="catalog-desc">{{ descText(item) }}</p>
          <div class="catalog-card-foot">
            <span class="catalog-tags">
              <el-tag v-for="tag in (item.tags || []).slice(0, 4)" :key="tag" size="small" effect="plain" type="info">
                {{ tag }}
              </el-tag>
            </span>
            <span class="catalog-actions">
              <el-button v-if="detailUrl(item)" size="small" text :icon="TopRight" @click="openExternal(detailUrl(item))">
                打开详情
              </el-button>
              <el-button v-if="isInstalled(item, keys)" size="small" disabled>已安装</el-button>
              <el-button v-else size="small" type="primary" :icon="Download" :disabled="globalBusy" @click="installPlugin(item.spec)">
                安装
              </el-button>
            </span>
          </div>
        </div>
      </TransitionGroup>

      <div v-if="hasMore" class="catalog-more">
        <el-button text :icon="ArrowDown" @click="showMore">显示更多</el-button>
      </div>
    </div>
  </section>
</template>
