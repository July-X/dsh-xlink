import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 2026-11-08 用户要求 UI 上「MiniMax」的品牌字样用手写体。纯观感约定，不写判据
// 的话下一次改版会静默回到原样（字号 / 字族又是最容易被顺手继承掉的）。
// 同一天收进技能页卡头的「接入共享技能库」总开关已于 2026-11-09 按用户要求移除，
// 这里不再管它的摆位（逐技能目标开关由 skillEnable.test.js 钉住）。

const read = (path) => readFileSync(path, 'utf8');
const theme = read('ui/src/theme.css');
const root = theme.match(/:root\s*\{([\s\S]*?)\n\}/)[1];
const declaration = (block, name) =>
  block.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1].trim();

test('手写体 token 只定义一次，取值是系统手写体 + cursive 兜底', () => {
  const stack = declaration(root, '--font-hand');
  assert.ok(stack, ':root 必须定义 --font-hand');
  // macOS 与 Windows 各取一个真实存在的系统手写体；缺字体时回落 cursive，
  // 而不是悄悄变回 --font-ui。
  assert.match(stack, /"Bradley Hand"/, 'macOS 手写体');
  assert.match(stack, /"Segoe Print"/, 'Windows 手写体');
  assert.match(stack, /cursive$/, '最后必须回落 cursive');
  assert.equal(theme.match(/--font-hand:/g).length, 1, '--font-hand 只许定义一次');
  // 手写体的消费方只有两条品牌字样类，加侧栏英文品牌名（.brand__name，
  // 2026-11-08 用户要求英文手写、中文不变）：别处不许借用。
  assert.equal(
    theme.match(/var\(--font-hand\)/g).length,
    3,
    '手写体只服务 MiniMax / DeepSeek 两个品牌词与侧栏英文品牌名，别处不许借用',
  );
});

test('品牌字样类只读偏差 token，调用点都只挂这一个类', () => {
  const rule = theme.match(/\.minimax-wordmark\s*\{([^}]*)\}/);
  assert.ok(rule, '缺少 .minimax-wordmark 规则');
  assert.match(rule[1], /font-family:\s*var\(--font-hand\)/);
  // 品牌词颜色也挂在词本身（设计 §1「手写体 + 现有实验色粉色」）：芯片不覆盖
  // 字族或颜色，所以这两条都只能在 .minimax-wordmark 上，两套主题共用 --experimental。
  assert.match(rule[1], /color:\s*var\(--experimental\)/);
  const dark = theme.match(/html\.dark\s*\{([\s\S]*?)\n\}/)[1];
  assert.ok(declaration(root, '--experimental'), '浅色主题必须有 --experimental');
  assert.ok(declaration(dark, '--experimental'), '深色主题必须有 --experimental；只定义一套时另一套下整条失效');

  const callers = [
    'ui/src/kernel/FamilySwitch.vue',
    'ui/src/plugins/MinimaxPluginTarget.vue',
    'ui/src/skills/SkillsPanel.vue',
    'ui/src/shell/SettingsPanel.vue',
  ];
  for (const path of callers) {
    const src = read(path);
    assert.match(src, /minimax-wordmark/, `${path} 里的 MiniMax 字样要挂 .minimax-wordmark`);
    // 字族不许各写一份：换字体时要一处生效，而不是逐个文件找。
    assert.doesNotMatch(src, /font-hand/, `${path} 不该自己拼手写体字族`);
  }
});

test('内核切换器里两个品牌词各用手写体：DSH 那侧写 DeepSeek', () => {
  const familySwitch = read('ui/src/kernel/FamilySwitch.vue');
  assert.match(familySwitch, /label: 'DeepSeek'/);
  assert.match(familySwitch, /'minimax-wordmark' : 'deepseek-wordmark'/);
});

