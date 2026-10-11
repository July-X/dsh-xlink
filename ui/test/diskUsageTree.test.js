import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { readShellSource } from '../../scripts/lib/shell-source.mjs';

// 「磁盘用量」的下钻：点瓦片里的条目行（「正式版 666.5 MB」这类小计），
// 就地展开该条目下一层子目录 / 文件的占用。三个会静默失效的约定：
//  ① 后端命令只许在 xlink_home 之内下钻——path 是前端传来的，越界校验
//     一旦被绕开，这条命令就变成通用目录探测器；
//  ② 前端按路径缓存展开结果，但报表一换（刷新 / 后台重扫回填）缓存必须
//     作废重取，否则「展开后的加总」与瓦片对不上；
//  ③ 行首箭头只给目录（isDir），文件是叶子——没有「里面」。

const versionsPanel = readFileSync('ui/src/kernel/VersionsPanel.vue', 'utf8');
const tree = readFileSync('ui/src/kernel/DiskUsageTree.vue', 'utf8');
const labels = readFileSync('ui/src/shell/labels.js', 'utf8');
const diskusage = readShellSource('diskusage.rs');

// --- ① 后端：命令存在、走 blocking、且必须过 xlink_home 越界校验 ---

test('下钻命令存在，异步走 blocking，且路径必须过 scope_to_home', () => {
  assert.match(
    diskusage,
    /pub async fn disk_usage_children\(path: String\) -> Result<Vec<UsageEntry>, String>/,
    '命令签名变了，前端与权限清单要跟着改',
  );
  // 命令体里必须先过越界校验再统计：把 `scope_to_home(...)?` 换成直接
  // `PathBuf::from(&path)`，本断言转红——那正是「探测器化」的第一步。
  assert.match(
    diskusage,
    /fn disk_usage_children[\s\S]{0,400}?scope_to_home\(Path::new\(&path\), &home\)\?/,
    '命令体必须先 scope_to_home 校验，再 child_entries 统计',
  );
  assert.match(
    diskusage,
    /fn disk_usage_children[\s\S]{0,600}?crate::commands::blocking/,
    '几万个文件的子树统计必须进 blocking worker，不许卡 IPC 线程',
  );
});

test('UsageEntry 带 is_dir，旧缓存缺该字段时按目录读回', () => {
  assert.match(
    diskusage,
    /#\[serde\(default = "default_true"\)\]\s*pub is_dir: bool/,
    'is_dir 必须带 default=true：缓存条目本来就全是目录，缺字段按 false 读会让旧缓存全表没有展开箭头',
  );
  assert.match(diskusage, /fn default_true\(\) -> bool\s*\{\s*true\s*\}/);
});

// --- ② 前端：接线、缓存失效、叶子与箭头 ---

test('VersionsPanel 把条目列表换成下钻树，并喂给它报表时间戳', () => {
  assert.match(
    versionsPanel,
    /<DiskUsageTree\s+v-if="group\.entries\.length"\s+:entries="group\.entries"\s+:stamp="diskUsage\.measuredAt"/,
    '瓦片条目必须经 DiskUsageTree 渲染，且带上 measuredAt 当缓存失效时钟',
  );
  // 旧的静态列表整个退场：模板与样式都不许留死代码。
  assert.doesNotMatch(versionsPanel, /class="usage-entries"/);
  assert.doesNotMatch(versionsPanel, /\.usage-entry[\s{-]/);
});

test('字节格式化单源：labels.js 导出，两个消费方都从它拿', () => {
  assert.match(labels, /export function formatBytes\(bytes\)/);
  assert.match(
    versionsPanel,
    /import \{ formatBytes, relativeTimeLabel \} from '\.\.\/shell\/labels\.js';/,
  );
  // VersionsPanel 不许再留一份私有实现：两份口径迟早漂成两张表两个数。
  assert.doesNotMatch(versionsPanel, /function formatBytes\(/);
  assert.match(tree, /import \{ formatBytes \} from '\.\.\/shell\/labels\.js';/);
});

test('只有目录行可展开：isDir 才渲染按钮并带 aria-expanded，文件是叶子', () => {
  assert.match(
    tree,
    /<button\s+v-if="entry\.isDir"\s+type="button"\s+class="usage-tree__row"\s+:aria-expanded=/,
    '目录行必须是原生 button（键盘可达）并带 aria-expanded',
  );
  // 文件行走 v-else 的叶子分支，不许复用目录按钮。
  assert.match(
    tree,
    /v-else class="usage-tree__row usage-tree__row--leaf"/,
  );
  assert.match(tree, /function toggle\(entry\)\s*\{\s*if \(!entry \|\| !entry\.isDir\) return;/);
});

test('展开结果按路径缓存：在途与已取不再发，出错可重试', () => {
  assert.match(
    tree,
    /invoke\('disk_usage_children', \{ path \}\)/,
    '下钻只许调 disk_usage_children 这一条命令',
  );
  assert.match(
    tree,
    /if \(cached && \(cached\.loading \|\| cached\.loaded\)\) return;/,
    '在途 / 已取必须短路，连点展开不重复扫描',
  );
  // 出错时 loaded 仍是 false，「重试」才走得进 loadChildren。
  assert.match(tree, /class="usage-tree__retry"/);
  assert.match(tree, /@click="loadChildren\(entry\.path\)"/);
});

test('报表一换（stamp 变化），子项缓存作废重取，展开状态保留', () => {
  assert.match(
    tree,
    /watch\(\s*\(\) => props\.stamp,\s*\(\) => \{\s*\/\/ 缓存清掉，展开状态保留[\s\S]*?for \(const key of Object\.keys\(children\)\) delete children\[key\];[\s\S]*?if \(expanded\[key\]\) loadChildren\(key\);/,
    'stamp 变化必须清空 children 缓存并重取已展开的层——旧数字配新报表会让加总对不上',
  );
});

test('树里没有任何删除入口（与瓦片同一纪律）', () => {
  // 模板块内不得出现删除类动作；这条是 2026-10-02「只读」拍板的延续。
  const template = tree.slice(tree.indexOf('<template>'), tree.lastIndexOf('</template>'));
  assert.doesNotMatch(template, /删除|卸载|清理/);
});
