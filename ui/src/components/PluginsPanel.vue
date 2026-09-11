<script setup>
// 插件页：已安装列表（同步 / 接线 / 隔离状态徽章 + 更新 / 模式切换 / 卸载）、
// 手动安装（回车即装）、插件中心（分类筛选 + 搜索 + 排序 + 分页卡片）。
import { computed, onUnmounted, watch } from 'vue';
import {
  Refresh,
  Switch,
  Delete,
  TopRight,
  Download,
  RefreshLeft,
  ArrowDown,
  Box,
  Link,
  InfoFilled,
  WarningFilled,
  Warning,
} from '@element-plus/icons-vue';
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
import { originLabel } from '../labels.js';
import { globalBusy, isLoading } from '../loading.js';

const view = computed(() => pluginStore.view);

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进标题旁的
// 信息气泡，与技能页保持一致。
const installTip =
  '插件统一存放于 ~/.dsh/plugins/，切换内核无需重装；安装完成后自动校验是否符合 ' +
  'dsh 插件规范，内核重启后生效。';

// --- 已安装列表 ---

function quarantineNote(row) {
  const reason = String(row.quarantined.reason || '');
  return '已隔离：' + (reason.length > 60 ? reason.slice(0, 57) + '…' : reason);
}

// 待同步 / 待接线的原因：只在有活动内核时才成立（没有内核时状态 chip 已经
// 说明「无活动内核」），返回文案用于动作区的警示点 tooltip。
function syncWarning(row) {
  if (!view.value || !view.value.active_kernel) return '';
  if (!row.synced) return '待同步：活动内核中的物化副本与当前版本不一致，点「同步到所有内核」修复';
  if (!row.wired) return '待接线：活动内核的 profile 尚未加载该插件，点「同步到所有内核」修复';
  return '';
}

// 当前物化模式：活动核心里已落地时以实际模式为准，否则回落到中央库记录的
// 期望模式（此时 actual_mode 为空，旧 UI 在这种情况下不显示任何模式标签）。
function currentMode(row) {
  if (row.synced && row.actual_mode) return row.actual_mode;
  return row.desired_mode === 'copy' ? 'copy' : 'link';
}

function modeTip(row) {
  return currentMode(row) === 'link' ? '当前为链接模式，点击改为复制' : '当前为复制模式，点击改为链接';
}

function togglePluginMode(row) {
  return setPluginMode(row.id, currentMode(row) === 'link' ? 'copy' : 'link');
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
      queryTimer = null;
      pluginStore.shown = CATALOG_PAGE;
    }, 150);
  }
);
watch([() => pluginStore.sort, () => pluginStore.filter], () => {
  pluginStore.shown = CATALOG_PAGE;
});

