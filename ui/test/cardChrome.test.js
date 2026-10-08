import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { extname, resolve } from 'node:path';
import test from 'node:test';
import { SRC, allRules, classesOf, effectiveDeclaration } from './lib/css-cascade.mjs';

// 卡片标题与内容之间必须有 1px 分割线（2026-10-06 用户拍板，见 ui/AGENTS.md §卡片）。
//
// 这条判据有两个方向，缺一不可：
//   ① 该有的地方要有——概览页与四个面板里每张卡的标题都必须真的画出那条线；
//   ② 不该有的地方不能有——5 个独立诊断窗口明确不动，它们的标题必须**仍然无线**。
// 只钉 ① 的话，一次「顺手把基线 .diag-card__title 也加上线」就会静默改掉那 5
// 个窗口；而那正是用户明令禁止的顺带变更。

const rules = () => allRules();
const read = (p) => readFileSync(resolve(SRC, p), 'utf8');

/** 面板内所有卡的标题元素类集合（`.card > h2` / `.card-head` / `--tower` 的标题）。 */
function panelCardTitles() {
  const found = [];
  const panels = [
    'shell/OverviewPanel.vue',
    'skills/SkillsPanel.vue',
    'plugins/PluginsPanel.vue',
    'shell/SettingsPanel.vue',
    'kernel/VersionsPanel.vue',
  ];
  for (const rel of panels) {
    const src = read(rel);
    for (const [, attrs] of src.matchAll(/class="(card-head|card-head-toggle)"/g)) {
      found.push({ set: new Set(classesOf(` class="${attrs}"`)), where: `${rel} 的 ${attrs}` });
    }
  }
  // 概览页的两张卡用裸 h2（一个带 kernel-title、一个带 card-title-with-tip）。
  // `.card > h2` 这条规则的主语里**一个类都没有**、只有标签 `h2`，所以元素侧
  // 必须把 `h2` 也算成一个 token，否则整条规则会被当成「不适用」而跳过。
  // 缩进用 `\s+` 而不是 `\s{6}`：uiv2 改版把概览页的卡移进了栅格列容器，
  // 缩进随之变化。钉的是「这是个裸 h2 卡标题」，不是「它缩进几格」。
  for (const rel of ['shell/OverviewPanel.vue', 'shell/SettingsPanel.vue']) {
    for (const [, cls] of read(rel).matchAll(/^\s+<h2 class="([^"]*)"/gm)) {
      found.push({
        set: new Set(['card', 'h2', ...classesOf(` class="${cls}"`)]),
        where: `${rel} 的 <h2 class="${cls}">`,
      });
    }
  }
  return found;
}

test('概览页与四个面板里每张卡的标题都真的画出了分割线（查层叠后的生效值）', () => {
  const rs = rules();
  const titles = panelCardTitles();
  assert.ok(titles.length >= 8, `应识别出至少 8 处卡标题，实际 ${titles.length}`);

  const missing = [];
  for (const { set, where } of titles) {
    const value = effectiveDeclaration(set, rs, 'border-bottom');
    if (!value || !/^1px/.test(value)) missing.push(`  [${[...set].join(' ')}] → border-bottom: ${value ?? '(无)'}  (${where})`);
  }
  assert.equal(missing.length, 0, `这些卡标题没有生效的 1px 下边线：\n${missing.join('\n')}`);
});

