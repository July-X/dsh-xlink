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
