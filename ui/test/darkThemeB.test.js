// 「蓝墨夜航」的契约：暗色 token + **两套主题共用同一套账页布局**。
// 浅色 Kami 的颜色断言在 designAlignment.test.js；这里钉「主题只换颜色」这条
// 边界——几何（位置 / 尺寸 / 字号 / 间距）一律写在两套主题共用的选择器上，
// `html.dark` 里不许再出现会改变布局的声明，否则切一次主题整页重排。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const theme = readFileSync(resolve('ui/src/theme.css'), 'utf8');
const dark = theme.match(/html\.dark\s*\{([\s\S]*?)\n\}/)[1];
const root = theme.match(/:root\s*\{([\s\S]*?)\n\}/)[1];
const declaration = (block, name) => block.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1].trim();

test('暗色蓝墨夜航的画布 / 雾银蓝 / 暖纸文字 token', () => {
  assert.equal(declaration(dark, '--canvas'), '#1c222a');
  assert.equal(declaration(dark, '--chrome'), '#15191f');
  assert.equal(declaration(dark, '--surface'), '#222b34');
  assert.equal(declaration(dark, '--accent'), '#b2c8e5');
  assert.equal(declaration(dark, '--success'), '#a3cebc');
  assert.equal(declaration(dark, '--border'), '#343f4d');
  assert.equal(declaration(root, '--font-serif'), '"Songti SC", "STSong", "Noto Serif CJK SC", Georgia, serif');
});

test('品牌字标与鲸标：亮色朱砂红，暗色珊瑚红 + 鲸体反色融合', () => {
  assert.equal(declaration(root, '--brand-wordmark'), '#a83e43');
  assert.equal(declaration(dark, '--brand-wordmark'), '#e69a98');
  const name = theme.match(/\n\.brand__name \{([^}]*)\}/)[1];
  assert.match(name, /color:\s*var\(--brand-wordmark\)/, '品牌字标读 --brand-wordmark');
  assert.doesNotMatch(name, /whale-eye-bright/, '鲸眼红留给鲸眼 / 活动指示，不再兼作字标');
  const img = theme.match(/\n\.brand img \{([^}]*)\}/)[1];
  assert.doesNotMatch(img, /background:|box-shadow:/, '鲸标不再铺暖灰标底');
  assert.doesNotMatch(theme, /--brand-mark-plate/, '标底 token 已随用户反馈移除');
  assert.match(
    theme,
    /\nhtml\.dark \.brand img \{\s*filter:\s*invert\(1\) hue-rotate\(180deg\);\s*\}/,
    '暗色只反色，不改几何 / 图源',
  );
});

test('亮色主按钮仍走原墨蓝档：Element Plus 的底色覆写只在 html.dark', () => {
  assert.equal(declaration(root, '--primary-bg'), '#1b365d');
  assert.equal(declaration(root, '--primary-bg-hover'), '#12263f');
  assert.equal(declaration(root, '--primary-bg-active'), '#12263f');
  assert.doesNotMatch(theme, /\n\.el-button--primary \{[^}]*--el-button-bg-color/, '亮色继续吃 Element Plus 自己的派生档');
  assert.match(
    theme,
    /\nhtml\.dark \.el-button--primary \{[^}]*--el-button-bg-color: var\(--primary-bg\)/,
    '暗色主按钮底色只在暗色覆写',
  );
});

test('主按钮底色与文字各有独立语义，暗色不用 accent 直接铺底', () => {
  assert.equal(declaration(root, '--primary-bg'), '#1b365d');
  assert.equal(declaration(dark, '--primary-bg'), '#334b64');
  assert.equal(declaration(dark, '--primary-bg-hover'), '#3d576f');
  assert.equal(declaration(dark, '--primary-bg-active'), '#293f55');
  assert.equal(declaration(dark, '--primary-text'), '#b2c8e5');
  assert.equal(declaration(dark, '--on-accent'), '#dde9f7');
  assert.doesNotMatch(theme, /html\.dark \.main-view \.btn--primary/, '旧黄铜主按钮覆写必须删除');
  const btn = theme.match(/\n\.btn--primary \{([^}]*)\}/)[1];
  assert.match(btn, /background:\s*var\(--primary-bg\)/);
  assert.match(btn, /color:\s*var\(--primary-text\)/);
});

