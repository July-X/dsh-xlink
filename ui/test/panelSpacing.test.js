import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { basename, extname, resolve } from 'node:path';
import test from 'node:test';

// 功能块之间的默认间距 = 6px，由**容器**统一提供，块自己不再带 margin-bottom。
// 这是 2026-10-06 用户拍板的默认约定（见 ui/AGENTS.md §间距），所有新 UI 按它写。
//
// 这组测试的判据刻意写成**层叠结果**而不是「某条规则写了什么」。出事那次
// （概览页控制塔三张卡各带 `margin-bottom: 6px`，与 `.panel` 的 `gap: 6px`
// 叠加）里，每一处的值单看都「合理」——只有把同一列的四段缝摆在一起才看得出
// 有两种。实测 `getBoundingClientRect` 是 12 / 12 / 12 / 6，改后 6 / 6 / 6 / 6。
// 按数值写的断言会被「顺手改成 6px」过掉，而 6px 恰恰是错的答案：它分不清
// 「归零」和「又叠了一层」。所以这里算的是**最终生效值**。

const SRC = resolve('ui/src');

/** Vue 内建包装：大写，但不是本仓组件，根元素类要从它自己身上找。 */
const BUILTIN_WRAPPERS = new Set([
  'Transition',
  'TransitionGroup',
  'KeepAlive',
  'Teleport',
  'Suspense',
]);

/** @ui/src 下所有 `.css`，递归。 */
function cssFiles(dir = SRC, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = resolve(dir, entry.name);
    if (entry.isDirectory()) cssFiles(p, out);
    else if (extname(entry.name) === '.css') out.push(p);
  }
  return out;
}

/**
 * 把一段 CSS 拆成 `{selector, body}` 列表。**必须按花括号配对**，不能用
 * `/([^{}]+)\{([^}]*)\}/g`：那种写法遇到 `@media { .a { … } }` 会把内层规则
 * 整条吞掉（选择器截到 `@media (…)`，body 截到内层的第一个 `}`），于是
 * 媒体查询里的 margin-bottom 全部查不到。
 */
function ruleBlocks(css) {
  const out = [];
  let i = 0;
  while (i < css.length) {
    const open = css.indexOf('{', i);
    if (open < 0) break;
    const prelude = css.slice(i, open).trim();
    let depth = 1;
    let j = open + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === '{') depth++;
      else if (css[j] === '}') depth--;
      j++;
    }
    const inner = css.slice(open + 1, j - 1);
    // 条件 at-rule（@media / @supports / @container / @layer）里仍是规则；
    // @keyframes / @font-face 的内层是声明块，不是选择器，不能当规则收。
    if (prelude.startsWith('@')) {
      if (/^@(media|supports|container|layer)\b/.test(prelude)) out.push(...ruleBlocks(inner));
    } else {
      out.push({ selector: prelude, body: inner });
    }
    i = j;
  }
  return out;
}

