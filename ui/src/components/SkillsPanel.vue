<script setup>
// 技能页：已安装技能包（来源 / 落地模式 / 版本 / 技能数 + 更新 / 重新同步 /
// 仓库 / 卸载）+ 手动安装（git 来源，回车即装）。安装与卸载对运行中的工作台
// 即时生效。
//
// 排版目标：一行一包、单行信息 + 单行描述，动作按钮固定在最右侧一列，
// 多行之间版本号与按钮纵向对齐；行高不做两级堆叠，避免每个包占掉三行。
import { computed } from 'vue';
import { Refresh, InfoFilled, Download, TopRight, Delete, Link, DocumentCopy } from '@element-plus/icons-vue';
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

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进标题旁的
// 信息气泡；路径优先用后端返回的真实根目录，拿不到时回退到约定路径。
const storeTip = computed(() => {
  const root = (view.value && view.value.skills_root) || '~/.dsh/skills/';
  return (
    '技能统一存放于 ~/.dsh/skills-store/，以链接方式进入内核读取的 ' +
    root +
    '（链接失败自动降级复制）。安装与卸载对运行中的工作台即时生效，无需重启。'
  );
});

// 模式（链接 / 复制）与来源（npm / git / local）都是只读状态，与名称同行展示；
// 动作区只保留真正可点的按钮，卸载与其它动作靠一条竖分隔线隔开。
</script>

<template>
  <section class="panel">
    <div class="card entity-card">
      <div class="card-head">
        <h2 class="card-head-with-tip">
          <span>已安装</span>
          <el-tooltip placement="top" effect="dark" :content="storeTip">
            <el-icon class="head-tip-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </h2>
        <span class="head-meta">
          <span v-if="view && view.updates" class="muted">{{ view.updates }} 个可更新</span>
          <el-button
            text
            :icon="Refresh"
            :loading="isLoading('checkSkillUpdates')"
            :disabled="globalBusy"
            @click="checkSkillUpdates({ busy: true, toastOnUpdates: true })"
          >
            检查更新
          </el-button>
        </span>
      </div>
      <el-alert
        v-if="view && view.warning"
        :title="view.warning + '（重启应用会自动修复；也可尝试重新安装对应技能包）'"
        type="warning"
        :closable="false"
        show-icon
      />

      <div class="entity-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
        <el-empty v-if="!view || !view.rows || view.rows.length === 0" description="尚未安装任何技能包。" :image-size="48" />
        <div v-for="row in view ? view.rows : []" :key="row.id" class="entity-row">
          <div class="entity-head">
            <span class="entity-name">{{ row.name }}</span>
            <span class="origin-chip" :class="'origin-chip-' + row.origin">{{ originLabel(row.origin) }}</span>
            <span v-if="row.actual_mode === 'copy'" class="mode-chip">
              <el-icon class="mode-chip-icon"><DocumentCopy /></el-icon>复制
            </span>
            <span v-else-if="row.actual_mode === 'link'" class="mode-chip mode-chip-link">
              <el-icon class="mode-chip-icon"><Link /></el-icon>链接
            </span>
            <el-tooltip v-if="row.description" placement="top" effect="dark" :content="row.description">
              <span class="entity-desc">{{ row.description }}</span>
            </el-tooltip>
          </div>
          <div class="entity-foot">
            <dl class="entity-meta">
              <span class="meta-version">{{ row.installed_version || '—' }}</span>
              <span v-if="row.latest_version" class="meta-upgrade">→ {{ row.latest_version }}</span>
              <span v-if="row.pinned && row.origin !== 'local'" class="meta-pinned">已锁定版本</span>
              <span class="meta-skill-count">{{ row.skills.length }} 个技能</span>
            </dl>
            <div class="entity-actions">
              <el-tooltip v-if="row.latest_version" :content="'更新到 ' + row.latest_version" placement="top" effect="dark">
                <el-button
                  class="entity-action entity-action-update"
                  size="small"
                  type="primary"
                  circle
                  :icon="Download"
                  :disabled="globalBusy"
                  @click="updateSkill(row.id)"
                />
              </el-tooltip>
              <el-tooltip v-else-if="row.origin === 'local'" content="重新同步本地目录" placement="top" effect="dark">
                <el-button
                  class="entity-action"
                  size="small"
                  circle
                  :icon="Refresh"
                  :disabled="globalBusy"
                  @click="updateSkill(row.id)"
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
                title="确认卸载该技能包？"
                confirm-button-text="卸载"
                cancel-button-text="取消"
                width="200"
                @confirm="uninstallSkill(row.id)"
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

      <h3 class="section-divider">
        手动安装
        <el-tooltip placement="top" effect="dark">
          <template #content>
            支持以下 git 来源：<br />
            · 仓库地址：https://github.com/owner/repo.git<br />
            · GitHub 简写：owner/repo<br />
            · 追加 #tag 可锁定版本
          </template>
          <el-icon class="head-tip-icon"><InfoFilled /></el-icon>
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
      <p class="install-hint">
        也可以在
        <a href="https://github.com/topics/dsh-skill" target="_blank" rel="noreferrer">GitHub dsh-skill topic</a>
        浏览社区资源，把 git 仓库地址粘贴到上方手动安装。
      </p>
    </div>
  </section>
</template>
