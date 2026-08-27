<script setup>
// 技能页：已安装技能包（来源 / 版本 / 技能数、更新 / 重新同步 / 卸载）+
// 手动安装（git 来源，回车即装）。安装与卸载对运行中的工作台即时生效。
import { computed } from 'vue';
import { Refresh, InfoFilled, Download, TopRight, Delete } from '@element-plus/icons-vue';
import {
  skillStore,
  originLabel,
  installSkill,
  updateSkill,
  uninstallSkill,
  checkSkillUpdates,
} from '../skills.js';
import { openExternal } from '../bridge.js';
import { globalBusy, isLoading } from '../loading.js';

const view = computed(() => skillStore.view);

function metaText(row) {
  const pinNote = row.pinned && row.origin !== 'local' ? ' · 已锁定版本' : '';
  const upgrade = row.latest_version ? ' → ' + row.latest_version : '';
  return (
    originLabel(row.origin) +
    ' · ' +
    (row.installed_version || '—') +
    upgrade +
    pinNote +
    ' · ' +
    row.skills.length +
    ' 个技能'
  );
}
</script>

<template>
  <section class="panel">
    <div class="card">
      <div class="card-head">
        <h2>已安装</h2>
        <span class="head-meta">
          <el-button
            text
            :icon="Refresh"
            :loading="isLoading('checkSkillUpdates')"
            @click="checkSkillUpdates({ busy: true, toastOnUpdates: true })"
          >
            检查更新
          </el-button>
        </span>
      </div>
      <p class="muted" style="margin: 0">
        技能统一存放于 <code>~/.dsh/skills-store/</code>，以链接方式进入内核读取的
        <code>~/.dsh/skills/</code>（链接失败自动降级复制）；安装与卸载对运行中的工作台<b>即时生效</b>，无需重启。
      </p>
      <el-alert
        v-if="view && view.warning"
        :title="view.warning + '（重启应用会自动修复；也可尝试重新安装对应技能包）'"
        type="warning"
        :closable="false"
        show-icon
      />

      <div class="installed-list">
        <el-empty v-if="!view || !view.rows || view.rows.length === 0" description="尚未安装任何技能包。" :image-size="64" />
        <div v-for="row in view ? view.rows : []" :key="row.id" class="installed-row plugin-row">
          <span class="skill-row-head">
            <span class="plugin-info">
              <span class="release-ver">{{ row.name }}</span>
              <span class="plugin-meta">{{ metaText(row) }}</span>
              <span v-if="row.description" class="plugin-meta">{{ row.description }}</span>
            </span>
            <span class="release-actions plugin-actions">
              <el-tag v-if="row.actual_mode === 'copy'" size="small" effect="plain">复制</el-tag>
              <el-tag v-else-if="row.actual_mode === 'link'" type="success" size="small" effect="plain">链接</el-tag>
              <template v-if="row.latest_version">
                <el-tag type="warning" size="small" effect="plain">有更新 {{ row.latest_version }}</el-tag>
                <el-button size="small" type="primary" :icon="Download" :disabled="globalBusy" @click="updateSkill(row.id)">更新</el-button>
              </template>
              <el-button v-else-if="row.origin === 'local'" size="small" text :icon="Refresh" :disabled="globalBusy" @click="updateSkill(row.id)">
                重新同步
              </el-button>
              <el-button v-if="row.repo_url" size="small" text :icon="TopRight" @click="openExternal(row.repo_url)">仓库</el-button>
              <el-popconfirm
                title="确认卸载该技能包？"
                confirm-button-text="卸载"
                cancel-button-text="取消"
                width="200"
                @confirm="uninstallSkill(row.id)"
              >
                <template #reference>
                  <el-button size="small" type="danger" plain :icon="Delete" :disabled="globalBusy">卸载</el-button>
                </template>
              </el-popconfirm>
            </span>
          </span>
        </div>
      </div>

      <h3 class="section-divider">
        手动安装
        <el-tooltip placement="top" effect="dark">
          <template #content>
            支持以下 git 来源：<br />
            · 仓库地址：https://github.com/owner/repo.git<br />
            · GitHub 简写：owner/repo<br />
            · 追加 #tag 可锁定版本
          </template>
          <el-icon class="muted" style="cursor: help"><InfoFilled /></el-icon>
        </el-tooltip>
      </h3>
      <div class="install-row">
        <el-input
          v-model="skillStore.spec"
          placeholder="输入后按回车键开始安装"
          spellcheck="false"
          clearable
          @keyup.enter="installSkill"
        >
          <template #suffix>
            <span class="muted" title="按 Enter 开始安装">↵</span>
          </template>
        </el-input>
      </div>
      <p class="muted" style="margin: 0">
        也可以在
        <a href="https://github.com/topics/dsh-skill" target="_blank" rel="noreferrer">GitHub dsh-skill topic</a>
        浏览社区资源，把 git 仓库地址粘贴到上方手动安装。
      </p>
    </div>
  </section>
</template>