/** CSS 特异度 (a,b,c)，够用即可。 */
function specificity(selector) {
  const s = selector.replace(/\([^)]*\)/g, '');
  const ids = (s.match(/#[\w-]+/g) || []).length;
  const classes = (s.match(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+/gi) || []).length;
  const elements = (s.match(/(^|[\s>+~,])\s*[a-z][\w-]*/gi) || []).length;
  return ids * 10000 + classes * 100 + elements;
}

/** 选择器的**主语部分**（最后一个简单选择器）里的类——后代里的类不算元素自身。 */
function subjectClasses(selector) {
  const out = new Set();
  for (const part of selector.split(',')) {
    const subject = part.trim().split(/\s+/).pop();
    if (!subject || /^[:[]/.test(subject)) continue;
    for (const m of subject.matchAll(/\.([A-Za-z][\w-]*)/g)) out.add(m[1]);
  }
  return out;
}

/** 该类集合构成的元素，最终生效的 `margin-bottom`（没有则为 null）。 */
function effectiveMarginBottom(classSet, rules) {
  let best = null;
  for (const rule of rules) {
    const value = (rule.body.match(/(?<!-)margin-bottom:\s*([^;}]+)/) || [])[1];
    if (value === undefined) continue;
    const classes = subjectClasses(rule.selector);
    if (classes.size === 0) continue;
    // 主语里的类必须**全部**在这组元素身上，否则这条规则不适用。
    if (![...classes].every((c) => classSet.has(c))) continue;
    const spec = specificity(rule.selector);
    if (!best || spec > best.spec) best = { spec, value: value.trim() };
  }
  return best && best.value;
}

/** 从 `class="a b c"` 里取出类名数组。 */
function classesOf(attrs) {
  const raw = (attrs.match(/class="([^"]*)"/) || [])[1] || '';
  // 用 `m[0]` 而不是解构 `[, cls]`：这里没有捕获组，matchAll 产出长度 1 的
  // 数组，`[, cls]` 拿到的是 undefined。收上来是一组 `undefined`，Set 仍
  // 非空，判据照样显示「识别出 1 个子元素」，报错离真正的原因十万八千里。
  return [...raw.matchAll(/[\w-]+/g)].map((m) => m[0]);
}

/** 每个 `.vue` 的**根元素**类名，按组件名索引。多根组件的每个根都会收进来。 */
function rootClassesByComponent(dir = SRC, out = new Map()) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = resolve(dir, entry.name);
    if (entry.isDirectory()) rootClassesByComponent(p, out);
    else if (extname(entry.name) === '.vue') {
      const src = readFileSync(p, 'utf8');
      const tpl = src.indexOf('<template>');
      if (tpl < 0) continue;
      const body = src.slice(src.indexOf('\n', tpl));
      const classes = new Set();
      // 模板内层缩进统一为 2 空格，根元素恰好是 `^ {2}<tag`；嵌套的更深。
      for (const [, tag, attrs] of body.matchAll(/^ {2}<([a-z][\w-]*)\b([^>]*)>/gm)) {
        if (BUILTIN_WRAPPERS.has(tag) || tag === 'template') continue;
        for (const cls of classesOf(attrs)) classes.add(cls);
      }
      out.set(basename(p, '.vue'), classes);
    }
  }
  return out;
}

/**
 * 四个面板的 `.panel` 里，**直接**子元素的类集合。
 *
 * 直接子元素 = 模板里缩进最浅的那些标签。这比「数 4 个空格」更贴近运行时：
 * `<Transition>` 的子节点同样被渲染成 `.panel` 的直接 flex item，数固定缩进
 * 会把它漏掉——而那正好是「首次运行引导」那张 callout 的位置。
 *
 * 自定义组件（大写）要续查一层拿根元素上的类：Vue 的多根组件会把每个根都
 * 提升成父容器的 flex item。
 */
function panelChildren() {
  const roots = rootClassesByComponent();
  const found = new Map(); // 类集合（排序后作键） → 出处
  const panels = [
    'shell/OverviewPanel.vue',
    'skills/SkillsPanel.vue',
    'plugins/PluginsPanel.vue',
    'shell/SettingsPanel.vue',
  ];
  for (const rel of panels) {
    const src = readFileSync(resolve(SRC, rel), 'utf8');
    const start = src.indexOf('<section class="panel">');
    assert.ok(start >= 0, `${rel} 应当有 .panel 容器`);
    const section = src.slice(start, src.lastIndexOf('</section>'));

    const tags = [...section.matchAll(/^( +)<([a-zA-Z][\w-]*)\b([^>]*)>/gm)];
    assert.ok(tags.length, `${rel} 的 .panel 里没解析出标签`);
    const indent = Math.min(...tags.map((t) => t[1].length));

    for (const [, pad, tag, attrs] of tags) {
      if (pad.length !== indent) continue;
      const where = `${rel} 的 .panel 直接子元素 <${tag}>`;
      const set = new Set(classesOf(attrs));
      if (/^[A-Z]/.test(tag) && !BUILTIN_WRAPPERS.has(tag)) {
        const compRoots = roots.get(tag);
        assert.ok(compRoots && compRoots.size, `${where} 的组件根元素没解析出类名`);
        for (const cls of compRoots) set.add(cls);
      }
      if (set.size === 0) continue;
      found.set([...set].sort().join(' '), { where, set });
    }
  }
  return found;
}

