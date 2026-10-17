import assert from 'node:assert/strict';
import test from 'node:test';
import { findDuplicateBlocks } from './check-code-duplicates.mjs';

const lines = (prefix, count) => Array.from({ length: count }, (_, i) => `${prefix}_${i}();`).join('\n');

test('长文件不受行数限制；不同内容不会被当成重复', () => {
  const result = findDuplicateBlocks([['large.js', lines('unique', 1200)]]);
  assert.equal(result.count, 0);
});

test('重叠窗口合并为一处重复，报告来源与份数', () => {
  const block = lines('shared', 30);
  const result = findDuplicateBlocks([['a.js', block], ['b.js', block]]);
  assert.equal(result.count, 1);
  assert.match(result.samples[0], /a.js:1（30 行 × 2 份）/);
});

test('短片段不计入；注释和空白不影响重复识别', () => {
  assert.equal(findDuplicateBlocks([['a.js', lines('short', 11)], ['b.js', lines('short', 11)]]).count, 0);
  const block = lines('shared', 15);
  assert.equal(findDuplicateBlocks([['a.js', block], ['b.js', block.replaceAll(';', '; // note\n')]]).count, 1);
});

test('Rust 测试模块不参与生产重复检测', () => {
  const block = lines('shared', 15);
  assert.equal(findDuplicateBlocks([
    ['a.rs', block],
    ['b.rs', `#[cfg(test)]\nmod tests {\n${block}\n}`],
  ]).count, 0);
});

test('独立重复块分别计数，超出原重复预算的样本不会漏掉', () => {
  const sources = Array.from({ length: 9 }, (_, i) => [
    [`a${i}.js`, lines(`block${i}`, 15)], [`b${i}.js`, lines(`block${i}`, 15)],
  ]).flat();
  assert.equal(findDuplicateBlocks(sources).count, 9);
});
