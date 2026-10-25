// 「模型用量」窗口的**外部来源**能力：勾选哪些外部 Agent 工具（Codex、
// MiniMax Desktop）的对话 token 用量也算进来。
//
// 这条链路上有三件事单测能钉住、别处钉不住：
//  ① **默认不勾**——不勾就不读那些工具留在本机的目录（后端按参数决定读什么，
//     前端不能偷偷传 true）；
//  ② **勾选状态的真值在 Rust 侧**——前端那份只是镜像，每次拉取都以
//     `external_sources` 为准；
//  ③ **概览卡片不扫外部来源**——它不显示那些数据，不该为它付扫描成本。
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { allRulesIncludingVue, effectiveDeclaration } from './lib/css-cascade.mjs';

const calls = [];
let nextData = null;

const core = {
  invoke(cmd, args) {
    calls.push({ cmd, args });
    if (cmd === 'get_model_usage') return Promise.resolve(nextData);
    if (cmd === 'set_usage_external_sources') {
      return Promise.resolve(args.sources);
    }
    return Promise.resolve(null);
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const { usage, refreshUsage, loadUsageSummary, setExternalSources } = await import(
  '../src/usage/usage.js'
);

const windowSource = readFileSync(new URL('../src/usage/UsageWindow.vue', import.meta.url), 'utf8');
// 模板与样式都带大段解释「为什么这么写」的中文注释，注释里常原样引用被判据
// 的词——按整份文件找会把自己写的解释当成命中。只取模板段。
const template = windowSource.slice(
  windowSource.indexOf('<template>'),
  windowSource.indexOf('<style')
);

function reset() {
  calls.length = 0;
  nextData = null;
}

test('默认不勾选：概览卡片与窗口首读都不带任何外部来源', async () => {
  reset();
  nextData = { external_sources: { codex: false, minimax: false }, external: [] };
  await loadUsageSummary();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].cmd, 'get_model_usage');
  assert.deepEqual(
    calls[0].args.external,
    { codex: false, minimax: false },
    '概览卡片显式传全 false：它不显示外部来源，也不该为它付扫描成本'
  );

  reset();
  await refreshUsage();
  assert.equal(calls[0].args.external, null, '省略 = 用后端已保存的偏好');
});

test('勾选状态的真值在 Rust 侧：前端那份只是镜像', async () => {
  reset();
  nextData = { external_sources: { codex: true, minimax: false }, external: [] };
  await refreshUsage();
  assert.deepEqual(
    usage.externalSources,
    { codex: true, minimax: false },
    '勾选状态从后端返回的 external_sources 回填，不在前端另存一份'
  );
});

test('勾选后先持久化再重扫，两步的顺序不能反', async () => {
  reset();
  nextData = { external_sources: { codex: true, minimax: false }, external: [] };
  await setExternalSources({ codex: true, minimax: false });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].cmd, 'set_usage_external_sources');
  assert.deepEqual(calls[0].args.sources, { codex: true, minimax: false });
  assert.equal(calls[1].cmd, 'get_model_usage');
  assert.deepEqual(
    calls[1].args.external,
    { codex: true, minimax: false },
    '重扫时只扫刚勾上的那些来源'
  );
});

test('取消勾选也要落盘，并让重扫不再带那个来源', async () => {
  reset();
  nextData = { external_sources: { codex: false, minimax: false }, external: [] };
  await setExternalSources({ codex: false, minimax: false });
  assert.equal(calls[0].cmd, 'set_usage_external_sources');
  assert.deepEqual(calls[0].args.sources, { codex: false, minimax: false });
  assert.deepEqual(calls[1].args.external, { codex: false, minimax: false });
});

