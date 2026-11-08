import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 2026-11-08 用户要求两件事：① 技能页那条「MiniMax 接入共享技能库」从独立整行
// 收进卡片标题栏；② UI 上「MiniMax」的品牌字样用手写体。两条都是纯观感约定，
// 不写判据的话下一次改版会静默回到原样（字号 / 字族又是最容易被顺手继承掉的）。

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
  // 手写体是品牌字样专用的：全仓只有 .minimax-wordmark 一条消费方。
  assert.equal(
    theme.match(/var\(--font-hand\)/g).length,
    1,
    '手写体只服务 MiniMax 字样，别处不许借用',
  );
});

test('品牌字样类只读偏差 token，三个调用点都只挂这一个类', () => {
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
  ];
  for (const path of callers) {
    const src = read(path);
    assert.match(src, /minimax-wordmark/, `${path} 里的 MiniMax 字样要挂 .minimax-wordmark`);
    // 字族不许各写一份：换字体时要一处生效，而不是逐个文件找。
    assert.doesNotMatch(src, /font-hand/, `${path} 不该自己拼手写体字族`);
  }
});

test('内核切换器里只有 MiniMax 那一段用手写体，DSH 保持正文', () => {
  const familySwitch = read('ui/src/kernel/FamilySwitch.vue');
  assert.match(familySwitch, /:class="\{ 'minimax-wordmark': item\.experimental \}"/);
});

test('插件页的内核目标标签与技能页同用一个品牌字样类', () => {
  const target = read('ui/src/plugins/MinimaxPluginTarget.vue');
  assert.match(target, /<span class="minimax-wordmark">\{\{/);
});

test('技能页的共享技能库开关收进「已安装」卡头，不再占独立整行', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  const cardStart = panel.indexOf('<div class="card entity-card">');
  assert.ok(cardStart > 0, '找不到「已安装」卡片');
  const cardHead = panel.indexOf('<div class="card-head">', cardStart);
  const list = panel.indexOf('class="entity-list"', cardStart);
  const link = panel.indexOf('class="mcode-link-head"', cardStart);
  assert.ok(cardHead > 0 && list > 0, '卡片结构变了，先确认这条判据还量得到');
  assert.ok(
    link > cardHead && link < list,
    '接入开关必须落在卡头里（列表之前），这是「缩小放到标题栏」的最小实现',
  );
  assert.doesNotMatch(panel, /class="mcode-link-row"/, '独立整行的旧形态必须删掉');
  assert.doesNotMatch(panel, /\.mcode-link-row\s*\{/, '旧行的样式也要删，留着是没有宿主的选择器');
  // 卡头里是压缩档：状态词小一档、芯片里的词用手写体、整句说明仍在 ⓘ。
  const headSlice = panel.slice(link, list);
  assert.match(headSlice, /class="minimax-wordmark"/, '芯片里的 MiniMax 字样要用手写体');
  assert.match(headSlice, /mcodeSkillNote/, '整句共享语义与生效时机仍在 ⓘ 里');
  assert.match(headSlice, /size="small"/, '开关在卡头里压到 small');
  assert.match(headSlice, /接入共享技能库/, '「接入什么」这句话不能只剩一个光开关');
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
      escaped + ':not\\(\\.minimax-wordmark\\)\\s*\\{[^}]*\\}',
    ).exec(style);
    assert.ok(guardedRule, path + ' 要用 :not(.minimax-wordmark) 给其余文字上色，避免盖掉品牌色');
    assert.ok(
      guardedRule[0].includes('color: ' + textToken),
      path + ' 的其余文字仍要保留自己的颜色：' + guardedRule[0],
    );
  }
});
