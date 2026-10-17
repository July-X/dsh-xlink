// dev 启动的前置接线：`tauri dev` 的 build script 会检查 tauri.conf.json 里
// 登记的 bundle.resources，其中 resources/builtin-plugins 是 prep:builtin 的
// 生成产物（不进 git）。2026-10-09 实测：dev 这条链漏挂前置，新 clone 上
// `pnpm run dev` 直接死在「resource path `resources\builtin-plugins` doesn't
// exist」。这里钉的不是行为（行为测试要真起 tauri），是接线形状——
// prep 的接线必须存在，且必须在 spawn tauri 之前。
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('dev.mjs 在 spawn tauri 之前先执行内嵌插件资源生成', () => {
  const source = read('./dev.mjs');
  // 钉的是「真的执行了生成」这一步（spawnSync 调用本体），不是「提过这个
  // 路径」——只搜路径字符串的话，把调用摘掉、留一句注释都能骗过测试
  //（2026-10-09 反向验实测抓到过这个洞）。
  const invocation = source.match(/spawnSync\(\s*process\.execPath,\s*\[prepareBuiltin\]/);
  assert.ok(
    invocation,
    'dev.mjs 里找不到 spawnSync 执行 prepare-builtin 的调用——dev 会在新 clone 上死于 tauri 资源检查',
  );
  const spawnAt = source.indexOf('spawn(process.execPath, [tauriCli');
  assert.ok(spawnAt !== -1, 'dev.mjs 里找不到 spawn tauri 的调用（结构变了？请同步本测试）');
  assert.ok(
    invocation.index < spawnAt,
    'prep 的执行必须出现在 spawn tauri 之前——顺序反了，资源检查死在生成之前，等于没挂',
  );
});

test('package.json 的每条 tauri build 入口都先跑 prep:builtin', () => {
  const pkg = JSON.parse(read('../package.json'));
  for (const script of ['build', 'build:win', 'build:mac-intel']) {
    assert.match(
      pkg.scripts[script] ?? '',
      /prep:builtin/,
      `${script} 没有先跑 prep:builtin——与 dev 同一类回归：tauri 的资源检查会在新 clone 上拦死它`,
    );
  }
});

// --- 资源管线 ↔ Rust 登记表：两份各管一门语言，必须一致 ------------------------
//
// 2026-10-12 加第二个内置插件（ponytail）时才暴露出来的一处**结构性**裂缝：
// `BuiltinSpec::entries`（Rust，物化时复制哪些）与 prep 脚本的 `BUILTINS[].entries`
// （JS，打包时复制哪些）是同一份清单的两份实现。它们不一致时的症状极难定位：
// 多出来的那份「打包了但没物化」看着一切正常，少掉的那份「物化时找不到目录」
// 报的是「应用资源可能不完整，请重新安装 dsh-xlink」——把源码侧的错报成用户
// 的安装包坏了。
//
// 两边**不可能**合成一份（一个 Rust、一个 Node，跨不过去），所以这里钉死它们
// 必须一致。解析刻意用最笨的正则：登记表是编译期常量，形状变了本条就该红，
// 而不是悄悄放过一种新写法。

/** 从 Rust 的 `BUILTINS` 常量里读出 `{ id, entries }`。 */
function rustRegistry() {
  const source = read('../src-tauri/src/plugins/builtin/builtins.rs');
  const block = source.match(/pub\(crate\) const BUILTINS:[^=]*=\s*&\[([\s\S]*?)\n\];/);
  assert.ok(block, 'builtins.rs 里找不到 BUILTINS 常量（结构变了？请同步本测试）');
  // id 在登记表里写成常量名（`id: OPENAI_OAUTH`）。**按声明解析，不在这里
  // 逐个映射**：2026-10-14 加第三个内置插件时，硬编码的映射表把新插件的 id
  // 读成了常量名（`MEMORY`），报的是「两侧 id 集合不一致」——而真正的原因只是
  // 判据少一个分支。声明解析器不认识的名字会原样出现在比较里，照样红得清楚。
  const constants = new Map();
  for (const match of source.matchAll(/const\s+([A-Z_]+)\s*:\s*&str\s*=\s*"([^"]+)";/g)) {
    constants.set(match[1], match[2]);
  }
  const out = [];
  // 每个条目从 `id:` 到下一个 `},` 之间的那几行。
  for (const chunk of block[1].split(/\n\s{4}\},/)) {
    const id = /\n\s{8}id:\s*([A-Z_]+),/.exec(chunk);
    const entries = /\n\s{8}entries:\s*&\[([\s\S]*?)\],/.exec(chunk);
    if (!id || !entries) continue;
    out.push({
      id: constants.get(id[1]) ?? id[1],
      entries: [...entries[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]),
    });
  }
  return out;
}

/** 从 prep 脚本的 `BUILTINS` 里读出 `{ id, entries }`。 */
function jsRegistry() {
  const source = read('./prepare-builtin-plugins.mjs');
  const block = source.match(/const BUILTINS = \[([\s\S]*?)\n\];/);
  assert.ok(block, 'prepare-builtin-plugins.mjs 里找不到 BUILTINS 登记表');
  return [...block[1].matchAll(/id:\s*'([^']+)',[\s\S]*?entries:\s*\[([\s\S]*?)\]/g)].map((m) => ({
    id: m[1],
    entries: [...m[2].matchAll(/'([^']+)'/g)].map((x) => x[1]),
  }));
}