test('窗口模板上有两个 checkbox，且都接了勾选变化后的重扫', () => {
  const boxes = template.match(/<el-checkbox\b[\s\S]*?\/>/g) || [];
  assert.equal(boxes.length, 2, 'Codex 与 MiniMax Desktop 各一个');
  assert.ok(boxes.some((b) => b.includes('codexChecked')), 'Codex 的 checkbox');
  assert.ok(boxes.some((b) => b.includes('minimaxChecked')), 'MiniMax Desktop 的 checkbox');
  // 两个都必须在变化时触发重扫——只改本地状态不落盘，关掉窗口就丢。
  for (const box of boxes) {
    assert.match(box, /@change="onSourceToggle"/, '勾选变化必须走 onSourceToggle');
  }
  assert.match(template, /v-model="codexChecked"/);
  assert.match(template, /v-model="minimaxChecked"/);
});

test('外部来源在详情页里是独立一块，不与内核那份用量相加', () => {
  assert.match(template, /外部 Agent 工具用量/, '独立一块');
  assert.match(template, /口径与上面那份不同，不相加/, '说清为什么不相加');
  // 外部那一块**只读自己那份**的合计，不碰内核那份的 `rangeSummary`——
  // 两套口径加到一起没有意义。
  const start = template.indexOf('外部 Agent 工具用量');
  assert.ok(start > 0, '外部来源那一块必须存在');
  const section = template.slice(start);
  assert.doesNotMatch(section, /rangeSummary/, '外部用量不进内核那份的汇总');
  assert.match(section, /src\.tokens/, '外部那一块用自己的合计');
});

// --- 外部卡片的数字必须跟着时间范围走 ----------------------------------------
//
// 内核那几块（热力图 / 趋势 / 模型用量）全都跟着范围切换器走。外部那块此前
// **不跟**：大数字与模型列表是后端给的 90 天合计，旁边的柱状图却是按范围切
// 的——一张卡里并存两个窗口，用户切到「今日」时看到的仍是 90 天的数
// （2026-10-24 评审发现）。
//
// 这条判据钉的是**口径来源**：卡片的数据必须由「按范围切片的日序列」聚合
// 出来；后端那份 90 天合计只许留在 tooltip 里（说清两个数各是什么）。
test('外部卡片的数字跟着时间范围走：大数字、请求数、活跃日都是范围口径', () => {
  const script = windowSource.slice(
    windowSource.indexOf('<script setup>'),
    windowSource.indexOf('</script>')
  );
  assert.match(
    script,
    /sliceDays\(src\.days, rangeDays\.value\)/,
    '外部卡片必须按当前范围切片（与内核同一套 sliceDays）'
  );
  assert.match(script, /summarizeDays\(rows\)/, '外部卡片必须按当前范围汇总');

  const secStart = template.indexOf('<section v-if="externalCards.length"');
  assert.ok(secStart > 0, '外部来源那一块必须存在');
  const section = template.slice(secStart, template.indexOf('</section>', secStart));
  assert.match(
    section,
    /formatTokens\(card\.summary\.tokens\)/,
    '大数字必须是范围口径（card.summary.tokens）'
  );
  assert.doesNotMatch(
    section,
    /formatTokens\(card\.src\.tokens\)/,
    '后端那份 90 天合计不能当大数字——那样切范围时它纹丝不动'
  );
  assert.match(section, /card\.src\.tokens/, '90 天合计仍要留在 tooltip 里说清');
  assert.match(section, /card\.summary\.requests/, '请求次数也是范围口径');
  assert.match(section, /card\.summary\.activeDays/, '活跃日也是范围口径');
  // 模型明细现在是**合出来的那张表**（`externalModels`）：它必须由范围口径的
  // 那份（`card.summary.models`）合出来，不能直接吃后端那份 90 天的。
  assert.match(
    script,
    /card\.summary\.models/,
    '合并后的模型表必须由范围口径的 card.summary.models 合出来'
  );
  assert.match(
    section,
    /v-for="\(m, index\) in externalModels"/,
    '模型明细渲染的是合并后的那张表'
  );
});

