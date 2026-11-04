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

test('卡片只在 Windows 显示，路径由后端下发', () => {
  assert.match(tpl, /v-if="avTrustStore\.supported"/, '非 Windows 不渲染整张卡');
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

test('只有一个数据主目录，不依赖内核是否安装', () => {
  assert.doesNotMatch(tpl, /两个目录|这两个目录|内核尚未安装/, '旧的两目录/内核前提不许回来');
  assert.match(panel, /avTargetMissing/, '主目录不存在时要有独立禁用原因');
  assert.match(
    panel,
    /avAddDisabled = computed\(\(\) => !avCanAct\.value \|\| avTargetMissing\.value/,
    '没有可排除的主目录时「加入」必须禁用',
  );
});

test('只对 Defender 生效要说在明面上', () => {
  assert.match(
    tpl,
    /只对 Microsoft Defender 生效[\s\S]*?360/,
    '装了第三方杀软的用户得知道这张卡救不了他们',
  );
});

test('Defender 没在跑时两颗按钮都禁用：不弹无意义的授权，也不回 0x800106ba', () => {
  // 2026-11-02 本机实测：Defender 被第三方杀软接管、服务停着时点「加入」
  // 只会弹一次 UAC 再得到 `MpPreference … 0x800106ba`，用户读不懂。
  const actions = /<div class="btn-row av-trust-actions">[\s\S]*?<\/div>/.exec(tpl);
  assert.ok(actions, '找不到 Defender 卡的动作排');
  const disabled = [...actions[0].matchAll(/:disabled="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(disabled, ['avAddDisabled', 'avRemoveDisabled'], '两颗按钮按各自状态禁用');
  // 前置守卫（平台 / Defender 在不在 / 实时防护 / 全局忙碌）集中在 avCanAct，
  // 两颗按钮共用一份，不再各抄一遍。
  const canAct = /const avCanAct = computed\(([\s\S]*?)\n\);/.exec(panel);
  assert.ok(canAct, '找不到 avCanAct');
  assert.match(canAct[1], /!globalBusy\.value/, '全局忙碌时也要拦');
  assert.match(canAct[1], /avTrustStore\.defenderPresent/, '没装 Defender 时按钮必须禁用');
  assert.match(
    canAct[1],
    /avTrustStore\.defenderRealtime/,
    'Defender 没在跑时按钮必须禁用（否则前端拦不住那次无意义提权）',
  );
  // 禁用按钮必须配一句话说清为什么，不能只是灰着。
  assert.match(
    panel,
    /Defender 未启用（可能已被第三方杀软接管）/,
    '状态句要说清为什么不给点',
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

test('排除项已加入后「加入排除项」不可点，没有加入时「移除排除项」不可点', () => {
  // 两颗都亮着会让人以为可以反复点，实际只是把同一个值再写一遍、再弹一次 UAC。
  // 可用性由后端记账（avTargetState）派生，不在前端另存一份真相。
  assert.match(tpl, /:disabled="avAddDisabled"/, '加入按钮按「全都加过了」禁用');
  assert.match(tpl, /:disabled="avRemoveDisabled"/, '移除按钮按「一个都没加」禁用');
  assert.match(
    panel,
    /avAllExcluded = computed\(\s*\(\) => avTrustStore\.targets\.length > 0/,
    '「全都加过」由目录数与前端的已加入计数比较得出',
  );
  assert.match(panel, /avTargetState\(target\) === '已加入'/, '计数只认后端记账，不另存一份');
  assert.match(panel, /数据主目录已经在 Defender 排除项里/, '禁用时要说清为什么，而不是一颗灰按钮');
});
