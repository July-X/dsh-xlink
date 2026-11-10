// 设计文档自检，不加载产品代码，不替代真实 WebView 验收。
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const dir = dirname(fileURLToPath(import.meta.url));
const read = (name) => readFileSync(resolve(dir, name), 'utf8');
const spec = JSON.parse(read('tokens.json'));
const images = JSON.parse(read('references.json'));
assert.equal(spec.status, 'proposed-not-implemented');
assert.match(spec.baselineCommit, /^[a-f0-9]{40}$/);
assert.deepEqual(Object.keys(spec.themes).sort(), ['dark', 'light']);
assert.deepEqual(Object.keys(spec.themes.light).sort(), Object.keys(spec.themes.dark).sort());
for (const [mode, tokens] of Object.entries(spec.themes)) {
  for (const [name, value] of Object.entries(tokens)) {
    assert.match(name, /^--[a-z0-9-]+$/, `${mode} token name`);
    assert.equal(typeof value, 'string');
    assert.ok(value.length > 0 && !value.includes('var('), `${mode} ${name} 必须是字面量`);
    assert.ok(!/^--(?:fs-|font-|radius-|sidebar-|action-h)/.test(name), '几何/字体不得随主题分叉');
  }
}
function rgb(hex) {
  assert.match(hex, /^#[0-9a-f]{6}$/i);
  return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
}
function luminance(values) {
  const linear = values.map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return linear.reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
}
function contrast(a, b) {
  const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y);
  return (high + 0.05) / (low + 0.05);
}
function composite(value, background) {
  if (value.startsWith('#')) return rgb(value);
  const match = value.match(/^rgba\((\d+),\s*(\d+),\s*(\d+),\s*(0?\.\d+|1)\)$/);
  assert.ok(match, `不支持的合成颜色 ${value}`);
  const alpha = Number(match[4]);
  return [1, 2, 3].map((n, i) => Number(match[n]) / 255 * alpha + background[i] * (1 - alpha));
}
let checked = 0;
for (const [mode, tokens] of Object.entries(spec.themes)) {
  for (const pair of spec.contrastPairs) {
    const foreground = rgb(tokens[pair.foreground]);
    const background = composite(tokens[pair.background], rgb(tokens[pair.base || '--surface']));
    const ratio = contrast(foreground, background);
    assert.ok(ratio >= pair.minimum, `${mode} ${pair.foreground}/${pair.background}: ${ratio.toFixed(2)} < ${pair.minimum}`);
    checked++;
  }
}
assert.equal(images.length, 7);
assert.equal(new Set(images.map((item) => item.path)).size, 7);
for (const item of images) {
  assert.equal(item.kind, 'generated-concept-not-product-screenshot');
  const bytes = readFileSync(resolve(dir, item.path));
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  assert.equal(bytes.length, item.bytes);
  assert.equal(bytes.readUInt32BE(16), item.width);
  assert.equal(bytes.readUInt32BE(20), item.height);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), item.sha256);
}
const docs = ['README.md', 'acceptance.md', 'references.md'];
for (const name of docs) {
  const content = read(name);
  assert.ok(content.endsWith('\n') && !content.endsWith('\n\n'), `${name} 应恰好一个末尾换行`);
  for (const [, target] of content.matchAll(/!?\[[^\]]*\]\(([^)]+)\)/g)) {
    if (/^https?:\/\//.test(target) || target.startsWith('#')) continue;
    assert.ok(existsSync(resolve(dir, target.split('#')[0])), `${name} 断链: ${target}`);
  }
}
console.log(`PASS: ${Object.keys(spec.themes.light).length} tokens × 2 themes; ${checked} contrast pairs; 7 image hashes; 3 document link sets.`);
console.log('Scope: design contract integrity only; UI rendering, behavior and native platform tests are not run by this script.');