// --- 跨语言契约：模板读的每个字段，后端都得真有 --------------------------------
//
// 上一条只钉住「模板里出现了 `src.tokens`」，**没钉住后端真有这个字段**。
// 后端把 `tokens` 改名成 `total`，模板不会报任何错——`{{ formatTokens(undefined) }}`
// 渲染成 "0"，界面看着「用量是 0」，实则字段对不上。这是**假门禁**：判据绿着，
// 功能已经坏了。
//
// 判据取「前端读的字段 ⊆ 后端结构体的字段」这一个方向：前端少读一个字段没关系
// （可能是后端多给了），但**前端读的那个字段后端必须真有**。
test('模板读的每个外部字段，后端结构体里都得真有', () => {
  const rustPath = new URL('../../src-tauri/src/usage/external/mod.rs', import.meta.url);
  const rust = readFileSync(rustPath, 'utf8');

  // 取一个结构体的 `pub <字段>:` 列表。
  function rustFields(structName) {
    const begin = rust.indexOf(`pub struct ${structName} {`);
    assert.ok(begin > 0, `后端得真有 ${structName} 结构体`);
    const body = rust.slice(begin, rust.indexOf('\n}', begin));
    const fields = new Set();
    for (const m of body.matchAll(/pub\s+(\w+)\s*:/g)) fields.add(m[1]);
    return fields;
  }

  const tool = rustFields('ExternalToolUsage');
  const model = rustFields('ExternalModelUsage');
  const day = rustFields('ExternalDayUsage');

  // 模板里外部来源那一块（`externalCards` 那个 section；它内部没有嵌套 section，
  // 所以第一个 </section> 就是它的收尾）。
  const secStart = template.indexOf('<section v-if="externalCards.length"');
  assert.ok(secStart > 0, '外部来源那一块必须存在');
  const section = template.slice(secStart, template.indexOf('</section>', secStart));

  // 模板里出现的 `src.X` / `m.X` / `row.X`。
  function accesses(prefix) {
    return new Set([...section.matchAll(new RegExp(`\\b${prefix}\\.(\\w+)`, 'g'))].map((m) => m[1]));
  }

  // `row.*` 来自 externalDays()：它把 `source.days`（ExternalDayUsage）映射成
  // {date, tokens, requests, height}——height 是前端自己算的，不在后端。
  const dayExtra = new Set(['height']);

  // `m.*` 现在是**前端合出来的那一行**（`externalModels`）：后端给的
  // model / provider / tokens，加上前端自己补的 tool（来自哪个工具）与
  // toolTotal（那个工具自己的合计，算占比的分母）。两者来源不同，**分开钉**：
  // 后端那几个必须在后端结构体里，前端补的那几个必须在前端那段计算里。
  const modelExtra = new Set(['tool', 'toolTotal']);
  const script = windowSource.slice(
    windowSource.indexOf('<script setup>'),
    windowSource.indexOf('</script>')
  );
  const builder = script.slice(script.indexOf('const externalModels = computed('));
  for (const field of modelExtra) {
    assert.ok(
      new RegExp(`\\b${field}:`).test(builder),
      `模板读了 m.${field}，它得由前端的 externalModels 那段算出来——` +
        '不是后端字段，后端结构体里当然没有'
    );
  }

  for (const [prefix, fields, structName] of [
    ['src', tool, 'ExternalToolUsage'],
    ['m', model, 'ExternalModelUsage'],
    ['row', day, 'ExternalDayUsage'],
  ]) {
    for (const field of accesses(prefix)) {
      if (prefix === 'row' && dayExtra.has(field)) continue;
      if (prefix === 'm' && modelExtra.has(field)) continue;
      assert.ok(
        fields.has(field),
        `模板读了 ${prefix}.${field}，但后端 ${structName} 没有这个字段——` +
          '字段对不上时界面渲染成 0 且不报任何错'
      );
    }
  }

  // 反向也钉一下：后端给了的这几个关键字段，模板必须真的用上（否则多半是
  // 模板写错了变量名，只是碰巧没崩）。
  for (const field of ['source', 'label', 'tokens', 'requests', 'active_days', 'tracked_files', 'note']) {
    assert.ok(accesses('src').has(field), `模板没读 src.${field}——后端给了却没用`);
  }
});

