import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const panel = readFileSync(new URL('../src/shell/SettingsPanel.vue', import.meta.url), 'utf8');
const theme = readFileSync(new URL('../src/theme.css', import.meta.url), 'utf8');
const luminance = (hex) => {
  const channels = hex.match(/[a-f\d]{2}/gi).map((part) => parseInt(part, 16) / 255)
    .map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return channels.reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
};

test('实验标识与边框共用主题色，明暗下文字满足 4.5:1', () => {
  assert.match(panel, /实验内核/);
  assert.match(panel, /稳定内核/);
  assert.match(panel, /\.settings-port-group--experimental\s*\{[^}]*border-color: var\(--experimental\)/);
  assert.match(panel, /\.settings-port-group--experimental legend\s*\{[^}]*color: var\(--experimental\)/);
  const blocks = [theme.split('html.dark {')[0], theme.split('html.dark {')[1]];
  for (const block of blocks) {
    const token = (name) => block.match(new RegExp('--' + name + ': (#[a-f\\d]{6});', 'i'))[1];
    const [a, b] = [luminance(token('experimental')), luminance(token('canvas'))].sort((a, b) => b - a);
    assert.ok((a + 0.05) / (b + 0.05) >= 4.5);
    assert.match(block, /--experimental-fill:/);
  }
});

test('双端口保留完整可访问名、整数范围和窄栏退让机制', () => {
  for (const label of ['DeepSeek Web UI 端口', 'MiniMax Web UI 端口', 'MiniMax App 端口']) {
    assert.ok(panel.includes('aria-label="' + label + '"'));
  }
  assert.match(panel, /<el-form-item label="Web UI 端口">/);
  assert.match(panel, /<el-form-item label="App 端口">/);
  assert.doesNotMatch(panel, /<el-form-item label="MiniMax/);
  const inputs = [...panel.matchAll(/<el-input-number[\s\S]*?\/>/g)].map((match) => match[0]);
  assert.equal(inputs.length, 3);
  for (const input of inputs) {
    assert.match(input, /:min="1024"/);
    assert.match(input, /:max="65535"/);
    assert.match(input, /:precision="0"/);
  }
  assert.match(panel, /container-type: inline-size/);
  assert.match(panel, /@container[^}]+flex-direction: column/);
  assert.match(panel, /min-inline-size: 0/);
  assert.match(panel, /<el-form-item label-width="0" class="settings-port-actions">/);
});