test('两份内置插件登记表（Rust 物化 / JS 打包）逐项一致', () => {
  const rust = rustRegistry();
  const js = jsRegistry();
  assert.ok(rust.length > 0, 'Rust 登记表解析出 0 项——判据自己坏了，必须红');
  assert.ok(js.length > 0, 'JS 登记表解析出 0 项——判据自己坏了，必须红');
  assert.deepEqual(
    rust.map((entry) => entry.id).sort(),
    js.map((entry) => entry.id).sort(),
    '两侧登记的内置插件 id 集合不一致：多出来的那份「打包了但没物化」，少掉的「物化时报安装包不完整」',
  );
  for (const entry of rust) {
    const other = js.find((x) => x.id === entry.id);
    assert.deepEqual(
      [...entry.entries].sort(),
      [...other.entries].sort(),
      `${entry.id}: Rust 的物化清单与 prep 的打包清单必须逐项一致`,
    );
  }
});

test('每个内置插件都有源码目录，且 package.json 的 name 与 id 对得上', () => {
  for (const entry of jsRegistry()) {
    const pkg = JSON.parse(read(`../plugins/${entry.id}/package.json`));
    assert.equal(
      pkg.name,
      `xlink-${entry.id}`,
      `${entry.id}: 包名必须是 xlink-<id>——接线行 id / settingsNs 靠它对上`,
    );
    for (const item of entry.entries) {
      assert.ok(
        existsSync(new URL(`../plugins/${entry.id}/${item}`, import.meta.url)),
        `${entry.id}: 登记表列了 ${item}，但 plugins/${entry.id}/${item} 不存在——prep 会因此中止`,
      );
    }
  }
});

// --- workflow -----------------------------------------------------------------
//
// 上面两条只钉了本地入口。2026-10-09 发 v0.4.4 时 CI 与发布流水线同时红：
// 两个 workflow 在 `pnpm install` 之后直接开编，从没跑过 prep:builtin，
// 于是 quality job 的 cargo test 与两个平台的 tauri build 全都死在
// `resource path 'resources/builtin-plugins' doesn't exist`。上面那两条判据
// 全绿——它们看的是 dev.mjs 与 package.json，看不见 workflow。
//
// 判据取「会跑 build script 的 cargo 子命令」而不是「文件里提没提 prep」：
// cargo fmt 不编译，cargo build/check/test/clippy 都编译。case-insensitive 是
// 为了连 `Cargo` 与 `CARGO` 一起算——Windows runner 上拼错大小写同样会编译。

const WORKFLOWS = ['../.github/workflows/desktop-ci.yml', '../.github/workflows/desktop-release.yml'];

/** 把 workflow 切成 job 块（只取 `jobs:` 之后、两空格缩进的 job id）。 */
function jobBlocks(source) {
  const lines = source.split('\n');
  const jobsStart = lines.findIndex((line) => /^jobs:\s*$/.test(line));
  assert.notEqual(jobsStart, -1, 'workflow 里找不到 jobs: 段（结构变了？请同步本测试）');
  const blocks = [];
  let current = null;
  for (const line of lines.slice(jobsStart + 1)) {
    const header = line.match(/^ {2}([A-Za-z0-9_-]+):\s*$/);
    if (header) {
      current = { name: header[1], lines: [] };
      blocks.push(current);
      continue;
    }
    if (current) current.lines.push(line);
  }
  return blocks;
}

test('每个会编译 Rust 的 workflow job 都先生成内嵌插件资源', () => {
  for (const workflow of WORKFLOWS) {
    const source = read(workflow);
    const jobs = jobBlocks(source);
    assert.ok(jobs.length > 0, `${workflow}: 没解析出任何 job——判据自己失效时必须红`);

    const compilation = /\bcargo\s+(build|check|test|clippy|run)\b|\btauri\s+(build|dev)\b|\bpnpm run check:rust:ci\b/i;
    const compiling = jobs.map((job) => ({
      name: job.name,
      source: job.lines.filter((line) => !line.trimStart().startsWith('#')).join('\n'),
    })).filter((job) => compilation.test(job.source));
    // 同样是为了「判据在空转」时能响：解析不出编译型 job 就说明匹配式坏了，
    // 而不是「所有 job 都不编译所以通过」。
    assert.ok(
      compiling.length > 0,
      `${workflow}: 没解析出任何编译型 job——cargo 子命令匹配式可能已经与 workflow 脱节`,
    );

    for (const job of compiling) {
      const prepareAt = job.source.search(/^\s+run:\s+(?:pnpm run prep:builtin|node scripts\/prepare-builtin-plugins\.mjs)\s*$/m);
      assert.ok(
        prepareAt >= 0 && prepareAt < job.source.search(compilation),
        `${workflow} 的 job「${job.name}」会编译但没生成内嵌插件资源——` +
          'tauri 的 build script 会校验 bundle.resources，新 checkout 上必然死于 ' +
          "`resource path 'resources/builtin-plugins' doesn't exist`",
      );
    }
  }
});
