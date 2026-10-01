<script setup>
// 技能页：已安装技能包（来源 / 落地模式 / 版本 / 包内技能启停 + 更新 / 重新同步 /
// 仓库 / 卸载）+ 手动安装（git 来源，回车即装）。安装与卸载对运行中的工作台
// 即时生效。
//
// 排版目标：包信息与动作分区；版本 tag 与包名同行，包级说明收敛为单行并通过
// Tooltip 承载全文；逐技能开关放在包头，包体保持简洁。
import { computed } from 'vue';
import { Refresh, InfoFilled, Download, TopRight, Delete, Link, DocumentCopy } from '@element-plus/icons-vue';
import {
  skillStore,
  originLabel,
  installSkill,
  updateSkill,
  uninstallSkill,
  setSkillEnabled,
  moveAsideShadowedSkills,
  moveAsideConflictingSkills,
  checkSkillUpdates,
} from './skills.js';
import { openExternalLink, confirmDialog } from '../shell/notify.js';
import { tildePath } from '../shell/labels.js';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';

const view = computed(() => skillStore.view);

// 被更高优先级的根盖住的活动条目。判据在后端，UI 只负责把它变成一个可点的
// 出路——告警文案是给人读的，不能让前端去解析那句话来决定显不显示按钮。
const shadowed = computed(() => (view.value && view.value.shadowed) || []);
const SHADOW_KEY = 'moveAsideShadowed';

// 确认框必须列出会被动到哪几条：这是一次改名（不是删除），但落点在壳平时
// 不写的地方，用户点确认之前有权先看见。
// 文案是纯文本——`confirmDialog` 没开 dangerouslyUseHTMLString，写 Markdown
// 星号会原样显示出来。
// 两个「改名让路」确认框共用的那句：改的是名不是内容。两个对话框只差
// 「为什么移」与「移走之后做什么」，那段共用的话不必各写一遍——上一次就是
// 因为各写一遍才出现逐字重复。
const RENAME_NOTICE = '下面这些条目会被改名（文件名加时间戳后缀），不会删除；改回原名即可恢复：';

async function moveAsideShadowed() {
  const lines = shadowed.value.map((e) => tildePath(e.path) + '（盖住 ' + e.skill + '）');
  const ok = await confirmDialog(
    '移走被盖住的条目？',
    RENAME_NOTICE + '\n' +
      lines.join('\n') +
      '\n\n移走后，壳管理的启停与更新才会对它们生效。',
    '改名让路'
  );
  if (!ok) return;
  await withLoading(SHADOW_KEY, moveAsideShadowedSkills);
}

// 活动视图里被同名条目占住、启用必然失败的条目。与 shadowed 同一条纪律：判据
// 在后端（view.conflicts），这里只把路径与来由摆到确认框里，不去解析告警文案。
// 这条曾是一个只有「关闭」的死胡同——判据已经精确到具体文件，按钮却不存在。
const conflicts = computed(() => (view.value && view.value.conflicts) || []);
const CONFLICT_KEY = 'moveAsideConflicts';

async function moveAsideConflicts() {
  const lines = conflicts.value.map(
    (e) => tildePath(e.path) + '（' + e.detail + '）'
  );
  const ok = await confirmDialog(
    '移走占位的同名条目？',
    '下面这些条目占着技能在活动视图里的位置，但它们不归技能库所有，所以启用一定失败。\n' +
      RENAME_NOTICE + '\n' +
      lines.join('\n') +
      '\n\n移走之后回到上面的开关上点「启用」即可。',
    '改名让路'
  );
  if (!ok) return;
  await withLoading(CONFLICT_KEY, moveAsideConflictingSkills);
}

// 技能启停：面板此前只提供安装/卸载/更新/重新同步，启停虽然有完整的后端能力
// （skill_set_enabled + enabled:false 语义 + 启动对账），却只能在面板外直接调命令。
// 粒度是「包里的单个技能」——停用比整包卸载轻，条目留在中央库、随时可恢复。
function skillEnabledKey(id, name) {
  return `skillEnabled:${id}:${name}`;
}