// --- 外部来源那一块必须滚得到 --------------------------------------------------
//
// 外部段是 `<main>` 的**最后一个**子元素。760×800 的窗口装不下「概览 + 趋势 +
// 模型明细 + 外部卡片」四段，而 `.usagewin-main` 此前是 `overflow-y: hidden`
// ——超出的那一段被整段裁掉：实测外部段的标题露在页脚上方、两张卡片只剩
// 一条边，用户永远看不到（2026-10-24 按真机几何渲染实测）。
//
// 全局 `theme.css` 里本来就有 `main { overflow-y: auto }`——主壳各面板的约定
// 就是「内容装不下就滚」。副窗这条 `hidden` 把全局约定覆盖掉了，于是新增的
// 段落直接被裁。修法是回到全局约定，而不是另发明一套滚动语义。
//
// 判据走**层叠后的生效值**：只在源码里找 `overflow-y: auto` 不算数——别的
// 规则再写一条 `hidden` 就把它盖回去了，而那正是当初出事的那条。
test('外部来源那一块必须滚得到：main 的纵向溢出不能是 hidden', () => {
  const rules = allRulesIncludingVue();
  assert.equal(
    effectiveDeclaration(['main', 'usagewin-main'], rules, 'overflow-y'),
    'auto',
    'main 装不下时必须能滚过去，否则外部来源那一段被整段裁掉、用户永远看不到'
  );

  // 外部段确实在 main 里（不是塞在页脚之类的地方）：main 才滚得到它。
  const secStart = template.indexOf('<section v-if="externalCards.length"');
  const mainEnd = template.indexOf('</main>');
  assert.ok(secStart > 0, '外部来源那一块必须存在');
  assert.ok(secStart < mainEnd, '外部来源那一块必须在 main 里，滚 main 才滚得到它');
});

// --- 每块数据都要说清是哪个来源的 ----------------------------------------------
//
// 这屏同时摆着**两套账**：本壳内核的用量，与勾选的外部 Agent 工具的用量。
// 两边名字都带「MiniMax」时（内核自己也在调 MiniMax 的模型），用户分不清
// 哪一行是谁的账——2026-10-24 用户就是这么问的。所以每一块都得挂来源牌：
// 内核那几块挂「内核」，外部那块由卡片标题给出工具名。
test('每块数据都标了来源：内核那几块挂「内核」，外部那块给出工具名', () => {
  const externalStart = template.indexOf('外部 Agent 工具用量');
  assert.ok(externalStart > 0, '外部来源那一块必须存在');

  // 内核那几块：热力图 / 按天趋势 / 模型用量，各挂一枚「内核」牌。
  const kernelPart = template.slice(0, externalStart);
  const chipParts = kernelPart.split('class="usage-source-chip"').slice(1);
  assert.ok(
    chipParts.length >= 3,
    `内核那几块（热力图 / 按天趋势 / 模型用量）都得挂来源牌，只挂了 ${chipParts.length} 枚`
  );
  for (const part of chipParts) {
    const chip = part.slice(0, part.indexOf('</span>'));
    assert.match(chip, /内核/, '来源牌必须写明是内核的用量');
  }

  // 外部那一块不挂「内核」牌——它的来源是工具名，由卡片标题给出。
  const externalPart = template.slice(externalStart);
  assert.doesNotMatch(
    externalPart,
    /class="usage-source-chip"/,
    '外部那一块的来源是工具名，不该再挂一枚「内核」牌'
  );
  assert.match(
    externalPart,
    /class="usage-external-name"[^>]*>\s*\{\{\s*card\.src\.label\s*\}\}/,
    '外部那张卡的标题必须读工具名（Codex / MiniMax Desktop）'
  );

  // 行的悬停说明也要带来源：外部的行说明是哪个工具的，内核的行说明是内核的
  // provider——两边的 provider 名字都叫 minimax 一类，光看列名分不出来。
  //
  // 外部那一块的模型行**合到一张表**（2026-10-25 起），每行自带一列「工具」，
  // 所以来源要在这三处都能读到：模型名的 title、工具列、provider 的 title。
  assert.match(
    externalPart,
    /:title="`\$\{m\.tool\} · \$\{m\.model \|\| m\.key\}`"/,
    '外部模型行的模型名悬停说明必须带工具名'
  );
  assert.match(
    externalPart,
    /class="usage-model-tool"[^>]*:title="`来自 \$\{m\.tool\}`"/,
    '外部模型行必须有一列「工具」'
  );
  assert.match(
    externalPart,
    /:title="`\$\{m\.tool\} 的 provider · \$\{m\.provider\}`"/,
    '外部模型行的 provider 必须说明是哪个工具的'
  );
  assert.match(
    kernelPart,
    /:title="`内核的 provider · \$\{m\.provider\}`"/,
    '内核模型行的 provider 必须说明它是内核的'
  );
});