test('控制塔标题的线走刻蚀线，与概览另两张卡同源；面板里的卡仍是 1px', () => {
  const rs = rules();
  const tower = new Set(['diag-card', 'diag-card--tower', 'diag-card__title', 'h3']);
  const cardHead = new Set(['card-head', 'div']);

  // **这条断言的形状 2026-10-08 变了**：原先是「控制塔与面板卡片的标题线必须
  // 写成同一个值（1px solid var(--border)）」。用户随后把概览这一屏的线定案成
  // 2px 两端收细、颜色读 `--divider-strong`，**「统一」的口径随之从
  // 「全前端一个值」收成「概览这一屏一个值」**——这是用户明知的范围取舍
  // （先只调概览），不是漂移。面板里的卡仍走 `.card-head` 的 1px。
  //
  // 原生 `border-bottom` 必须退场为 0：线换成了 `clip-path` 的伪元素，
  // border 没法 clip，两者是二选一。
  assert.equal(
    effectiveDeclaration(tower, rs, 'border-bottom'),
    '0',
    '控制塔标题的原生 border 必须退场 0——线已经交给伪元素的 clip-path'
  );
  assert.equal(
    effectiveDeclaration(cardHead, rs, 'border-bottom'),
    '1px solid var(--border)',
    '面板里其余卡的标题线不受概览这次调整影响'
  );

  // 真正要守的是「概览这一屏的线同源」：控制塔三张与 kernel-card / usage-card
  // 读的是同一个 `--divider-strong`，收细比例也一致。
  const diagCss = read('diagnostics/diagnostics.css');
  const before = /\.diag-card--tower \.diag-card__title:not\(\.diag-card__title--bare\)::before\s*\{([^}]*)\}/.exec(diagCss);
  assert.ok(before, 'diagnostics.css 里要有带 :not(--bare) 的卡头 ::before');
  assert.match(before[1], /background:\s*var\(--divider-strong\)/, '控制塔的线必须与概览另两张卡读同一个 token');
  assert.match(before[1], /height:\s*2px;/);
  const ov = read('shell/OverviewPanel.vue');
  const ovLine = /\.kernel-card \.card-head::before,\s*\.usage-card \.card-head::before\s*\{([^}]*)\}/s.exec(ov);
  assert.ok(ovLine, 'OverviewPanel 里要有两张卡的卡头 ::before');
  assert.match(ovLine[1], /background:\s*var\(--divider-strong\)/);
  // 六边形比例必须逐字相同——同一个六边形在两个文件里各写了一份
  // （OverviewPanel 的 scoped 块够不到 ControlTower 的元素），只改一处就会
  // 在同一屏里出现两种收细比例。
  const poly = /clip-path:\s*(polygon\([^)]*\))/.exec(before[1])[1];
  assert.ok(ovLine[1].includes(poly), `两处的收细六边形必须一致，期望 ${poly}`);
});

test('标题的 padding-bottom 是 9px（线与标题文字之间的留白，与面板里的卡一致）', () => {
  const rs = rules();
  const tower = effectiveDeclaration(
    new Set(['diag-card', 'diag-card--tower', 'diag-card__title', 'h3']),
    rs,
    'padding-bottom'
  );
  const cardHead = effectiveDeclaration(new Set(['card-head', 'div']), rs, 'padding-bottom');
  // 6px → 9px：卡片内边距从 12px 收到 12px 的同时把标题字号从 15px 降到 14px，
  // 行盒跟着变矮，留白不跟着收会让分割线贴着文字。9px 是与新设计稿
  // `.page-card__head` 的头部高度对齐后的值，两处仍然必须相等。
  assert.equal(tower, '9px', `控制塔标题的 padding-bottom 应为 9px，实际 ${tower ?? '(无)'}`);
  assert.equal(tower, cardHead, '两处标题的 padding-bottom 必须一致');
});

