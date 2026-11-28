import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { ruleBlocks, splitSelectors } from './lib/css-cascade.mjs';

const read = (rel) => readFileSync(new URL(rel, import.meta.url), 'utf8');
const rules = ruleBlocks(read('../src/theme.css'));
// 祖先与属性选择器由测试明确指定，避免把其它 body 后代的规则算进来。
function declarations(...selectors) {
  return Object.fromEntries(rules
    .filter(({ selector, conditions }) => !conditions.length &&
      splitSelectors(selector).some((part) => selectors.includes(part.trim())))
    .flatMap(({ body }) => Array.from(body.matchAll(/([\w-]+)\s*:\s*([^;]+);/g),
      ([, property, value]) => [property, value.trim()])));
}

test('两道业务脉冲贴内容区上缘，不占布局也不拦截点击', () => {
  const html = read('../index.html').replace(/<!--[\s\S]*?-->/g, '');
  assert.match(html, /<body[^>]*>\s*<div data-titlebar-pulse="2" aria-hidden="true"><\/div>/);
  for (const lane of ['body::after', "body > [data-titlebar-pulse='2']"]) {
    const base = declarations(lane);
    for (const [property, value] of Object.entries({
      position: 'fixed', top: '0', height: '3px', 'pointer-events': 'none', opacity: '0',
    })) assert.equal(base[property], value, `${lane} 的 ${property}`);
    assert.equal(base.animation, undefined, `${lane} 空闲时不应播放动画`);
    const active = declarations(lane, lane.replace('body', 'body.pulse-active'));
    assert.match(active.animation ?? '', /^titlebar-pulse-sweep 7\.68s /);
    assert.equal(active['animation-delay'] ?? '0s', lane === 'body::after' ? '0s' : '3.84s');
  }
});
