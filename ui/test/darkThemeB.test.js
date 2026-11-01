// 方案 B「夜航日志」的暗色契约。浅色 Kami 的断言留在 designAlignment.test.js；
// 这里只钉暗色：黄铜/鼠尾草 token、宋体标题、透明账页卡、页号。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const theme = readFileSync(resolve('ui/src/theme.css'), 'utf8');
const dark = theme.match(/html\.dark\s*\{([\s\S]*?)\n\}/)[1];
const root = theme.match(/:root\s*\{([\s\S]*?)\n\}/)[1];
const declaration = (block, name) => block.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1].trim();

test('暗色 B 的画布/黄铜/鼠尾草 token', () => {
  assert.equal(declaration(dark, '--canvas'), '#0a100f');
  assert.equal(declaration(dark, '--surface'), '#111a17');
  assert.equal(declaration(dark, '--accent'), '#d8a84e');
  assert.equal(declaration(dark, '--success'), '#8fbfae');
  assert.equal(declaration(root, '--font-serif'), '"Songti SC", "STSong", "Noto Serif CJK SC", Georgia, serif');
});

test('暗色页标题是宋体 31px，浅色仍是 24px', () => {
  const darkTitle = theme.match(/html\.dark \.page-title\s*\{([^}]*)\}/)[1];
  assert.match(darkTitle, /font-family:\s*var\(--font-serif\)/);
  assert.match(darkTitle, /font-size:\s*31px/);
  assert.match(darkTitle, /font-weight:\s*600/);
  const lightTitle = theme.match(/\n\.page-title\s*\{([^}]*)\}/)[1];
  assert.match(lightTitle, /font-size:\s*24px/);
  assert.match(lightTitle, /font-weight:\s*700/);
});

test('暗色卡片收成透明账页段，不再有圆角和卡片投影', () => {
  const card = theme.match(/html\.dark \.main-view \.card\s*\{([^}]*)\}/)[1];
  assert.match(card, /background:\s*transparent/);
  assert.match(card, /border:\s*0/);
  assert.match(card, /border-radius:\s*0/);
  assert.match(card, /box-shadow:\s*0 1px 0 var\(--border-soft\)/);
});

test('八个面板都带页号，暗色用等宽序号显示', () => {
  const pages = [
    ['ui/src/shell/OverviewPanel.vue', '01'],
    ['ui/src/usage/UsageAnalysisPanel.vue', '02'],
    ['ui/src/kernel/VersionsPanel.vue', '03'],
    ['ui/src/plugins/PluginsPanel.vue', '04'],
    ['ui/src/skills/SkillsPanel.vue', '05'],
    ['ui/src/memory/MemoryPanel.vue', '06'],
    ['ui/src/shell/SettingsPanel.vue', '07'],
    ['ui/src/migration/MigrationPanel.vue', '08'],
  ];
  for (const [file, no] of pages) {
    assert.match(readFileSync(resolve(file), 'utf8'), new RegExp(`class="page-head__text" data-page="${no}"`), file);
  }
  assert.match(theme, /html\.dark \.page-head__text::before\s*\{[^}]*content:\s*attr\(data-page\)/s);
});