function toggleSkill(row, skill, enabled) {
  return withLoading(skillEnabledKey(row.id, skill.name), () =>
    setSkillEnabled(row.id, skill.name, enabled)
  );
}

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进标题旁的
// 信息气泡；两条路径都由后端返回（`store_root` 中央库 / `skills_root` 活动视图），
// 并按迁移面板的同一套规则把 home 折叠成 `~`。这里曾经把 `~/.dsh/skills-store/`
// 写死在前端，而 P5 之后中央库早已搬到 `skills/packages/`——后端搬了家、提示还
// 指着旧目录，正是「同一份路径在前端与 Rust 各写一遍」的必然结果。
const storeTip = computed(() => {
  const v = view.value || {};
  const store = tildePath(v.store_root) || '~/.dsh-xlink/skills/packages/';
  const active = tildePath(v.skills_root) || '~/.dsh-xlink/skills/active/';
  return (
    '技能统一存放于 ' + store +
    '，以链接方式进入内核读取的 ' + active +
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
      <!-- 警告文案自带可执行的下一步（清单损坏、清单缺失、被同名条目盖住…），
           模板**不要**再统一追加「重启应用会自动修复」：那对「清单缺失」是假的
           （reconcile 不会凭空重建 store.json），对「被盖住」更是假的（重启什么
           都不会改变内核的 rank 次序）——一句对两条都不成立的建议，比没有更糟。 -->
      <el-alert
        v-if="view && view.warning"
        :title="view.warning"
        type="warning"
        :closable="false"
        show-icon
      />
      <!-- 告警的出路。按钮只在判据非空时出现（`view.shadowed` / `view.conflicts`），
           文案自带的「删掉上面列出的条目」是兜底——判据与路径都由后端给出。 -->
      <div v-if="shadowed.length" class="shadow-actions">
        <span class="muted">
          这 {{ shadowed.length }} 份盖住了壳管理的同名条目，移走前请确认下面列出的路径。
        </span>
        <el-button
          size="small"
          type="warning"
          plain
          :loading="isLoading(SHADOW_KEY)"
          :disabled="globalBusy"
          @click="moveAsideShadowed"
        >
          移走被盖住的条目
        </el-button>
      </div>
      <div v-if="conflicts.length" class="shadow-actions">
        <span class="muted">
          这 {{ conflicts.length }} 个技能的位置被占着，现在点启用一定会失败，移走前请确认下面列出的路径。
        </span>
        <el-button
          size="small"
          type="warning"
          plain
          :loading="isLoading(CONFLICT_KEY)"
          :disabled="globalBusy"
          @click="moveAsideConflicts"
        >
          移走冲突条目
        </el-button>
      </div>

      <div class="entity-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
        <el-empty v-if="!view || !view.rows || view.rows.length === 0" description="尚未安装任何技能包。" :image-size="48" />
        <div v-for="row in view ? view.rows : []" :key="row.id" class="entity-row skill-entity-row">
          <div class="skill-row-main">
            <div class="entity-head">
              <span class="entity-name">{{ row.name }}</span>
              <span class="origin-chip" :class="'origin-chip-' + row.origin">{{ originLabel(row.origin) }}</span>
              <span v-if="row.actual_mode === 'copy'" class="mode-chip">
                <el-icon class="mode-chip-icon"><DocumentCopy /></el-icon>复制
              </span>
              <span v-else-if="row.actual_mode === 'link'" class="mode-chip mode-chip-link">
                <el-icon class="mode-chip-icon"><Link /></el-icon>链接
              </span>
              <span class="meta-version">{{ row.installed_version || '—' }}</span>
            </div>
            <el-tooltip v-if="row.description" placement="top" effect="dark" :content="row.description">
              <span class="entity-desc skill-package-description">{{ row.description }}</span>
            </el-tooltip>
            <div
              v-if="row.latest_version || (row.pinned && row.origin !== 'local')"
              class="skill-package-meta"
            >
              <span v-if="row.latest_version" class="meta-upgrade">→ {{ row.latest_version }}</span>
              <span v-if="row.pinned && row.origin !== 'local'" class="meta-pinned">已锁定版本</span>
            </div>
          </div>
          <div class="entity-actions skill-row-actions">
            <template v-for="skill in row.skills" :key="'skill-control-' + skill.name">
              <el-tag v-if="skill.enabled && !skill.present" size="small" type="warning" effect="plain">
                条目缺失
              </el-tag>
              <el-switch
                :model-value="skill.enabled"
                :loading="isLoading(skillEnabledKey(row.id, skill.name))" :disabled="globalBusy"
                size="small" :aria-label="(skill.enabled ? '停用 ' : '启用 ') + skill.name"
                @change="(value) => toggleSkill(row, skill, value)"
              />
            </template>
            <el-tooltip v-if="row.latest_version" :content="'更新到 ' + row.latest_version" placement="top" effect="dark">
              <el-button
                class="entity-action entity-action-update"
                size="small"
                type="primary"
                circle
                :icon="Download"
                :aria-label="'更新技能 ' + row.name + ' 到 ' + row.latest_version"
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
                :aria-label="'重新同步 ' + row.name"
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
                :aria-label="'在浏览器打开 ' + row.name + ' 的仓库'"
                :disabled="globalBusy"
                @click="openExternalLink(row.repo_url, '仓库地址')"
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
                  :aria-label="'卸载技能包 ' + row.name"
                  :disabled="globalBusy"
                />
              </template>
            </el-popconfirm>
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

<style scoped>
/* 告警与它的出路排成一行：告警条本身要占满宽度，按钮跟在下面一行右侧，
   不去挤 `el-alert` 的可点区域。 */
.shadow-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 8px;
  font-size: 12px;
}
</style>
