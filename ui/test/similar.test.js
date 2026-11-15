// 「疑似同一条的不同说法」的判据：纯函数行为 + 真的接在面板上。
//
// 阈值是按 2026-11-15 本机 36 条记忆回放标定的（见 similar.js 模块文档）：
// 命中暗色主题 3 条与 MiniMax 手写体 2 条；「判定不了的问题」那对只共享套话前缀，
// 抓到它会连带 6 对误报，故意不追——提示宁缺勿滥，不做自动合并。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { dice, looksSimilar, similarCounts, similarPeers } from '../src/memory/similar.js';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');
const WS = '8381a4be-5593-4f6d-b246-c0256fc7580a';
const PATH = '/Users/zhongxingxing/2026/github/dsh-xlink';

const row = (id, title, keywords, extra = {}) => ({
  id,
  title,
  body: extra.body || '说明正文。',
  scope: 'project',
  workspace_id: WS,
  workspace_path: PATH,
  state: 'accepted',
  keywords,
  ...extra,
});

const darkA = row('a', '暗色主题采用方案 B「夜航日志」', ['暗色主题', '方案 B', '夜航日志', 'UI 设计', '多界面验证']);
const darkB = row('b', 'dsh-xlink 暗色主题采用方案 B', ['dsh-xlink', '暗色主题', '方案B', '夜航日志', 'UI设计', '账页式', '黄铜色']);
const darkC = row('c', '暗色主题确定为方案 B「夜航日志」', ['暗色主题', '方案B', '夜航日志', '设计方向', '宋体']);
const fontA = row('d', 'MiniMax 品牌字样使用手写体', ['MiniMax', '手写体', '字体', '--font-hand', '品牌字样', 'UI 约定']);
const fontB = row('e', 'MiniMax 手写体品牌字统一用粉色', ['MiniMax', '手写体', '粉色', '品牌字', 'token', '主题']);
const entry = row('f', '开发环境默认显示 MiniMax 入口', ['MiniMax', 'pnpm run dev', '实验入口', '开发环境', '默认开启']);
const principle = row('g', '多内核 UI 的总原则：内核专属内容才切界面，共享能力用小开关', ['多内核', 'UI 设计', '开关', '心智负担', 'dsh', 'minimax', '共享能力']);
const notify = row('h', '「任务通知」面向多内核，UI 必须标出通知来源', ['任务通知', '多内核', 'MiniMax', 'DSH', '通知来源', '来源标识', '设置页']);

test('dice：相同为 1、空串为 0、归一化忽略空白与标点', () => {
  assert.equal(dice('暗色主题方案B', '暗色主题方案B'), 1);
  assert.equal(dice('', '暗色主题'), 0);
  assert.equal(dice('方案 B', '方案B'), 1);
  assert.ok(dice('暗色主题采用方案 B「夜航日志」', '暗色主题确定为方案 B「夜航日志」') > 0.5);
});

test('近义重复：暗色主题三条连成一组、MiniMax 手写体两条互命中', () => {
  assert.ok(looksSimilar(darkA, darkB), '标题高度相似的暗色两条要命中');
  assert.ok(looksSimilar(darkA, darkC), '三条暗色是同一组');
  assert.ok(looksSimilar(darkB, darkC) || looksSimilar(darkA, darkC), '组内不许漏到只剩孤点');
  assert.ok(looksSimilar(fontA, fontB), 'MiniMax 手写体与粉色是同一条约定');
  const counts = similarCounts([darkA, darkB, darkC, fontA, fontB]);
  assert.equal(counts.get('a'), 2);
  assert.equal(counts.get('d'), 1);
  assert.equal(counts.get('e'), 1);
  assert.deepEqual(similarPeers(darkA, [darkA, darkB, darkC]).map((r) => r.id), ['b', 'c']);
});

test('不相关对不误报：MiniMax 入口 vs 手写体、多内核总原则 vs 任务通知', () => {
  assert.equal(looksSimilar(entry, fontA), false);
  assert.equal(looksSimilar(entry, fontB), false);
  assert.equal(looksSimilar(principle, notify), false);
  assert.equal(looksSimilar(darkA, principle), false);
});

test('范围与状态：不同项目不比、通用不和项目比、停用与交接不参与', () => {
  const otherProject = row('i', '暗色主题采用方案 B「夜航日志」', darkA.keywords, { workspace_id: 'other', workspace_path: '/x/other' });
  assert.equal(looksSimilar(darkA, otherProject), false, '两个项目各自的同一条约定不算重复');
  const general = row('j', '暗色主题采用方案 B「夜航日志」', darkA.keywords, { scope: 'general', workspace_id: '', workspace_path: '' });
  assert.equal(looksSimilar(darkA, general), false, '通用偏好只和通用偏好比');
  const disabled = { ...darkB, state: 'disabled' };
  assert.equal(looksSimilar(darkA, disabled), false, '停用的不该被劝删');
  const handoff = { ...darkB, kind: 'handoff' };
  assert.equal(looksSimilar(darkA, handoff), false, '交接不在记忆库里比');
});

test('接线：行挂提示、详情并排对照、面板把相似数传下去', () => {
  const rowSrc = read('../src/memory/MemoryRow.vue');
  assert.ok(rowSrc.includes('similarCount'), '行要接相似条数');
  assert.ok(rowSrc.includes('疑似重复 {{ similarCount }}'), '行要有可见提示');
  const detailSrc = read('../src/memory/MemoryDetail.vue');
  assert.match(detailSrc, /import MemoryConflict from '\.\/MemoryConflict\.vue'/);
  assert.ok(detailSrc.includes('kind="similar"'), '详情用相似关系的说明文字');
  assert.ok(detailSrc.includes("emit('removePeer'"), '详情要把删除动作交给面板');
  const panelSrc = read('../src/memory/MemoryPanel.vue');
  assert.match(panelSrc, /import \{ similarCounts, similarPeers \} from '\.\/similar\.js'/);
  assert.ok(panelSrc.includes(':similar-count="similarHits.get(row.id)'), '列表行接上相似数');
  assert.ok(panelSrc.includes(':similar-peers="detailSimilar"'), '详情接上相似条目');
  assert.match(panelSrc, /@remove-peer="removeMemory"/);
});