test('标题的 margin-bottom 归控制塔自己定，且必须显式写 6px', () => {
  const css = read('diagnostics/diagnostics.css');
  const override = css.match(/\.diag-card--tower \.diag-card__title \{([^}]*)\}/);
  assert.ok(override, '必须能找到 .diag-card--tower .diag-card__title 覆盖规则');
  // 这条规则改过一次方向，两种写法各自都会出错，所以钉的是**结果**而不是
  // 「删不写 margin-bottom」这个手法：
  //   · 早先要求删掉覆盖、回到基线 8px，理由是 8 与 `.card` 基线 gap 是同一个数，
  //     补线时另写一个 5px 就成了凭空多出来的第二个数。
  //   · 2026-10-07 用户要求继续压纵向空白，于是控制塔整体降到「卡 4px / 行 4px」
  //     这一档，它的「线到内容」也该跟着降到 6px——继续挂 8px 会让收紧只做了一半。
  // 现在两种「写法」都会红：写 5px / 8px 这类随手值，或干脆删掉让标题回基线。
  assert.match(
    override[1],
    /margin-bottom:\s*6px/,
    `控制塔标题的「线到内容」应为 6px，实际 ${override[1].trim()}`
  );
  // 基线仍是 8px：其余五个诊断窗口与别的卡片都靠它，一个数都不许动。
  const base = css.match(/^\.diag-card__title \{([^}]*)\}/m);
  assert.ok(base, '必须能找到 .diag-card__title 基线');
  assert.match(base[1], /margin:\s*0 0 8px/, '基线的 margin 应为 0 0 8px（其余卡片靠它）');
});

test('5 个独立诊断窗口的标题仍然无线（用户 2026-10-06 明确要求不动）', () => {
  const css = read('diagnostics/diagnostics.css');
  const base = css.match(/^\.diag-card__title \{([^}]*)\}/m);
  assert.ok(base, '必须能找到 .diag-card__title 基线');
  // 基线一旦加上 border-bottom，那 5 个独立窗口会一起变——那是明令禁止的
  // 顺带变更。`.diag-card` / `.diag-row` 被它们共用（KernelStatus / Startup /
  // Plugin / Operation / DiagnosisShell），装在 `.diagnosis` 容器里。
  assert.doesNotMatch(
    base[1],
    /border/,
    '基线 .diag-card__title 不得带 border：它同时决定 5 个独立诊断窗口的标题线'
  );
});

test('控制塔的线只挂在 --tower 上，不从基线漏过去', () => {
  const css = read('diagnostics/diagnostics.css');
  assert.match(
    css,
    /\.diag-card--tower \.diag-card__title \{[^}]*border-bottom/,
    '分割线必须写在 --tower 限定选择器上'
  );
  // --tower 只被 ControlTower 用，而 ControlTower 只被 OverviewPanel 用。
  const tower = read('diagnostics/ControlTower.vue');
  assert.equal(
    (tower.match(/diag-card--tower/g) || []).length,
    3,
    '控制塔仍是三张卡，且都挂了 --tower'
  );
  const overview = read('shell/OverviewPanel.vue');
  assert.ok(overview.includes('ControlTower'), 'ControlTower 仍只出现在概览页');
});

test('空态不再单开正文行；基线 .diag-empty 的 24px 留给 5 个诊断窗口', () => {
  const css = read('diagnostics/diagnostics.css');
  // 改完这一轮之后 `.diag-empty` 只剩一处用法：5 个独立诊断窗口的
  // `RunTimeline.vue`。用户 2026-10-06 明确要求那 5 页不动，而基线
  // `padding: 24px 8px` 正是它靠的——把 24 写小会让那 5 页的空时间线忽然
  // 贴到卡片边。
  const base = css.match(/^\.diag-empty \{([^}]*)\}/m);
  assert.ok(base, '必须能找到 .diag-empty 基线');
  assert.match(base[1], /padding:\s*24px 8px/, '基线的 24px 上下留白属于 5 个诊断窗口，不许被收');

  // 概览页不再用这个类：空态说明已经收进标题行右侧。多一句「概览页的覆盖规则
  // 还在」就会有人把 6px 当成有意义的调参旋钮接着往下压，而它已经不匹配任何
  // 元素了——一条不匹配任何元素的规则比没有规则更难查。
  const tower = read('diagnostics/ControlTower.vue');
  assert.doesNotMatch(tower, /diag-empty/, 'ControlTower 不该再有独立的 .diag-empty 正文行');
  assert.doesNotMatch(
    css,
    /\.diag-card--tower \.diag-empty/,
    '概览页已不用 .diag-empty，--tower 的覆盖规则是死代码'
  );

  // 真正省下来的是标题区那三项，不是空态正文自己的 padding：4 + 17.55 + 4 = 25.55，
  // 正好等于有记录时单行的行高。三项都要清——只清 padding-bottom 的话，分割线
  // 下方那 6px 外边距照样把卡撑高，看起来却像是「线还在、内容没了」。
  const bare = css.match(/\.diag-card\.diag-card--tower \.diag-card__title--bare \{([^}]*)\}/);
  assert.ok(bare, '必须能找到空态专用的 --bare 标题规则');
  for (const prop of ['padding-bottom: 0', 'margin-bottom: 0', 'border-bottom: 0']) {
    assert.match(
      bare[1].replace(/\s+/g, ' '),
      new RegExp(prop.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
      `--bare 必须清掉 ${prop}，实际 ${bare[1].trim()}`
    );
  }
  // 特异度必须高于 `.diag-card--tower .diag-card__title`（0,2,0），否则同特异度
  // 只能靠「写在后面」取胜，而文件里后来插一条规则就会静默失效。
  assert.match(
    css,
    /\.diag-card\.diag-card--tower \.diag-card__title--bare/,
    '选择器要多带一段 .diag-card，靠特异度压过 --tower 的标题规则，不靠源序'
  );

  // 修饰类挂在标题上而不是卡片上：叫 `diag-card--tower-empty` 会含 `--tower`
  // 子串，把上面那条「三张卡都挂了 --tower」的计数变成 4。
  assert.match(tower, /diag-card__title--bare/, '空态修饰类应挂在标题上');
  assert.doesNotMatch(tower, /diag-card--tower-empty/, '别给卡片起带 --tower 子串的修饰类名');
});