onUnmounted(() => {
  if (queryTimer) clearTimeout(queryTimer);
  queryTimer = null;
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
    <!-- 第三方来源的免责提示放在这里：它是插件页的语境，原先常驻在侧栏底部，
         在概览 / 内核版本等页面也会一直占着位置。 -->
    <div class="panel-notice" role="note">
      <el-icon><Warning /></el-icon>
      <p>第三方插件由社区提供，本工具不对其安全性负责，请自行甄别。</p>
    </div>
    <div class="card entity-card">
      <div class="card-head">
        <h2 class="card-head-with-tip">
          <span>已安装</span>
          <el-tooltip placement="top" effect="dark" :content="installTip">
            <el-icon class="head-tip-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </h2>
        <span class="head-meta">
          <el-button text :icon="Switch" :disabled="globalBusy" @click="syncPlugins">同步到所有内核</el-button>
          <el-button
            text
            :icon="Refresh"
            :loading="isLoading('checkPluginUpdates')"
            :disabled="globalBusy"
            @click="checkPluginUpdates({ busy: true, toastOnUpdates: true })"
          >
            检查更新
          </el-button>
        </span>
      </div>
      <el-alert
        v-if="view && view.warning"
        :title="view.warning + '（可在「日志」侧查看 plugin-wiring.log）'"
        type="warning"
        :closable="false"
        show-icon
      />

      <div class="entity-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
        <el-empty v-if="!view || !view.rows || view.rows.length === 0" description="尚未安装任何插件。" :image-size="48" />
        <div
          v-for="row in view ? view.rows : []"
          :key="row.id"
          class="entity-row"
          :class="{ 'is-warn': !!row.quarantined }"
        >
          <div class="entity-head">
            <el-tooltip v-if="row.quarantined" placement="top" effect="dark" :content="quarantineNote(row)">
              <span class="entity-warn"><el-icon><WarningFilled /></el-icon> 已停用</span>
            </el-tooltip>
            <span class="entity-name">{{ row.name }}</span>
            <span class="origin-chip" :class="'origin-chip-' + row.origin">
              <el-icon class="origin-chip-icon">
                <Box v-if="row.origin === 'npm'" />
                <Link v-else />
              </el-icon>
              <span class="origin-chip-label">{{ originLabel(row.origin) }}</span>
            </span>
            <el-tooltip v-if="row.description" placement="top" effect="dark" :content="row.description">
              <span class="entity-desc">{{ row.description }}</span>
            </el-tooltip>
          </div>
          <div class="entity-foot">
            <dl class="entity-meta">
              <span class="meta-version">{{ row.installed_version }}</span>
              <span v-if="row.latest_version" class="meta-upgrade">→ {{ row.latest_version }}</span>
              <span v-if="row.pinned" class="meta-pinned">已锁定版本</span>
            </dl>
            <span class="entity-states">
              <el-tag v-if="!view || !view.active_kernel" type="warning" size="small" effect="plain">无活动内核</el-tag>
              <el-tag v-else-if="row.synced && row.wired" type="success" size="small" effect="plain">已同步</el-tag>
            </span>
            <!-- 待同步 / 待接线只在动作区点一个警示点，原因走 tooltip：原先把状态
                 铺成整枚文字标签，一行挤三四枚，把行高和右半区一起顶满。 -->
            <span v-if="syncWarning(row)" class="state-dot">
              <el-tooltip placement="top" effect="dark" :content="syncWarning(row)">
                <el-icon><WarningFilled /></el-icon>
              </el-tooltip>
            </span>
            <div class="entity-actions">
              <!-- 物化模式：徽章文案即当前模式，点击切到另一种。切换走
                   plugin_set_mode 长任务，状态以 row.desired_mode 为准，
                   命令完成刷新后才翻转（未落地时回落到中央库记录的模式）。 -->
              <el-tooltip placement="top" effect="dark" :content="modeTip(row)">
                <el-button
                  class="entity-mode"
                  :class="{ 'is-link': currentMode(row) === 'link' }"
                  size="small"
                  :loading="globalBusy"
                  @click="togglePluginMode(row)"
                >
                  {{ currentMode(row) === 'link' ? '链接' : '复制' }}
                </el-button>
              </el-tooltip>
              <el-tooltip v-if="row.quarantined" content="恢复启用" placement="top" effect="dark">
                <el-button
                  class="entity-action"
                  size="small"
                  circle
                  :icon="RefreshLeft"
                  :disabled="globalBusy"
                  @click="resolvePluginQuarantine(row.id, 'enable')"
                />
              </el-tooltip>
              <el-tooltip v-if="row.latest_version && !row.pinned" :content="'更新到 ' + row.latest_version" placement="top" effect="dark">
                <el-button
                  class="entity-action entity-action-update"
                  size="small"
                  type="primary"
                  circle
                  :icon="Download"
                  :disabled="globalBusy"
                  @click="updatePlugin(row.id)"
                />
              </el-tooltip>
              <el-tooltip v-if="row.repo_url" content="打开仓库" placement="top" effect="dark">
                <el-button
                  class="entity-action"
                  size="small"
                  circle
                  :icon="TopRight"
                  :disabled="globalBusy"
                  @click="openExternal(row.repo_url)"
                />
              </el-tooltip>
              <span class="entity-action-sep" aria-hidden="true"></span>
              <el-popconfirm
                title="确认卸载该插件？"
                confirm-button-text="卸载"
                cancel-button-text="取消"
                width="200"
                @confirm="uninstallPlugin(row.id)"
              >
                <template #reference>
                  <el-button
                    class="entity-action entity-action-danger"
                    size="small"
                    circle
                    :icon="Delete"
                    :disabled="globalBusy"
                  />
                </template>
              </el-popconfirm>
            </div>
          </div>
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
          <el-button text :icon="Refresh" :loading="isLoading('catalogReload')" :disabled="globalBusy" @click="loadCatalog(true)">
            刷新目录
          </el-button>
        </span>
      </div>
      <p class="muted" style="margin: 0">
        来自 <a href="https://dshfind.com/zh" target="_blank" rel="noreferrer">dshfind.com</a>
        插件超市目录，点击「安装」即可装到本机插件库并接入所有内核。
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
              <span v-if="item.version" class="catalog-version">{{ item.version }}</span>
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