// --- 几个外部工具合在一张卡里，模型明细合成一张表 ------------------------------
//
// 2026-10-25 用户提的：「数据多了会撑出去，把两个 agent 工具整合在一个里面显示」。
// 起因是实测的截图：勾上 Codex 与 MiniMax Desktop 两个来源、90 天的范围下，
// 那两块**并排**放在 760px 的窗口里，每块只剩 340px——90 天那条柱状图每根柱
// 不到 2px、模型名被截成「MiniMax-M3.1-Fla…」、百分比贴着卡片边缘。
//
// 三条钉：
//  ① **一个容器**：`.usage-external` 不许再是多列栅格（`auto-fit` / `minmax`），
//     否则一勾两个来源就又变回并排。
//  ② **模型明细合成一张表**，带一列「工具」——合表之后每行必须自带来源。
//  ③ **这张表自己滚**（`max-height` + `overflow-y: auto`）：模型行一多，页面被
//     顶长之后下面的东西全被推到视口外——那正是「撑出去」。
test('外部那几个工具合在一张卡里，模型明细合成一张带「工具」列的表', () => {
  const rules = allRulesIncludingVue();

  // ① 一个容器：不是多列栅格。
  const gridTemplate = effectiveDeclaration(['usage-external'], rules, 'grid-template-columns');
  assert.ok(
    gridTemplate === null || !/auto-fit|repeat\(/.test(gridTemplate),
    `外部那几个工具必须合成一张卡占满整幅宽度——现在还是多列栅格（${gridTemplate}），` +
      '两个来源并排时每块只剩 340px，模型名被截断、柱条不到 2px'
  );
  // 那张卡得是**一张卡**（有边框有底色），不是几块拼起来。
  assert.ok(
    effectiveDeclaration(['usage-external'], rules, 'border') !== null,
    '外部那几个工具合成的那张卡得有边框'
  );

  // ② 模型明细合成一张表，带「工具」列。
  const secStart = template.indexOf('<section v-if="externalCards.length"');
  assert.ok(secStart > 0, '外部来源那一块必须存在');
  const section = template.slice(secStart, template.indexOf('</section>', secStart));
  assert.match(
    section,
    /v-for="\(m, index\) in externalModels"/,
    '模型明细必须渲染合并后的那一张表'
  );
  assert.match(section, /class="usage-model-tool"/, '合表之后每行必须有一列「工具」');
  assert.doesNotMatch(
    section,
    /v-for="\(m, index\) in card\.summary\.models"/,
    '模型明细不许再按工具各画一张表——那正是并排挤爆的形状'
  );

  // ③ 这张表自己滚，不许把整页撑长。
  assert.ok(
    effectiveDeclaration(['usage-external-models'], rules, 'overflow-y') === 'auto',
    '合并后的模型表必须自己滚——否则模型行一多，整页被顶长，下面的内容全被推出视口'
  );
  assert.ok(
    effectiveDeclaration(['usage-external-models'], rules, 'max-height') !== null,
    '合并后的模型表必须有高度上限，否则它照旧把整页撑长'
  );
});