test('控制塔的卡 padding 与行 padding 收到紧凑档（2026-10-07 用户）', () => {
  const css = read('diagnostics/diagnostics.css');
  const card = css.match(/\.diag-card\.diag-card--tower \{([^}]*)\}/);
  assert.ok(card, '必须能找到 .diag-card.diag-card--tower');
  // 纵向 4px、横向仍 6px：这次只收**高度**，横向那 6px 是卡片内文的左右留白，
  // 跟着收会让读数贴到描边上。写成 `padding: 4px` 会把横向一起清掉。
  assert.match(
    card[1],
    /padding:\s*4px 6px/,
    `控制塔卡内边距应为「纵向 4px / 横向 6px」，实际 ${card[1].trim()}`
  );

  const row = css.match(/\.diag-card--tower \.diag-row \{([^}]*)\}/);
  assert.ok(row, '必须能找到 .diag-card--tower .diag-row');
  assert.match(row[1], /padding-block:\s*4px/, `控制塔行内边距应为 4px，实际 ${row[1].trim()}`);
  // 行高不收：1.35（13px → 17.55px）已经是密集面板的可读下限，再收会顶到
  // 「看不出哪里能点」那条线——收紧靠 padding，不靠压行高。
  assert.match(row[1], /line-height:\s*1\.35/, '控制塔行高保持 1.35，不再往下收');
});

test('标题「线到内容」的留白是 6px，而「标题到线」守住全前端统一的 9px（2026-10-07 用户 / uiv2 改版）', () => {
  const css = read('diagnostics/diagnostics.css');
  const title = css.match(/\.diag-card--tower \.diag-card__title \{([^}]*)\}/);
  assert.ok(title, '必须能找到 .diag-card--tower .diag-card__title');

  // 「线到内容」由 margin-bottom 决定，6px。这条正是用户截图里第一个箭头指的
  // 那段空白。ui/AGENTS.md §卡片把这一段明确交给各卡片自己的间距机制，
  // 所以控制塔自己定 6px 不算破规范。
  assert.match(
    title[1],
    /margin-bottom:\s*6px/,
    `标题线到内容的留白应为 6px，实际 ${title[1].trim()}`
  );
  // 「标题到线」是 ui/AGENTS.md §卡片里**全前端统一**的一条（概览页与四个面板
  // 的卡共用），单给控制塔另定一个数会让上下相邻的卡在同一屏里给出两种分割线
  // 间距。uiv2 改版把它从 6px 提到 9px：卡内边距与标题字号都变了，6px 会让线
  // 贴住标题文字。
  assert.match(
    title[1],
    /padding-bottom:\s*9px/,
    '标题文字到分割线的 9px 是全前端统一规范，不许只给控制塔改'
  );
});

