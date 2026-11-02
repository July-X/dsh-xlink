import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 「Defender 排除项」卡（2026-11-02）的几条硬约束：
//   ① 代价必须常驻可见——排除之后这些目录不再被实时扫描；
//   ② 路径一律读后端返回的真值，前端不写死任何目录；
//   ③ 只对 Defender 生效这件事要说在明面上，不许让装了第三方杀软的用户白等；
//   ④ 加入是自动的，但用户必须能撤销——「移除排除项」那颗按钮不许消失。
//
// 判据扫**模板**（HTML 注释已剥）：本仓模板带着大段解释「为什么这么写」的
// 中文注释，注释里常原样引用被判据的词——按整份文件搜会把自己写的解释当成命中。

const panel = readFileSync('ui/src/shell/SettingsPanel.vue', 'utf8');
const store = readFileSync('ui/src/shell/avTrust.js', 'utf8');

function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

const tpl = templateOf(panel);

test('卡片把代价写在常驻正文里（不许藏进 tooltip）', () => {
  assert.match(
    tpl,
    /<p class="muted av-trust-note">[\s\S]*?不再被 Defender 实时扫描/,
    '「排除之后不再被实时扫描」必须出现在可见正文里，不能只写在悬停提示里',
  );
});

test('路径由后端下发，前端不写死任何目录', () => {
  assert.match(tpl, /v-for="target in avTrustStore\.targets"/, '目录清单读后端返回的真值');
  assert.doesNotMatch(
    tpl,
    /\.dsh-xlink|node_modules|\\\\kernels/,
    '前端不许出现写死的目录：路径一律经 Rust 的 paths::/kernel::data_dir 解析',
  );
});

test('加入是自动的，但撤销入口必须在，且触 IO 的按钮挂 loading', () => {
  assert.match(tpl, /加入排除项/);
  assert.match(tpl, /移除排除项/, '自动加入之后用户必须能撤销');
  assert.match(tpl, /isLoading\('defenderExclusion'\)/);
  // 手动加入只是「自动那次被拒 / 失败」的重试入口，不能是唯一入口。
  assert.match(store, /av_trust_set_exclusion/);
});

test('四种结果各有一句话，用户知道下一步该做什么', () => {
  const box = tpl + panel;
  for (const outcome of ['applied', 'removed', 'declined']) {
    assert.ok(box.includes(`'${outcome}'`), `状态句要区分 ${outcome}`);
  }
  assert.match(box, /上次加入失败/, '失败要把原因说出来，而不是只说「失败」');
  assert.match(box, /不会再自动/, '拒绝 / 移除之后不再自动重试，这件事要告诉用户');
});

test('只对 Defender 生效要说在明面上', () => {
  assert.match(
    tpl,
    /只对 Microsoft Defender 生效[\s\S]*?360/,
    '装了第三方杀软的用户得知道这张卡救不了他们',
  );
});

test('自动加入的接线在后端（不是前端在 Vue 里点按钮）', () => {
  const rust = readFileSync('src-tauri/src/lib.rs', 'utf8');
  assert.match(
    rust,
    /shell::av_trust::spawn_auto\(\)/,
    '启动时那条自动加入的线程必须真的被派出去（摘掉这行测试不会红，线上也不会发生任何事）',
  );
  // 前端只读状态 + 手动重试，不许自己排「启动时加一次」。
  assert.doesNotMatch(store, /setTimeout|onMounted/, '自动加入在 Rust 侧，前端不搞定时器');
});

test('与 Rust 的通信只走 bridge 的 invoke', () => {
  assert.match(store, /import \{ invoke \} from '\.\/bridge\.js'/);
  assert.doesNotMatch(store, /window\.__TAURI__/, '组件侧不直接碰 __TAURI__');
});