test('DeepSeek 品牌字样：与 MiniMax 同一手写体，颜色走品牌蓝 token', () => {
  const rule = theme.match(/\.deepseek-wordmark\s*\{([^}]*)\}/);
  assert.ok(rule, '缺少 .deepseek-wordmark 规则');
  assert.match(rule[1], /font-family:\s*var\(--font-hand\)/);
  assert.match(rule[1], /color:\s*var\(--brand-deepseek\)/);
  const dark = theme.match(/html\.dark\s*\{([\s\S]*?)\n\}/)[1];
  assert.ok(declaration(root, '--brand-deepseek'), '浅色主题必须有 --brand-deepseek');
  assert.ok(declaration(dark, '--brand-deepseek'), '深色主题必须有 --brand-deepseek；只定义一套时另一套下整条失效');
});

test('DeepSeek 品牌字样挂在这些位置，且组件不自己拼字族', () => {
  const callers = [
    'ui/src/kernel/FamilySwitch.vue',
    'ui/src/skills/SkillsPanel.vue',
    'ui/src/memory/MemoryPanel.vue',
    'ui/src/memory/MemoryEditor.vue',
    'ui/src/plugins/PluginsPanel.vue',
    'ui/src/shell/SettingsPanel.vue',
  ];
  for (const path of callers) {
    const src = read(path);
    assert.match(src, /deepseek-wordmark/, `${path} 的 DeepSeek 字样要挂 .deepseek-wordmark`);
    // 字族不许各写一份：换字体时要一处生效，而不是逐个文件找。
    assert.doesNotMatch(src, /font-hand/, `${path} 不该自己拼手写体字族`);
  }
});

test('插件页的内核目标标签与技能页同用一个品牌字样类', () => {
  const target = read('ui/src/plugins/MinimaxPluginTarget.vue');
  assert.match(target, /<span class="minimax-wordmark">\{\{/);
});

test('设置页端口分组标题的 MiniMax 也用手写体', () => {
  // 2026-11-10 用户截图：同一张卡里「MiniMax 内核体验」是手写体，下面端口分组
  // 的 legend 却是正文衬线字；品牌词在同一屏里出现两种字形，读起来像两个产品。
  const settings = read('ui/src/shell/SettingsPanel.vue');
  assert.match(
    settings,
    /<legend><span class="minimax-wordmark">MiniMax<\/span> <span class="settings-port-kind">实验内核<\/span><\/legend>/,
  );
});

test('同元素上的功能类不许把品牌色盖回去（scoped [data-v] 特异性更高）', () => {
  // 2026-11-08 实机截图：技能行与记忆页的 MiniMax 仍是灰色。根因是 .target-label /
  // .memory-target__name 与 .minimax-wordmark 挂在同一个元素上，组件 scoped 规则带
  // [data-v]（(0,2,0)）压过全局 (0,1,0) 的彩色声明。功能类必须用 :not 排除品牌词，
  // 颜色只留 theme.css 一处。
  const cases = [
    ['ui/src/skills/SkillsPanel.vue', '.target-label', 'var(--text-secondary)'],
    ['ui/src/memory/MemoryPanel.vue', '.memory-target__name', 'var(--text)'],
  ];
  for (const [path, selector, textToken] of cases) {
    const style = read(path).split('<style')[1].split('</style>')[0].replace(/\/\*[\s\S]*?\*\//g, '');
    const escaped = selector.replace(/[.*+?^$()|[\]\\]/g, '\\$&');
    assert.doesNotMatch(
      style,
      new RegExp(escaped + '\\s*\\{[^}]*\\bcolor\\s*:'),
      path + ' 的 ' + selector + ' 直接写 color 会在同元素上盖掉 .minimax-wordmark 的粉色',
    );
    const guardedRule = new RegExp(
      escaped + ':not\\(\\.minimax-wordmark\\)[^{]*\\{[^}]*\\}',
    ).exec(style);
    assert.ok(guardedRule, path + ' 要用 :not(.minimax-wordmark) 给其余文字上色，避免盖掉品牌色');
    assert.ok(
      guardedRule[0].includes('color: ' + textToken),
      path + ' 的其余文字仍要保留自己的颜色：' + guardedRule[0],
    );
  }
});