test('内核状态页不再摆底部按钮栏，两个动作都留在顶部（2026-10-07 用户）', () => {
  const vue = read('diagnostics/KernelStatusDiagnosis.vue');
  const shell = read('diagnostics/DiagnosisShell.vue');
  const menu = read('diagnostics/diagnosis-more-menu.js');

  // ① 页面里不许再有 .diag-actions：两个按钮都已在顶部有入口（下方 ②③）。
  //    这条挡的是「删掉按钮、却又手滑加回来」——重复入口不是排版问题，
  //    是同一动作有两条实现路径。
  //    **只扫类名、不扫裸字符串**：模板位置上必须留着那段注释说明为什么删，
  //    而注释里出现 `.diag-actions` 这个词是应该的（第一版判据写成
  //    `doesNotMatch(vue, /diag-actions/)`，结果被自己那段注释判红）。
  assert.doesNotMatch(
    vue,
    /class="diag-actions"/,
    '内核状态页不许再有底部按钮栏（.diag-actions），两个动作都留在顶部'
  );

  // ② 「刷新状态」：头部 ⟳ 走 DiagnosisShell 的 onRefresh，它对 kernel 分派
  //    到 loadKernelStatusDiagnosis(true)；更多菜单的 refresh 项也走同一个
  //    onRefresh。曾经页面底部那份自己调 loadKernelStatusDiagnosis，
  //    **绕过 onRefresh**——与菜单里那句「不是两份实现」正好相反。
  assert.match(
    shell,
    /kind === 'kernel'\) loadKernelStatusDiagnosis\(true\)/,
    '顶部 ⟳ 必须经 onRefresh 分派到内核状态读取'
  );
  assert.match(
    menu,
    /key: 'refresh', label: '刷新状态'/,
    '更多菜单里必须保留「刷新状态」项'
  );

  // ③ 「查看完整日志」：更多菜单第一项走
  //    openEvidence(evidencePath)；原先页面底部那份是 openEvidence()，
  //    无参时内部取的正是同一个 evidencePath，所以删掉不改变行为。
  assert.match(menu, /key: 'logs', label: '查看完整日志'/, '更多菜单里必须保留「查看完整日志」项');
  const actions = read('diagnostics/diagnostic-actions.js');
  assert.match(
    actions,
    /path \|\| diagnosticStore\.evidencePath/,
    'openEvidence 无参时必须落到 store 的 evidencePath，否则删掉页面底部入口会改变行为'
  );
});

test('其余 3 个诊断页的 .diag-actions 仍在（它们的入口还没进顶部菜单）', () => {
  const users = ['diagnostics/StartupDiagnosis.vue', 'diagnostics/OperationDiagnosis.vue', 'diagnostics/PluginDiagnosis.vue'];
  for (const rel of users) {
    assert.match(read(rel), /class="diag-actions"/, `${rel} 的底部操作栏不应被顺手删掉`);
  }
  // 类本身也不能从 CSS 里消失。
  assert.match(read('diagnostics/diagnostics.css'), /^\.diag-actions \{/m);
});

test('规范本身写在 ui/AGENTS.md 里，不只是代码里', () => {
  const doc = readFileSync(resolve('ui/AGENTS.md'), 'utf8');
  assert.match(doc, /每张卡片的标题与内容之间必须有 1px 分割线/, 'ui/AGENTS.md 必须写明标题分割线这条规范');
  assert.match(doc, /border-bottom:\s*1px solid var\(--border\)/, '必须写明唯一的写法');
  assert.match(doc, /不要为标题线另开一个更深的 token/, '必须写明色值只有 --border 一种');
  assert.match(doc, /5 个独立诊断窗口/, '必须写明覆盖范围不含那 5 个独立窗口');
});
