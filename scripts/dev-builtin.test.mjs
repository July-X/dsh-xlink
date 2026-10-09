// dev 启动的前置接线：`tauri dev` 的 build script 会检查 tauri.conf.json 里
// 登记的 bundle.resources，其中 resources/builtin-plugins 是 prep:builtin 的
// 生成产物（不进 git）。2026-10-09 实测：dev 这条链漏挂前置，新 clone 上
// `pnpm run dev` 直接死在「resource path `resources\builtin-plugins` doesn't
// exist」。这里钉的不是行为（行为测试要真起 tauri），是接线形状——
// prep 的接线必须存在，且必须在 spawn tauri 之前。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
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

    const compiling = jobs.filter((job) =>
      /\bcargo\s+(build|check|test|clippy|run)\b/i.test(job.lines.join('\n')),
    );
    // 同样是为了「判据在空转」时能响：解析不出编译型 job 就说明匹配式坏了，
    // 而不是「所有 job 都不编译所以通过」。
    assert.ok(
      compiling.length > 0,
      `${workflow}: 没解析出任何编译型 job——cargo 子命令匹配式可能已经与 workflow 脱节`,
    );

    for (const job of compiling) {
      assert.match(
        job.lines.join('\n'),
        /prep:builtin|prepare-builtin-plugins\.mjs/,
        `${workflow} 的 job「${job.name}」会编译但没生成内嵌插件资源——` +
          'tauri 的 build script 会校验 bundle.resources，新 checkout 上必然死于 ' +
          "`resource path 'resources/builtin-plugins' doesn't exist`",
      );
    }
  }
});