test('暗色红点角标与用量档位不拿浅色状态色配白字', () => {
  const badge = theme.match(/\nhtml\.dark \.nav-item__badge \{([^}]*)\}/)[1];
  assert.match(badge, /color:\s*var\(--canvas\)/, '珊瑚红底上的文字改用深色');
  const usage = readFileSync(resolve('ui/src/usage/UsageWindow.vue'), 'utf8');
  const range = usage.match(/\.usage-range-btn\.active \{([^}]*)\}/)[1];
  assert.match(range, /background:\s*var\(--primary-bg\)/, '选中档位走主按钮底');
  assert.match(range, /color:\s*var\(--primary-text\)/, '选中档位文字不写死白色');
});

test('两套主题共用宋体页标题与 31px 档位', () => {
  const title = theme.match(/\n\.page-title\s*\{([^}]*)\}/)[1];
  assert.match(title, /font-family:\s*var\(--font-serif\)/);
  assert.match(title, /font-size:\s*var\(--fs-page-title\)/);
  assert.match(title, /font-weight:\s*600/);
  assert.match(theme, /--fs-page-title:\s*var\(--fs-11\)/);
});

test('账页卡是两套主题共用的（不能再有 html.dark 前缀）', () => {
  const card = theme.match(/\n\.main-view \.card\s*\{([^}]*)\}/);
  assert.ok(card, '找不到两套主题共用的账页卡规则 .main-view .card');
  assert.match(card[1], /background:\s*transparent/);
  assert.match(card[1], /border:\s*0/);
  assert.match(card[1], /border-radius:\s*0/);
  assert.match(card[1], /box-shadow:\s*0 1px 0 var\(--border-soft\)/);
  assert.match(card[1], /padding:\s*14px 0/, '账页段的上下内边距由卡片自己提供');
  assert.doesNotMatch(theme, /html\.dark \.main-view \.card\s*\{/, '账页卡不许再退回暗色覆盖');
});

test('html.dark 只换颜色：不得出现 padding / gap / font-size / 圆角等几何声明', () => {
  // 两套主题的几何完全一致（2026-11-02 用户要求浅色也呈现账页风格）。
  // 暗色段里出现这些声明 = 切主题会重排，是本次要防的回归。
  const geometry =
    /(?:^|;)\s*(?:padding|margin|gap|font-size|line-height|border-radius|height|width|display|align-items|justify-content|grid-template[^:]*)\s*:/;
  const darkRules = [...theme.matchAll(/(^|\n)(html\.dark[^{]*)\{([^}]*)\}/g)];
  assert.ok(darkRules.length >= 1, `应找到暗色规则，实际 ${darkRules.length} 条`);
  for (const [, , selector, body] of darkRules) {
    assert.doesNotMatch(
      body,
      geometry,
      `${selector.trim()} 在 html.dark 里改了布局 / 字号——两套主题必须共用同一套几何`,
    );
  }
});

test('两套主题共用描边方片图标，只换 accent 颜色', () => {
  const icon = theme.match(/\n\.card-title-icon \{([^}]*)\}/)[1];
  assert.match(icon, /width:\s*var\(--size-block-icon\)/);
  assert.match(icon, /border:\s*1px solid var\(--accent-line\)/);
  assert.match(icon, /background:\s*var\(--accent-soft\)/);
  assert.match(icon, /color:\s*var\(--accent\)/);
  assert.doesNotMatch(icon, /background:\s*var\(--accent\);/);
  assert.match(theme, /--size-block-icon:\s*22px/);
});

test('八个面板的页头改用与侧栏菜单同源的图标，不再画 01/02 编号', () => {
  // 2026-11-02 用户要求：页头那个 01 / 02 编号换成菜单对应的图标，两边一致。
  // 一致性靠单一来源：图标只登记在 shell/pageIcons.js（侧栏与页头共用）。
  const pages = [
    ['ui/src/shell/OverviewPanel.vue', 'overview'],
    ['ui/src/usage/UsageAnalysisPanel.vue', 'usage'],
    ['ui/src/kernel/VersionsPanel.vue', 'versions'],
    ['ui/src/plugins/PluginsPanel.vue', 'plugins'],
    ['ui/src/skills/SkillsPanel.vue', 'skills'],
    ['ui/src/memory/MemoryPanel.vue', 'memory'],
    ['ui/src/shell/SettingsPanel.vue', 'settings'],
  ];
  for (const [file, page] of pages) {
    const tpl = readFileSync(resolve(file), 'utf8');
    assert.ok(tpl.includes(`<PageHeadIcon :page="'${page}'" />`), `${file} 页头要挂菜单图标`);
    assert.doesNotMatch(tpl, /data-page=/, `${file} 不许再画编号`);
  }
  // 样式也只剩图标那一档，编号的 ::before 不许回来。
  assert.doesNotMatch(theme, /\.page-head__text::before/);
  assert.match(theme, /\n\.page-head__icon \{/);
});