test('.panel 的 gap 是 6px（功能块默认间距的落点）', () => {
  const theme = readFileSync(resolve(SRC, 'theme.css'), 'utf8');
  const panel = theme.match(/^\.panel \{([^}]*)\}/m);
  assert.ok(panel, '必须能找到 .panel');
  assert.match(panel[1], /gap:\s*6px/, '.panel 的 gap 必须是 6px');
});

test('.panel 的任何一个直接子元素都不自带 margin-bottom（按层叠后的生效值判）', () => {
  const rules = [];
  for (const file of cssFiles()) {
    for (const rule of ruleBlocks(readFileSync(file, 'utf8'))) {
      rules.push({ ...rule, file });
    }
  }

  const children = panelChildren();
  // 四个面板加起来至少 6 种不同的类组合（概览 2 + 插件 2 + 技能 1 + 设置 2）。
  assert.ok(children.size >= 6, `应至少识别出 6 个 .panel 子元素组合，实际 ${children.size}`);

  const offenders = [];
  for (const [key, { where, set }] of children) {
    const value = effectiveMarginBottom(set, rules);
    if (value && !/^(0|none)$/.test(value)) offenders.push(`  [${key}] → margin-bottom: ${value}  (${where})`);
  }
  assert.equal(
    offenders.length,
    0,
    `这些 .panel 子元素带有生效的下外边距，会叠在 gap 上、造成同一列两种缝：\n${offenders.join('\n')}`
  );
});

test('控制塔三张卡的下外边距显式写 0（删掉声明等于让基线的 10px 回来）', () => {
  const css = readFileSync(resolve(SRC, 'diagnostics/diagnostics.css'), 'utf8');
  const tower = css.match(/\.diag-card\.diag-card--tower \{([^}]*)\}/);
  assert.ok(tower, '必须能找到 .diag-card.diag-card--tower 覆盖规则');
  assert.match(
    tower[1],
    /margin-bottom:\s*0(px)?\s*;/,
    '作用域选择器只在它自己声明过的属性上赢过基线，必须显式写 margin-bottom: 0'
  );
});

test('诊断页（无 gap 的容器）仍由卡片自带 margin，不受本约定影响', () => {
  const css = readFileSync(resolve(SRC, 'diagnostics/diagnostics.css'), 'utf8');
  // `.diagnosis` 是另一套容器（position: fixed，不吃 `.panel` 的 gap），
  // 那 5 个独立窗口的间距仍来自基线 `.diag-card` 的 10px。一旦连基线也归零，
  // 它们会完全贴死——所以这条守的是「别顺手把基线也改了」。
  const base = css.match(/^\.diag-card \{([^}]*)\}/m);
  assert.ok(base, '必须能找到 .diag-card 基线');
  assert.match(base[1], /margin-bottom:\s*10px/, '诊断页仍依赖基线的 10px 下外边距');
});

test('约定本身写在 ui/AGENTS.md 里，不只是代码里', () => {
  const doc = readFileSync(resolve('ui/AGENTS.md'), 'utf8');
  assert.match(doc, /功能块之间的默认间距是 6px/, 'ui/AGENTS.md §间距 必须写明默认 6px');
  assert.match(doc, /由容器统一提供/, '必须写明间距由容器提供、块不带外边距');
  assert.match(doc, /除非用户明确要求特调/, '必须写明这是默认值、可被用户特调覆盖');
});