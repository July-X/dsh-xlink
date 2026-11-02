import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 「杀软排除项」卡（2026-11-02）的几条硬约束：
//   ① 代价必须常驻可见——排除之后这些目录不再被实时扫描；
//   ② 路径一律读后端返回的真值，前端不写死任何目录；
//   ③ 界面里不许出现写死的耗时数字（每台机器、每套杀软配置都不一样）；
//   ④ 第三方杀软只给指引，不假装有接口。
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

test('杀软卡把代价写在常驻正文里（不许藏进 tooltip）', () => {
  assert.match(
    tpl,
    /<p class="muted av-trust-note">[\s\S]*?不再被实时扫描/,
    '「排除之后不再被实时扫描」必须出现在可见正文里，不能只写在悬停提示里',
  );
});

test('路径由后端下发，前端不写死任何目录', () => {
  assert.match(tpl, /v-for="target in avTrustStore\.targets"/, '目录清单读后端返回的真值');
  assert.doesNotMatch(
    tpl,
    /\.dsh-xlink|node_modules|\\\\kernels/,
    '前端不许出现写死的目录：路径一律经 Rust 的 paths::* / kernel::data_dir 解析',
  );
});

test('界面里不出现写死的耗时数字（只能现测）', () => {
  assert.doesNotMatch(
    tpl,
    /7\.7|55\s*秒|7,?700/,
    '实测数字只属于那台机器，不许写进界面',
  );
  assert.match(tpl, /avOpenCostText/, '耗时读后端现测的值');
  assert.match(store, /av_trust_measure_open_cost/, '测量走后端那条命令');
});

test('加 / 移除两个动作都在卡上，且触 IO 的按钮都挂了 loading', () => {
  assert.match(tpl, /加入排除项/);
  assert.match(tpl, /移除排除项/);
  assert.match(tpl, /isLoading\('avTrustSet'\)/);
  assert.match(tpl, /isLoading\('avTrustMeasure'\)/);
});

test('第三方杀软只给指引，不假装有接口', () => {
  assert.match(store, /信任区/, '第三方杀软走它自己的界面');
  assert.match(tpl, /avThirdPartyGuide/, '指引要呈现在卡上，不能只躺在 store 里');
});

test('排除项清单读不到时要说「读不到」，不能显示成「一条都没有」', () => {
  // Windows 把 Defender 的 Exclusions 子树收给管理员了，普通用户读不动。
  assert.match(store, /exclusionsReadable/, '后端要区分「读到」与「读不到」');
  assert.match(tpl, /avExclusionText/, '界面要分开说');
  // 「状态未知」是脚本里的判定（模板只调 avTargetState），所以扫全文。
  assert.match(panel, /状态未知/, '读不到时目录状态是「未知」，不是「未排除」');
  assert.match(tpl, /avTargetState\(target\)/, '模板要走那个判定');
});

test('与 Rust 的通信只走 bridge 的 invoke', () => {
  assert.match(store, /import \{ invoke \} from '\.\/bridge\.js'/);
  assert.doesNotMatch(store, /window\.__TAURI__/, '组件侧不直接碰 __TAURI__');
});
