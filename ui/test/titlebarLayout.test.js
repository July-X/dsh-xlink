import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// Windows 标题栏的标题必须在**整条标题栏**上居中。
//
// 旧写法是 `.mac-titlebar--win .mac-titlebar__caption { right: 104px }`：它把标题容器
// 从右边压掉 104px，容器的 flex 居中于是把文字整体推到 52px 偏左（窗口固定 480px 宽，
// 肉眼可见），而紧挨着的注释还写着「Windows 上标题也真正居中」——数值与注释互相矛盾，
// 且没有任何门禁能发现（`check:invariants` 只管无边框来源与最小化语义）。
test('Windows 标题栏的标题对整条标题栏居中，不靠单边 inset 让位', () => {
  const css = readFileSync('ui/src/theme.css', 'utf8');
  const rule = css.match(/\.mac-titlebar--win \.mac-titlebar__caption\s*\{([^}]*)\}/s);
  assert.ok(rule, '必须能找到 .mac-titlebar--win .mac-titlebar__caption 规则');
  const body = rule[1];

  assert.doesNotMatch(
    body,
    /(^|\s)right\s*:/,
    '单边 right 会让标题左偏按钮组宽度的一半；要用左右对称的内边距',
  );
  assert.doesNotMatch(body, /(^|\s)left\s*:/, '单边 left 同理会让标题右偏');

  const padding = body.match(/padding:\s*0\s+(\d+)px/);
  assert.ok(padding, '必须用 `padding: 0 <N>px` 的对称内边距避开右侧按钮');

  // Windows 只有两枚按钮（最小化 / 关闭，窗口不可缩放所以没有最大化），
  // 内边距必须不小于按钮组合计宽度，否则标题会压到按钮上。
  const button = css.match(/\.win-caption__btn\s*\{[^}]*?width:\s*(\d+)px/s);
  assert.ok(button, '必须能从 .win-caption__btn 读到按钮宽度');
  const buttonGroup = Number(button[1]) * 2;
  assert.ok(
    Number(padding[1]) >= buttonGroup,
    `左右内边距 ${padding[1]}px 小于按钮组合计宽度 ${buttonGroup}px`,
  );
});
