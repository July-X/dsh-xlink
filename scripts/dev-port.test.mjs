// dev 端口在三处各写了一遍，漂了不会立刻报错，只会让 `pnpm run dev` 打开一个
// 连不上 vite 的窗口（tauri 指向 5174、vite 却在 5173），症状比冲突本身难查得多。
// 这里把三处钉死成同一个数字。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

function portOf(source, pattern, what) {
  const matched = source.match(pattern);
  assert.ok(matched, `${what} 里找不到端口，请确认该配置项还在`);
  return Number(matched[1]);
}

test('vite 的 server.port 默认值与 tauri 的 devUrl 一致', () => {
  const vite = portOf(
    read('../vite.config.mjs'),
    /server:\s*\{[\s\S]*?port:\s*Number\(process\.env\.DSH_DEV_PORT\)\s*\|\|\s*(\d+)/,
    'vite.config.mjs 的 server.port',
  );
  const devUrl = portOf(
    read('../src-tauri/tauri.conf.json'),
    /"devUrl":\s*"http:\/\/localhost:(\d+)"/,
    'tauri.conf.json 的 devUrl',
  );
  assert.equal(vite, devUrl, 'vite 与 tauri 的 dev 端口必须一致，否则 tauri 会连上一个空端口');
});

test('dev.mjs 的 DEFAULT_PORT 与上面两处一致', () => {
  const wrapper = portOf(read('./dev.mjs'), /const DEFAULT_PORT = (\d+);/, 'dev.mjs 的 DEFAULT_PORT');
  const vite = portOf(
    read('../vite.config.mjs'),
    /server:\s*\{[\s\S]*?port:\s*Number\(process\.env\.DSH_DEV_PORT\)\s*\|\|\s*(\d+)/,
    'vite.config.mjs 的 server.port',
  );
  assert.equal(wrapper, vite, 'dev.mjs 与 vite.config.mjs 的默认端口必须一致');
});

test('默认端口不是 vite 的首选项 5173', () => {
  // 不是硬性规则，是当初改默认端口的理由：5173 常被本机另一个工程的 vite 占着，
  // 而 strictPort 下 vite 不会顺延。哪天想换回去，把这条一并改掉。
  const wrapper = portOf(read('./dev.mjs'), /const DEFAULT_PORT = (\d+);/, 'dev.mjs 的 DEFAULT_PORT');
  assert.notEqual(wrapper, 5173, '默认端口已经挪走过一次了，别悄悄绕回去');
});
